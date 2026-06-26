"""Corpus loading + collation for training.

A corpus is JSONL of `{obs, legal, policy, to_act, value}` (from `kdagent.selfplay` or the
Rust `selfplay_batch`). Records store the **raw** inputs, so improving the feature schema
never invalidates a corpus — each minibatch is re-encoded through `encoder.encode_obs` at
train time. `collate` pads the variable-length action lists into index tensors so the policy
logits can be gathered for the whole batch at once (no Python per-sample loop in the hot path).
"""
from __future__ import annotations

import json
from dataclasses import dataclass

import numpy as np
import torch

from .encoder import A_CLAIM, A_PLACE, N_PLANES, STORE, encode_obs


def load_corpus(path: str, limit: int | None = None) -> list[dict]:
    recs = []
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            recs.append(json.loads(line))
            if limit and len(recs) >= limit:
                break
    return recs


@dataclass
class Batch:
    board: torch.Tensor      # [B, pc·N_PLANES, 13, 13] float32
    lines: torch.Tensor      # [B, 8, F] float32
    glob: torch.Tensor       # [B, G] float32
    a_type: torch.Tensor     # [B, Amax] int (A_PLACE/A_CLAIM/A_DISCARD; -1 = pad)
    a_pidx: torch.Tensor     # [B, Amax] int, place flat index rot·169+row·13+col (else 0)
    a_ltok: torch.Tensor     # [B, Amax] int, claim line-token 0..7 (else 0)
    a_mask: torch.Tensor     # [B, Amax] bool, real (non-pad) action
    policy: torch.Tensor     # [B, Amax] float32, MCTS visit-distribution target (0 in pad)
    value_rel: torch.Tensor  # [B, pc] float32, seat-relative outcome target (self first)
    pc: int

    def to(self, device) -> "Batch":
        return Batch(
            self.board.to(device), self.lines.to(device), self.glob.to(device),
            self.a_type.to(device), self.a_pidx.to(device), self.a_ltok.to(device),
            self.a_mask.to(device), self.policy.to(device), self.value_rel.to(device), self.pc,
        )

    def __len__(self) -> int:
        return self.board.size(0)


def collate(records: list[dict], table=None, pc: int = 2) -> Batch:
    """Encode + pad a list of corpus records into a `Batch`. Records whose player count differs
    from `pc` are skipped (the net is built for one player count)."""
    encs, pols, vals, toacts = [], [], [], []
    for r in records:
        enc = encode_obs(r["obs"], r["legal"], table)
        if enc.player_count != pc:
            continue
        encs.append(enc)
        pols.append(np.asarray(r["policy"], dtype=np.float32))
        vals.append(np.asarray(r["value"], dtype=np.float32))
        toacts.append(r["to_act"])
    if not encs:
        raise ValueError(f"no records with player_count == {pc}")

    b = len(encs)
    amax = max(len(e.actions.type_id) for e in encs)
    c = N_PLANES
    f = encs[0].lines.shape[1]
    g = encs[0].glob.shape[0]

    board = np.zeros((b, pc * c, STORE, STORE), dtype=np.float32)
    lines = np.zeros((b, 8, f), dtype=np.float32)
    glob = np.zeros((b, g), dtype=np.float32)
    a_type = np.full((b, amax), -1, dtype=np.int64)
    a_pidx = np.zeros((b, amax), dtype=np.int64)
    a_ltok = np.zeros((b, amax), dtype=np.int64)
    a_mask = np.zeros((b, amax), dtype=bool)
    policy = np.zeros((b, amax), dtype=np.float32)
    value_rel = np.zeros((b, pc), dtype=np.float32)

    for i, enc in enumerate(encs):
        board[i] = enc.board.reshape(pc * c, STORE, STORE)
        lines[i] = enc.lines
        glob[i] = enc.glob
        act = enc.actions
        n = len(act.type_id)
        a_type[i, :n] = act.type_id
        place = act.type_id == A_PLACE
        a_pidx[i, :n] = np.where(place, act.rot * STORE * STORE + act.row * STORE + act.col, 0)
        claim = act.type_id == A_CLAIM
        a_ltok[i, :n] = np.where(claim, np.clip(act.line_tok, 0, None), 0)
        a_mask[i, :n] = True
        policy[i, :n] = pols[i]
        ta = toacts[i]
        for k in range(pc):
            value_rel[i, k] = vals[i][(ta + k) % pc]

    return Batch(
        torch.from_numpy(board), torch.from_numpy(lines), torch.from_numpy(glob),
        torch.from_numpy(a_type), torch.from_numpy(a_pidx), torch.from_numpy(a_ltok),
        torch.from_numpy(a_mask), torch.from_numpy(policy), torch.from_numpy(value_rel), pc,
    )
