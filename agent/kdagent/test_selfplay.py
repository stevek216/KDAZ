"""Batched self-play pool semantics tests (regressions for the search-audit fixes):
sim accounting (n_sims=1 must yield a real one-hot policy, not uniform), the
dirichlet_alpha=0 no-panic guard, the n_sims=0 constructor rejection, and that the packed
and JSONL output forms carry identical records.
Run: `../.venv/Scripts/python -m kdagent.test_selfplay` from agent/."""
import json

import numpy as np
import torch

import kingdomino as kd
from kdagent.net import KingdominoNet
from kdagent.selfplay import _gather_logits


def _pump(pool, net, harvest):
    """Minimal CPU driver: run the pool to completion, calling `harvest` each round."""
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
        harvest()
    harvest()


def _drive(pool, net):
    """Run a JSONL-mode pool to completion and return its corpus records as dicts."""
    lines_out = []
    _pump(pool, net, lambda: lines_out.extend(pool.drain()))
    return [json.loads(ln) for ln in lines_out]


def _drive_packed(pool, net):
    """Run a packed-mode pool and return `(records ndarray, policy ndarray)`."""
    recs, pols = [], []

    def harvest():
        r, p = pool.drain_packed()
        if r.shape[0]:
            recs.append(r)
            pols.append(p)

    _pump(pool, net, harvest)
    return (np.concatenate(recs) if recs else np.zeros((0, 0), dtype=np.uint8),
            np.concatenate(pols) if pols else np.zeros(0, dtype=np.float32))


def test_sims1_policy_is_onehot():
    """With n_sims=1 the single true descent must produce a one-hot visit policy.
    (Before the sim-accounting fix, the root expansion consumed the budget and every
    recorded policy was uniform — zero search signal.)"""
    net = KingdominoNet(player_count=2)
    pool = kd.BatchedNetSelfPlay(n_games=2, total_games=2, players=2, n_sims=1,
                                 temp_moves=0, seed=3, packed=False)
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
                                 dirichlet_alpha=0.0, noise_eps=0.25, seed=5, packed=False)
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


def test_packed_matches_jsonl():
    """The packed corpus must carry exactly what the JSONL form carried: same states, same
    legal actions (re-derived, not stored), same policy/value/score targets."""
    def run(packed):
        net = KingdominoNet(player_count=2)
        torch.manual_seed(0)
        pool = kd.BatchedNetSelfPlay(n_games=2, total_games=2, players=2, n_sims=8,
                                     temp_moves=0, noise_eps=0.0, seed=17, packed=packed)
        return (_drive_packed if packed else _drive)(pool, net)

    torch.manual_seed(0)
    jl = run(False)
    torch.manual_seed(0)
    recs, pols = run(True)
    assert len(jl) == recs.shape[0], f"{len(jl)} JSONL records vs {recs.shape[0]} packed"

    off = 0
    for i, want in enumerate(jl):
        got = json.loads(kd.packed_record_json(np.ascontiguousarray(recs[i])))
        assert got["to_act"] == want["to_act"], f"record {i}: to_act"
        assert got["obs"] == want["obs"], f"record {i}: observation differs"
        assert got["legal"] == want["legal"], f"record {i}: legal actions differ"
        assert got["value"] == want["value"], f"record {i}: value target"
        assert got["scores"] == want["scores"], f"record {i}: score target"
        n = len(want["policy"])
        assert np.allclose(pols[off:off + n], want["policy"], atol=0), f"record {i}: policy"
        off += n
    assert off == pols.size, "policy blob has trailing data"
    print(f"  packed == jsonl across {len(jl)} records (obs, legal, policy, value, scores) OK")


def test_packed_games_are_distinct():
    """Two pools covering disjoint game-index ranges must not replay each other's games.
    (Deriving a second base seed instead collided with the game-seed stride and made ~48% of
    every --overlap corpus exact duplicates.)"""
    net = KingdominoNet(player_count=2)
    ids = set()
    for first_game, total in [(0, 3), (3, 3)]:
        pool = kd.BatchedNetSelfPlay(n_games=3, total_games=total, players=2, n_sims=2,
                                     temp_moves=0, seed=99, first_game=first_game)
        recs, _ = _drive_packed(pool, net)
        for i in range(recs.shape[0]):
            ids.add(json.loads(kd.packed_record_json(np.ascontiguousarray(recs[i])))["game"])
    assert len(ids) == 6, f"expected 6 distinct games across the two ranges, got {len(ids)}"
    print(f"  disjoint game ranges -> {len(ids)} distinct game ids OK")


if __name__ == "__main__":
    print("kdagent batched self-play pool tests")
    test_zero_sims_rejected()
    test_alpha_zero_noise_does_not_panic()
    test_sims1_policy_is_onehot()
    test_packed_matches_jsonl()
    test_packed_games_are_distinct()
    print("ALL OK")
