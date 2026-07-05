"""Batched self-play pool semantics tests (regressions for the search-audit fixes):
sim accounting (n_sims=1 must yield a real one-hot policy, not uniform), the
dirichlet_alpha=0 no-panic guard, and the n_sims=0 constructor rejection.
Run: `../.venv/Scripts/python -m kdagent.test_selfplay` from agent/."""
import json

import numpy as np
import torch

import kingdomino as kd
from kdagent.net import KingdominoNet
from kdagent.selfplay import _gather_logits


def _drive(pool, net):
    """Minimal CPU driver: run the pool to completion, return all corpus records."""
    lines_out = []
    rounds = 0
    while not pool.done():
        rounds += 1
        assert rounds < 100_000, "pool failed to terminate"
        batch = pool.collect()
        if batch["b"] > 0:
            board = torch.from_numpy(batch["board"]).float()
            lines = torch.from_numpy(batch["lines"])
            glob = torch.from_numpy(batch["glob"])
            with torch.no_grad():
                place_map, claim_logits, discard, value = net.forward_batch(board, lines, glob)
                logits = _gather_logits(place_map, claim_logits, discard, batch, "cpu")
                value_rel = torch.softmax(value.float(), dim=1)
            pool.apply(logits.float().numpy(), value_rel.numpy())
        lines_out += pool.drain()
    lines_out += pool.drain()
    return [json.loads(ln) for ln in lines_out]


def test_sims1_policy_is_onehot():
    """With n_sims=1 the single true descent must produce a one-hot visit policy.
    (Before the sim-accounting fix, the root expansion consumed the budget and every
    recorded policy was uniform — zero search signal.)"""
    net = KingdominoNet(player_count=2)
    pool = kd.BatchedNetSelfPlay(n_games=2, total_games=2, players=2, n_sims=1,
                                 temp_moves=0, seed=3)
    records = _drive(pool, net)
    assert records, "no records produced"
    for r in records:
        p = np.asarray(r["policy"], dtype=np.float64)
        assert len(p) == len(r["legal"]) and len(p) >= 2  # forced plies are never recorded
        assert abs(p.sum() - 1.0) < 1e-5
        assert p.max() == 1.0, f"n_sims=1 policy not one-hot: {p}"
        assert 0.0 <= min(r["value"]) and max(r["value"]) <= 1.0  # engine outcome scale
    print(f"  n_sims=1: {len(records)} records, all policies one-hot OK")


def test_alpha_zero_noise_does_not_panic():
    """dirichlet_alpha=0 with noise_eps>0 must silently disable noise (used to panic in
    Gamma::new on a rayon worker)."""
    net = KingdominoNet(player_count=2)
    pool = kd.BatchedNetSelfPlay(n_games=1, total_games=1, players=2, n_sims=4,
                                 dirichlet_alpha=0.0, noise_eps=0.25, seed=5)
    records = _drive(pool, net)
    assert records
    print(f"  dirichlet_alpha=0: game completed without panic ({len(records)} records) OK")


def test_zero_sims_rejected():
    """n_sims=0 self-play would record uniform policy targets — the constructor refuses."""
    try:
        kd.BatchedNetSelfPlay(n_games=1, total_games=1, players=2, n_sims=0, seed=0)
    except ValueError as e:
        print(f"  n_sims=0 rejected: {e} OK")
        return
    raise AssertionError("BatchedNetSelfPlay accepted n_sims=0")


if __name__ == "__main__":
    print("kdagent batched self-play pool tests")
    test_zero_sims_rejected()
    test_alpha_zero_noise_does_not_panic()
    test_sims1_policy_is_onehot()
    print("ALL OK")
