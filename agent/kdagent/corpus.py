"""Packed binary corpus: writer + memory-mapped reader.

The byte layout is defined once, in Rust, in `kd-features/src/pack.rs` — read that module's
doc comment for the field map. This is the Python end: `PackedCorpusWriter` appends what
`BatchedNetSelfPlay.drain_packed()` hands back, and `PackedCorpus` maps a finished file for
training without materializing it.

Why this exists: the previous JSONL-of-dicts corpus cost ~18.4 KiB per record *resident*, so a
single 10k-game corpus needed ~14 GiB of RAM and the training loop could not hold the two
corpora it wants. Packed records are 434 B and are memory-mapped, so resident memory tracks the
batch size rather than the corpus size, and generations can scale past 10k games.

A record stores the `GameState` and the targets, never the observation or the legal-action
list: both are pure functions of the state, so the loader re-derives them through the same
engine call the generator used. That keeps corpora valid across feature-schema changes.
"""
from __future__ import annotations

import os

import numpy as np

MAGIC = b"KDC1"
FORMAT_VERSION = 1
HEADER_BYTES = 32

# Field offsets inside a record that the loader reads directly (see pack.rs).
O_N_ACTIONS = 8
O_GAME = 74


def record_size(player_count: int) -> int:
    """Bytes per record — must agree with `pack::record_size`."""
    return 82 + 176 * player_count


class PackedCorpusWriter:
    """Streams records to `path`, spilling the ragged policy blob to a sidecar until close.

    The header carries counts that are only known at the end, so it is written twice: a
    placeholder up front, then the real one via a seek on close.
    """

    def __init__(self, path: str, player_count: int):
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        self.path = path
        self.player_count = player_count
        self.rec_size = record_size(player_count)
        self.n_records = 0
        self.n_policy = 0
        self._pol_path = path + ".pol.tmp"
        self._f = open(path, "wb")
        self._f.write(b"\0" * HEADER_BYTES)
        self._pol = open(self._pol_path, "wb")

    def add(self, records: np.ndarray, policy: np.ndarray) -> int:
        """Append a drained batch. Returns the number of records added."""
        if records.size == 0:
            return 0
        if records.ndim != 2 or records.shape[1] != self.rec_size:
            raise ValueError(
                f"expected records of width {self.rec_size} for {self.player_count}p, "
                f"got shape {records.shape}"
            )
        self._f.write(np.ascontiguousarray(records, dtype=np.uint8).tobytes())
        self._pol.write(np.ascontiguousarray(policy, dtype=np.float32).tobytes())
        self.n_records += records.shape[0]
        self.n_policy += policy.size
        return records.shape[0]

    def close(self) -> None:
        self._pol.close()
        policy_offset = HEADER_BYTES + self.n_records * self.rec_size
        with open(self._pol_path, "rb") as pol:
            while chunk := pol.read(1 << 22):
                self._f.write(chunk)
        header = bytearray(HEADER_BYTES)
        header[0:4] = MAGIC
        header[4:8] = FORMAT_VERSION.to_bytes(4, "little")
        header[8:12] = self.player_count.to_bytes(4, "little")
        header[12:16] = self.rec_size.to_bytes(4, "little")
        header[16:24] = self.n_records.to_bytes(8, "little")
        header[24:32] = policy_offset.to_bytes(8, "little")
        self._f.seek(0)
        self._f.write(bytes(header))
        self._f.close()
        os.remove(self._pol_path)

    def __enter__(self) -> "PackedCorpusWriter":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


class PackedCorpus:
    """A memory-mapped packed corpus.

    `records[i]` is record `i`'s raw bytes (fed to the Rust batch encoder); `policy_for(i)`
    is its visit-distribution target, aligned with `legal_actions` of the decoded state.
    Nothing is read from disk until indexed, so opening a 4M-record corpus is instant and
    resident memory stays proportional to what the trainer actually touches.
    """

    def __init__(self, path: str):
        with open(path, "rb") as f:
            head = f.read(HEADER_BYTES)
        if len(head) < HEADER_BYTES or head[0:4] != MAGIC:
            raise ValueError(f"{path}: not a packed Kingdomino corpus (bad magic)")
        version = int.from_bytes(head[4:8], "little")
        if version != FORMAT_VERSION:
            raise ValueError(f"{path}: corpus format version {version}, this build reads {FORMAT_VERSION}")
        self.path = path
        self.player_count = int.from_bytes(head[8:12], "little")
        self.rec_size = int.from_bytes(head[12:16], "little")
        self.n_records = int.from_bytes(head[16:24], "little")
        self._policy_offset = int.from_bytes(head[24:32], "little")
        if self.rec_size != record_size(self.player_count):
            raise ValueError(
                f"{path}: record_size {self.rec_size} disagrees with player_count {self.player_count}"
            )

        self.records = np.memmap(
            path, dtype=np.uint8, mode="r", offset=HEADER_BYTES,
            shape=(self.n_records, self.rec_size),
        )
        # Per-record action counts -> policy offsets. One vectorized pass over a u16 column;
        # storing the offsets in the file would only duplicate what this recovers.
        self.n_actions = (
            np.ascontiguousarray(self.records[:, O_N_ACTIONS:O_N_ACTIONS + 2])
            .view(np.uint16).ravel().astype(np.int64)
        )
        self.offsets = np.zeros(self.n_records + 1, dtype=np.int64)
        np.cumsum(self.n_actions, out=self.offsets[1:])
        total = int(self.offsets[-1])
        self.policy = np.memmap(
            path, dtype=np.float32, mode="r", offset=self._policy_offset, shape=(total,)
        )

    @property
    def game_ids(self) -> np.ndarray:
        """Per-record generating-game id, for holding out whole games rather than positions."""
        return (
            np.ascontiguousarray(self.records[:, O_GAME:O_GAME + 8])
            .view(np.uint64).ravel()
        )

    def policy_for(self, i: int) -> np.ndarray:
        return self.policy[self.offsets[i]:self.offsets[i + 1]]

    def policies_for(self, idx: np.ndarray) -> list[np.ndarray]:
        return [self.policy[self.offsets[i]:self.offsets[i + 1]] for i in idx]

    def __len__(self) -> int:
        return self.n_records

    def __repr__(self) -> str:
        mb = (os.path.getsize(self.path)) / 2**20
        return (f"PackedCorpus({self.path!r}, {self.n_records:,} records, "
                f"{self.player_count}p, {mb:,.0f} MiB)")


def is_packed(path: str) -> bool:
    """Cheap sniff so callers can accept either corpus form."""
    try:
        with open(path, "rb") as f:
            return f.read(4) == MAGIC
    except OSError:
        return False
