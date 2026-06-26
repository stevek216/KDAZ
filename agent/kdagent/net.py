"""The Kingdomino network (feature-schema.md §6–§7): a conv tower over the per-seat board
planes, a small MLP over the 8 draft tokens and the global vector, and three policy heads —
a spatial **place** map (rotation × cell), a **claim** pointer over line tokens, and a
**discard** scalar — plus a seat-relative (max-n) **value** head.

`policy_value(enc, device)` returns `(action_logits [A], value_logits [P])` aligned to the
encoder's action batch — exactly what the MCTS leaf evaluator needs (softmax of the logits
is the prior; softmax of the value is the seat-relative outcome distribution).
"""
from __future__ import annotations

import numpy as np
import torch
import torch.nn as nn

from .encoder import A_CLAIM, A_DISCARD, A_PLACE, N_PLANES, STORE, global_dim

SCALABLE_KEYS = ("ch", "board_blocks")


class KingdominoNet(nn.Module):
    def __init__(self, player_count: int = 2, ch: int = 64, board_blocks: int = 3):
        super().__init__()
        self.pc = player_count
        self.ch = ch
        in_ch = player_count * N_PLANES
        blocks = [nn.Conv2d(in_ch, ch, 3, padding=1), nn.GELU()]
        for _ in range(board_blocks - 1):
            blocks += [nn.Conv2d(ch, ch, 3, padding=1), nn.GELU()]
        self.board_conv = nn.Sequential(*blocks)
        self.place_head = nn.Conv2d(ch, 4, 1)  # -> [4, 13, 13]: rotation × anchor cell
        self.line_mlp = nn.Sequential(
            nn.Linear(self._line_feats(), ch), nn.GELU(), nn.Linear(ch, ch), nn.GELU()
        )
        self.claim_head = nn.Linear(ch, 1)
        self.glob_mlp = nn.Sequential(nn.Linear(global_dim(player_count), ch), nn.GELU())
        # summary = board pool ⊕ line pool ⊕ global  (3·ch)
        self.discard_head = nn.Sequential(nn.Linear(3 * ch, ch), nn.GELU(), nn.Linear(ch, 1))
        self.value_head = nn.Sequential(
            nn.Linear(3 * ch, ch), nn.GELU(), nn.Linear(ch, player_count)
        )

    @staticmethod
    def _line_feats() -> int:
        from .encoder import LINE_FEATS

        return LINE_FEATS

    def heads(self, board: torch.Tensor, lines: torch.Tensor, glob: torch.Tensor):
        """board [1, pc·C, 13, 13], lines [8, F], glob [G] -> raw heads."""
        feat = self.board_conv(board)  # [1, ch, 13, 13]
        place_map = self.place_head(feat)[0]  # [4, 13, 13]
        board_pool = feat.mean(dim=(2, 3))[0]  # [ch]
        line_emb = self.line_mlp(lines)  # [8, ch]
        claim_logits = self.claim_head(line_emb).squeeze(-1)  # [8]
        line_pool = line_emb.mean(dim=0)  # [ch]
        glob_emb = self.glob_mlp(glob)  # [ch]
        summary = torch.cat([board_pool, line_pool, glob_emb], dim=-1)  # [3·ch]
        discard_logit = self.discard_head(summary).squeeze(-1)  # scalar
        value_logits = self.value_head(summary)  # [pc]
        return place_map, claim_logits, discard_logit, value_logits

    def forward_batch(self, board: torch.Tensor, lines: torch.Tensor, glob: torch.Tensor):
        """Batched heads for training. board [B, pc·C, 13, 13], lines [B, 8, F], glob [B, G]
        -> place_map [B, 4, 13, 13], claim_logits [B, 8], discard [B], value [B, pc]."""
        feat = self.board_conv(board)  # [B, ch, 13, 13]
        place_map = self.place_head(feat)  # [B, 4, 13, 13]
        board_pool = feat.mean(dim=(2, 3))  # [B, ch]
        line_emb = self.line_mlp(lines)  # [B, 8, ch]
        claim_logits = self.claim_head(line_emb).squeeze(-1)  # [B, 8]
        line_pool = line_emb.mean(dim=1)  # [B, ch]
        glob_emb = self.glob_mlp(glob)  # [B, ch]
        summary = torch.cat([board_pool, line_pool, glob_emb], dim=-1)  # [B, 3·ch]
        discard = self.discard_head(summary).squeeze(-1)  # [B]
        value = self.value_head(summary)  # [B, pc]
        return place_map, claim_logits, discard, value

    def policy_value(self, enc, device: str = "cpu"):
        """Per-action logits + value for one encoded state (the MCTS leaf interface)."""
        board = torch.from_numpy(enc.board).reshape(1, self.pc * N_PLANES, STORE, STORE).to(device)
        lines = torch.from_numpy(enc.lines).to(device)
        glob = torch.from_numpy(enc.glob).to(device)
        place_map, claim_logits, discard_logit, value_logits = self.heads(board, lines, glob)

        acts = enc.actions
        a = len(acts.type_id)
        t = torch.from_numpy(acts.type_id).to(device)
        place_flat = place_map.reshape(-1)  # index = rot·169 + row·13 + col
        pidx = (
            torch.from_numpy(acts.rot).to(device) * STORE * STORE
            + torch.from_numpy(acts.row).to(device) * STORE
            + torch.from_numpy(acts.col).to(device)
        ).clamp(min=0)
        ltok = torch.from_numpy(acts.line_tok).to(device).clamp(min=0)

        logits = torch.zeros(a, device=device)
        logits = torch.where(t == A_PLACE, place_flat[pidx], logits)
        logits = torch.where(t == A_CLAIM, claim_logits[ltok], logits)
        logits = torch.where(t == A_DISCARD, discard_logit.expand(a), logits)
        return logits, value_logits

    def num_params(self) -> int:
        return sum(p.numel() for p in self.parameters())


def load_net(path, device="cpu"):
    """Load a checkpoint into a correctly-sized `KingdominoNet` (architecture from the saved
    `net_cfg`, defaults for old checkpoints). Returns `(net.eval(), ckpt_dict)`."""
    ck = torch.load(path, map_location=device, weights_only=False)
    cfg = ck.get("net_cfg", {}) if isinstance(ck, dict) else {}
    net = KingdominoNet(**cfg).to(device)
    sd = ck["model"] if isinstance(ck, dict) and "model" in ck else ck
    net.load_state_dict(sd)
    return net.eval(), ck
