"""Smoke test for the `kingdomino` bridge: drive full games via the control API and
sanity-check observation / domino_table / chance / clone.
Run: `../.venv/Scripts/python smoke_test.py` (after `maturin develop`)."""
import json
import random

import kingdomino as kd


def test_domino_table():
    table = json.loads(kd.domino_table())
    assert len(table) == 48, f"expected 48 dominoes, got {len(table)}"
    assert [d["number"] for d in table] == list(range(1, 49)), "numbers must be 1..48"
    for d in table:
        for half in ("a", "b"):
            assert 0 <= d[half]["terrain"] <= 5
            assert 0 <= d[half]["crowns"] <= 3
    # Domino 48 is wheat | mine(3 crowns) — the famous high-value tile (terrain 5 == mine).
    assert table[47]["b"] == {"terrain": 5, "crowns": 3}, table[47]
    print(f"  domino_table: 48 dominoes OK")


def test_initial_chance():
    g = kd.Game(7, 2)
    assert g.is_chance(), "a fresh game opens at the first Draw (chance) node"
    oc = json.loads(g.chance_outcomes())
    assert len(oc) == 48, f"first draw is over all 48 dominoes, got {len(oc)}"
    total = sum(o["prob"] for o in oc)
    assert abs(total - 1.0) < 1e-5, f"probs must sum to 1, got {total}"
    print(f"  chance: {len(oc)} outcomes, sum={total:.6f} OK")


def play_random_game(seed, players, rng):
    g = kd.Game(seed, players)
    steps = 0
    while not g.is_terminal():
        steps += 1
        assert steps < 100_000, "game failed to terminate"
        if g.is_chance():
            g.apply_chance()  # sample from the true distribution (what self-play does)
        else:
            acts = json.loads(g.legal_actions())
            assert acts, "a player node must offer at least one action"
            obs = json.loads(g.observation())  # observation parses & is well-formed
            assert len(obs["seats"]) == players
            assert obs["deck_remaining"] == len(obs["remaining"])
            g.apply(rng.choice(acts)["index"])
    val = g.terminal_value()
    assert val is not None and len(val) == players, f"bad value {val}"
    assert abs(sum(val) - 1.0) < 1e-5, f"value vector sums to 1: {val}"
    assert all(0.0 <= v <= 1.0 for v in val), val
    return steps, val


def test_full_games():
    rng = random.Random(123)
    for seed in range(5):
        for players in (2, 4):
            play_random_game(seed, players, rng)
    print("  full games: 5 seeds x {2,4} players all terminated with a value OK")


def test_clone_independence():
    g = kd.Game(1, 2)
    while g.is_chance():
        g.apply_chance()  # advance to the first player decision (a starting claim)
    before = g.observation()
    h = g.clone()
    acts = json.loads(h.legal_actions())
    h.apply(acts[0]["index"])  # mutate the clone
    assert g.observation() == before, "clone must not alias the original"
    print("  clone: independent state OK")


if __name__ == "__main__":
    print("kingdomino bridge smoke test")
    test_domino_table()
    test_initial_chance()
    test_full_games()
    test_clone_independence()
    print("ALL OK")
