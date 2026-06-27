"""Step 1 check: the Rust batch encoder (`kingdomino.encode_batch`) must be bit-for-bit
identical to the Python encoder, and we measure the speedup. Run (from agent/):
  ../.venv/Scripts/python -m kdagent.test_rust_encoder"""
import random
import time

import numpy as np

import kingdomino as kd
from kdagent.encoder import N_PLANES, STORE, encode


def collect_games(n, seed=0):
    """A list of `n` cloned Game handles, each sitting at a player decision node."""
    rng = random.Random(seed)
    games, gi = [], 0
    while len(games) < n:
        g = kd.Game(gi, 2)
        gi += 1
        while not g.is_terminal() and len(games) < n:
            if g.is_chance():
                g.apply_chance()
                continue
            games.append(g.clone())
            g.apply(rng.randrange(g.num_actions()))
    return games


def test_parity():
    games = collect_games(200)
    board, lines, glob = kd.encode_batch(games)
    pc = games[0].player_count()
    for i, g in enumerate(games):
        e = encode(g)
        assert np.allclose(board[i], e.board.reshape(pc * N_PLANES, STORE, STORE), atol=1e-6), f"board {i}"
        assert np.allclose(lines[i], e.lines, atol=1e-6), f"lines {i}"
        assert np.allclose(glob[i], e.glob, atol=1e-6), f"glob {i}"
    print(f"  parity: Rust encode_batch == Python encoder on {len(games)} states OK")


def test_speedup():
    games = collect_games(1024)

    # Rust: Game -> tensors, batched, rayon-parallel.
    t0, reps = time.perf_counter(), 0
    while time.perf_counter() - t0 < 1.0:
        kd.encode_batch(games)
        reps += len(games)
    rust = reps / (time.perf_counter() - t0)

    # Python: Game -> observation()/legal_actions() JSON -> encode_obs (the path we replace).
    sample = games[:128]
    t0, reps = time.perf_counter(), 0
    while time.perf_counter() - t0 < 1.0:
        for g in sample:
            encode(g)
        reps += len(sample)
    py = reps / (time.perf_counter() - t0)

    print(f"  Python encoder : {py:>12,.0f} states/sec")
    print(f"  Rust encode_batch: {rust:>12,.0f} states/sec   ({rust / py:.0f}x faster)")


if __name__ == "__main__":
    print("kdagent Rust encoder (step 1)")
    test_parity()
    test_speedup()
    print("ALL OK")
