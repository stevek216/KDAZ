"""MCTS tests: valid policy/value at a node, full games to terminal under both evaluators,
and seed determinism. Run: `../.venv/Scripts/python -m kdagent.mcts.test_mcts` from agent/."""
import numpy as np

import kingdomino as kd
from kdagent.mcts import MCTS, NetEvaluator, RolloutEvaluator
from kdagent.net import KingdominoNet


def advance_to_player(g):
    while g.is_chance() and not g.is_terminal():
        g.apply_chance()
    return g


def _check_outputs(policy, value, g):
    a = g.num_actions()
    assert policy.shape == (a,) and np.all(np.isfinite(policy))
    assert abs(float(policy.sum()) - 1.0) < 1e-5
    assert value.shape == (g.player_count(),)
    # Search values are [-1,1] (rescaled from the [0,1] outcome scale), so the per-seat
    # values sum to 2·1 - pc (= 0 for 2p) instead of 1.
    assert abs(float(value.sum()) - (2.0 - g.player_count())) < 1e-4, value
    assert np.all(value >= -1.0 - 1e-4) and np.all(value <= 1.0 + 1e-4), value


def test_rollout_search_outputs():
    g = advance_to_player(kd.Game(7, 2))
    mcts = MCTS(RolloutEvaluator(0), n_sims=48, seed=1)
    policy, value, root = mcts.run(g, add_noise=True)
    _check_outputs(policy, value, g)
    assert int(root.N.sum()) == 48
    print("  rollout search: valid policy/value, visits accounted OK")


def play_full_game(evaluator, n_sims, seed):
    g = kd.Game(seed, 2)
    mcts = MCTS(evaluator, n_sims=n_sims, seed=seed)
    steps = 0
    while not g.is_terminal():
        steps += 1
        assert steps < 100_000
        if g.is_chance():
            g.apply_chance()
            continue
        policy, _, _ = mcts.run(g)
        g.apply(int(np.argmax(policy)))
    v = g.terminal_value()
    assert v is not None and abs(sum(v) - 1.0) < 1e-5
    return v


def test_full_game_rollout():
    v = play_full_game(RolloutEvaluator(0), n_sims=6, seed=0)
    print(f"  full game (rollout MCTS) terminated, value={['%.2f' % x for x in v]} OK")


def test_full_game_net():
    net = KingdominoNet(player_count=2)
    v = play_full_game(NetEvaluator(net), n_sims=12, seed=1)
    print(f"  full game (net MCTS) terminated, value={['%.2f' % x for x in v]} OK")


def test_determinism():
    g = advance_to_player(kd.Game(5, 2))
    p1, v1, _ = MCTS(RolloutEvaluator(0), n_sims=24, seed=1).run(g)
    p2, v2, _ = MCTS(RolloutEvaluator(0), n_sims=24, seed=1).run(g)
    assert np.array_equal(p1, p2) and np.array_equal(v1, v2)
    print("  determinism: identical seeds reproduce the search OK")


if __name__ == "__main__":
    print("kdagent MCTS tests")
    test_rollout_search_outputs()
    test_determinism()
    test_full_game_net()
    test_full_game_rollout()
    print("ALL OK")
