"""Arena tests: seat scores sum to 1, MCTS clearly beats random (the core search sanity
check), and the net/netmcts agents run. Run: `../.venv/Scripts/python -m kdagent.test_arena`."""
import tempfile

import torch

from kdagent.arena import RandomAgent, RolloutMctsAgent, play_game, run_match
from kdagent.net import KingdominoNet


def test_scores_sum_to_one():
    agents = [RandomAgent(0), RandomAgent(1)]
    for seed in range(5):
        val = play_game(agents, seed, 2)
        assert abs(sum(val) - 1.0) < 1e-5, val
    print("  per-game seat scores sum to 1 OK")


def test_mcts_beats_random():
    # The central sanity check: rollout-MCTS should clearly outscore a random field.
    mean, ci, n, _ = run_match("mcts:64", "random", games=30, seed=1)
    print(f"  mcts:64 vs random -> {mean * 100:.1f}% +/- {ci * 100:.1f} over {n} games")
    assert mean > 0.6, f"MCTS should beat random (got {mean:.2f})"


def _save_tiny_net():
    cfg = {"player_count": 2, "ch": 16, "board_blocks": 2}
    net = KingdominoNet(**cfg)
    path = tempfile.NamedTemporaryFile(suffix=".pt", delete=False).name
    torch.save({"model": net.state_dict(), "net_cfg": cfg}, path)
    return path


def test_net_agents_run():
    path = _save_tiny_net()
    # An untrained net — we only check the agents play valid games and return sane scores.
    m1, _, n1, _ = run_match(f"net:{path}", "random", games=4, seed=2)
    assert 0.0 <= m1 <= 1.0 and n1 == 4
    m2, _, n2, _ = run_match(f"netmcts:16:{path}", "random", games=4, seed=3)
    assert 0.0 <= m2 <= 1.0 and n2 == 4
    print(f"  net agents run OK (net {m1 * 100:.0f}%, netmcts {m2 * 100:.0f}% vs random)")


if __name__ == "__main__":
    print("kdagent arena tests")
    test_scores_sum_to_one()
    test_mcts_beats_random()
    test_net_agents_run()
    print("ALL OK")
