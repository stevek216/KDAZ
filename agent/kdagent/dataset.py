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

# Final scores are divided by this for the auxiliary score-head target (keeps the Huber
# loss in a sane range; typical totals run ~40-160).
SCORE_SCALE = 50.0


def load_corpus(path: str, limit: int | None = None) -> list[dict]:
    """Load a JSONL corpus into memory as dicts.

    Legacy path, kept for the `.jsonl` debugging form. It costs ~18.4 KiB per record resident,
    so it cannot hold a real generation — training reads packed corpora through
    `PackedCorpus` + `collate_packed` instead.
    """
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


class Corpora:
    """One or more corpora addressed as a single 0..N index space.

    Packed corpora are memory-mapped, so constructing this over a replay window of several
    generations is instant and costs no resident memory per record. The legacy JSONL form is
    still accepted (it loads into dicts) so `.jsonl` debugging corpora keep working, but the
    two forms cannot be mixed in one window.
    """

    def __init__(self, paths: list[str], limit: int | None = None):
        from .corpus import PackedCorpus, is_packed

        flags = [is_packed(p) for p in paths]
        if any(flags) and not all(flags):
            raise SystemExit(
                "cannot mix packed and JSONL corpora in one window; regenerate the odd one out"
            )
        self.packed = bool(paths) and all(flags)
        if self.packed:
            self.parts = [PackedCorpus(p) for p in paths]
            sizes = [len(c) for c in self.parts]
            pcs = {c.player_count for c in self.parts}
            if len(pcs) > 1:
                raise SystemExit(f"corpora disagree on player_count: {sorted(pcs)}")
            self.player_count = self.parts[0].player_count
        else:
            self.records: list[dict] = []
            for p in paths:
                self.records += load_corpus(p, limit=limit)
            sizes = [len(self.records)]
            self.player_count = None
        self.bounds = np.cumsum([0] + sizes)
        self.n = int(self.bounds[-1])
        if limit:
            self.n = min(self.n, limit)

    def __len__(self) -> int:
        return self.n

    @property
    def game_ids(self) -> np.ndarray:
        """Per-record generating-game id, used to split by game rather than by position.

        Ids from different corpora are not re-salted: a collision would merely force two
        unrelated games onto the same side of the split, which is conservative — it can never
        split one game across train and val, which is the leak that matters.
        """
        if self.packed:
            return np.concatenate([c.game_ids for c in self.parts])[: self.n]
        return np.array(
            [hash(r.get("game", f"_solo{i}")) for i, r in enumerate(self.records[: self.n])],
            dtype=np.int64,
        )

    def batch(self, idx: np.ndarray, pc: int) -> "Batch":
        """Encode the records at global indices `idx`."""
        if not self.packed:
            return collate([self.records[i] for i in idx], pc=pc)
        idx = np.asarray(idx, dtype=np.int64)
        part = np.searchsorted(self.bounds, idx, side="right") - 1
        local = idx - self.bounds[part]
        rec_size = self.parts[0].rec_size
        records = np.empty((len(idx), rec_size), dtype=np.uint8)
        counts = np.empty(len(idx), dtype=np.int64)
        for j, (p, l) in enumerate(zip(part, local)):
            records[j] = self.parts[p].records[l]
            counts[j] = self.parts[p].n_actions[l]
        offsets = np.zeros(len(idx) + 1, dtype=np.int64)
        np.cumsum(counts, out=offsets[1:])
        policy = np.empty(int(offsets[-1]), dtype=np.float32)
        for j, (p, l) in enumerate(zip(part, local)):
            policy[offsets[j]:offsets[j + 1]] = self.parts[p].policy_for(l)
        return _batch_from_rust(records, policy, offsets, pc)


def collate_packed(corpus, idx: np.ndarray, pc: int = 2) -> "Batch":
    """Encode the records at `idx` of a `PackedCorpus` into a `Batch`.

    All the per-record work (decode state, re-derive legal actions, build planes and action
    descriptors) happens in one Rust call across the whole minibatch, so no Python object is
    created per record. Produces exactly what `collate` produces for the same positions.
    """
    import kingdomino as kd

    idx = np.ascontiguousarray(idx, dtype=np.int64)
    records = np.ascontiguousarray(corpus.records[idx])
    # Gather this batch's ragged policy slices into one flat buffer + offsets.
    counts = corpus.n_actions[idx]
    offsets = np.zeros(len(idx) + 1, dtype=np.int64)
    np.cumsum(counts, out=offsets[1:])
    policy = np.empty(int(offsets[-1]), dtype=np.float32)
    for j, i in enumerate(idx):
        policy[offsets[j]:offsets[j + 1]] = corpus.policy[
            corpus.offsets[i]:corpus.offsets[i + 1]
        ]

    return _batch_from_rust(records, policy, offsets, pc)


def _batch_from_rust(records: np.ndarray, policy: np.ndarray,
                     offsets: np.ndarray, pc: int) -> "Batch":
    """One Rust call turns packed bytes into every tensor a `Batch` carries."""
    import kingdomino as kd

    d = kd.encode_packed_batch(records, policy, offsets)
    if d["pc"] != pc:
        raise ValueError(f"corpus is {d['pc']}p but the net is {pc}p")
    return Batch(
        torch.from_numpy(d["board"]),
        torch.from_numpy(d["lines"]),
        torch.from_numpy(d["glob"]),
        torch.from_numpy(d["a_type"]).long(),
        torch.from_numpy(d["a_pidx"]).long(),
        torch.from_numpy(d["a_ltok"]).long(),
        torch.from_numpy(d["a_mask"]).bool(),
        torch.from_numpy(d["policy"]),
        torch.from_numpy(d["value_rel"]),
        torch.from_numpy(d["score_rel"]) / SCORE_SCALE,
        torch.from_numpy(d["score_mask"]).bool(),
        pc,
    )


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
    score_rel: torch.Tensor  # [B, pc] float32, seat-relative final score / SCORE_SCALE (0 if absent)
    score_mask: torch.Tensor  # [B] bool, record carried final scores (old corpora: False)
    pc: int

    def to(self, device) -> "Batch":
        return Batch(
            self.board.to(device), self.lines.to(device), self.glob.to(device),
            self.a_type.to(device), self.a_pidx.to(device), self.a_ltok.to(device),
            self.a_mask.to(device), self.policy.to(device), self.value_rel.to(device),
            self.score_rel.to(device), self.score_mask.to(device), self.pc,
        )

    def __len__(self) -> int:
        return self.board.size(0)


def collate(records: list[dict], table=None, pc: int = 2) -> Batch:
    """Encode + pad a list of corpus records into a `Batch`. Records whose player count differs
    from `pc` are skipped (the net is built for one player count)."""
    encs, pols, vals, toacts, scores = [], [], [], [], []
    for r in records:
        enc = encode_obs(r["obs"], r["legal"], table)
        if enc.player_count != pc:
            continue
        encs.append(enc)
        pols.append(np.asarray(r["policy"], dtype=np.float32))
        vals.append(np.asarray(r["value"], dtype=np.float32))
        toacts.append(r["to_act"])
        scores.append(r.get("scores"))  # None for pre-score-target corpora
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
    score_rel = np.zeros((b, pc), dtype=np.float32)
    score_mask = np.zeros(b, dtype=bool)

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
        if scores[i] is not None:
            for k in range(pc):
                score_rel[i, k] = scores[i][(ta + k) % pc] / SCORE_SCALE
            score_mask[i] = True

    return Batch(
        torch.from_numpy(board), torch.from_numpy(lines), torch.from_numpy(glob),
        torch.from_numpy(a_type), torch.from_numpy(a_pidx), torch.from_numpy(a_ltok),
        torch.from_numpy(a_mask), torch.from_numpy(policy), torch.from_numpy(value_rel),
        torch.from_numpy(score_rel), torch.from_numpy(score_mask), pc,
    )
