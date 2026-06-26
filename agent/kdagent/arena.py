"""Arena: measure relative strength by seating a lineup of agents (one per seat) and scoring
each over many games. The sanity checks the training loop needs — MCTS must beat random, and
a newly-trained net must beat its predecessor.

Fairness: each "round" plays `players` games on one shared game seed, cyclically rotating the
lineup through every seat, so every agent occupies every seat once on the same shuffled deck.
Scoring uses the engine's `terminal_value` (win 1.0 / loss 0.0 / shared 0.5), so per-game seat
scores sum to 1 and the even-strength baseline is **1/players** (0.5 for 2p).

    cd agent
    .venv/Scripts/python -m kdagent.arena --a mcts:128 --b random --games 200
    .venv/Scripts/python -m kdagent.arena --a netmcts:64:runs/net.best.pt --b mcts:64 --games 200 --device cuda
"""
from __future__ import annotations

import argparse
import json
import math
import os
import random
import time

import numpy as np

import kingdomino as kd

MOVE_CAP = 2000  # defensive only — Kingdomino always terminates (the deck depletes)


def _argmax(xs) -> int:
    best, bv = 0, float("-inf")
    for i, x in enumerate(xs):
        if x > bv:
            bv, best = x, i
    return best


# --------------------------------------------------------------------------- agents
class RandomAgent:
    def __init__(self, seed=0):
        self.name = "random"
        self.seed = seed

    def act(self, g, move_seed):
        return random.Random(move_seed ^ self.seed).randrange(g.num_actions())


class RolloutMctsAgent:
    """Greedy rollout-MCTS (most-visited action after `n_sims` sims), run in Rust."""

    def __init__(self, n_sims, c_puct=1.5, seed=0):
        self.name = f"mcts{n_sims}"
        self.n_sims, self.c_puct, self.seed = n_sims, c_puct, seed

    def act(self, g, move_seed):
        return _argmax(g.mcts_policy(self.n_sims, self.c_puct, (move_seed ^ self.seed) & 0xFFFFFFFF))


class NetAgent:
    """Plays the network's policy head greedily (no search) — the raw learned policy."""

    def __init__(self, path, seed=0, device="cpu"):
        self.name = f"net:{os.path.basename(path)}"
        self.path, self.device, self.seed = path, device, seed
        self._net = self._torch = None

    def _ensure(self):
        if self._net is None:
            import torch

            from .net import load_net
            self._net, _ = load_net(self.path, self.device)
            self._torch = torch

    def act(self, g, _move_seed):
        self._ensure()
        from .encoder import encode
        with self._torch.no_grad():
            logits, _ = self._net.policy_value(encode(g), self.device)
        return int(logits.argmax().item())


class NetMctsAgent:
    """AlphaZero-style play: the Python MCTS guided by the net, greedy on root visit counts."""

    def __init__(self, path, n_sims, c_puct=1.5, seed=0, device="cpu"):
        self.name = f"netmcts{n_sims}:{os.path.basename(path)}"
        self.path, self.n_sims, self.c_puct, self.seed, self.device = (
            path, n_sims, c_puct, seed, device)
        self._ev = None

    def _ensure(self):
        if self._ev is None:
            from .mcts.evaluators import NetEvaluator
            from .net import load_net
            net, _ = load_net(self.path, self.device)
            self._ev = NetEvaluator(net, device=self.device)

    def act(self, g, move_seed):
        self._ensure()
        from .mcts.search import MCTS
        mcts = MCTS(self._ev, n_sims=self.n_sims, c_puct=self.c_puct,
                    seed=(move_seed ^ self.seed) & 0xFFFFFFFF)
        policy, _, _ = mcts.run(g, add_noise=False)
        return int(policy.argmax())


def make_agent(spec, seed=0, device="cpu"):
    """Agent spec: `random` | `mcts:SIMS[:C_PUCT]` | `net:CKPT` | `netmcts:SIMS:CKPT`."""
    if spec == "random":
        return RandomAgent(seed)
    if spec.startswith("mcts:"):
        parts = spec.split(":")
        return RolloutMctsAgent(int(parts[1]), float(parts[2]) if len(parts) > 2 else 1.5, seed)
    if spec.startswith("netmcts:"):
        _, sims, path = spec.split(":", 2)
        return NetMctsAgent(path, int(sims), seed=seed, device=device)
    if spec.startswith("net:"):
        return NetAgent(spec[len("net:"):], seed=seed, device=device)
    raise ValueError(f"unknown agent spec: {spec!r}")


# --------------------------------------------------------------------------- play
def play_game(agents, game_seed, players, harmony=True, middle_kingdom=True):
    """Play one game; `agents[seat]` acts for that seat. Returns the per-seat terminal value."""
    g = kd.Game(game_seed, players, harmony, middle_kingdom)
    move = 0
    while not g.is_terminal():
        if move >= MOVE_CAP:
            return _resolve_by_total(g, players)
        if g.is_chance():
            g.apply_chance()
            continue
        if g.num_actions() == 1:
            g.apply(0)
            continue
        seat = g.to_act()
        move_seed = (game_seed * 1_000_003 + move) & 0xFFFFFFFF
        g.apply(agents[seat].act(g, move_seed))
        move += 1
    return list(g.terminal_value())


def _resolve_by_total(g, players):
    scores = json.loads(g.observation())["scores"]
    totals = [scores[k]["total"] for k in range(players)]
    mx = max(totals)
    winners = [k for k in range(players) if totals[k] == mx]
    return [1.0 / len(winners) if k in winners else 0.0 for k in range(players)]


def _round_lineup(agents, gs, players, variants):
    out = [[] for _ in range(players)]
    for s in range(players):
        seated = [agents[(j + s) % players] for j in range(players)]
        val = play_game(seated, gs, players, *variants)
        for j in range(players):
            out[(j + s) % players].append(val[j])
    return out


def _stats(score_lists, t0):
    means, cis = [], []
    for sc in score_lists:
        n = len(sc)
        mean = sum(sc) / n if n else 0.0
        var = sum((x - mean) ** 2 for x in sc) / max(1, n - 1)
        means.append(mean)
        cis.append(1.96 * math.sqrt(var / n) if n else 0.0)
    n = len(score_lists[0]) if score_lists else 0
    return means, cis, n, time.time() - t0


def run_lineup(specs, games, players=2, seed=0, device="cpu", variants=(True, True), tag=""):
    if len(specs) != players:
        raise ValueError(f"need one agent per seat: {len(specs)} specs for {players} players")
    agents = [make_agent(s, seed=i, device=device) for i, s in enumerate(specs)]
    scores = [[] for _ in range(players)]
    rounds = max(1, round(games / players))
    total, t0 = rounds * players, time.time()
    for rd in range(rounds):
        r = _round_lineup(agents, seed * 1_000_003 + rd, players, variants)
        for i in range(players):
            scores[i] += r[i]
        done = (rd + 1) * players
        el = time.time() - t0
        mstr = " ".join(f"{sum(s) / len(s) * 100:4.1f}" for s in scores)
        print(f"  {tag}{done}/{total} games | scores% {mstr} | {el / done:.2f} s/game",
              flush=True, end="\r")
    print(flush=True)
    return _stats(scores, t0)


def run_match(a_spec, b_spec, games, players=2, seed=0, device="cpu", variants=(True, True), tag=""):
    specs = [a_spec] + [b_spec] * (players - 1)
    means, cis, n, secs = run_lineup(specs, games, players, seed, device, variants, tag)
    return means[0], cis[0], n, secs


def main():
    ap = argparse.ArgumentParser(description="Agent arena for Kingdomino (Mighty Duel).")
    ap.add_argument("--a", default="mcts:64", help="hero: random | mcts:SIMS | net:CKPT | netmcts:SIMS:CKPT")
    ap.add_argument("--b", default="random", help="field agent (fills the other seats)")
    ap.add_argument("--games", type=int, default=100)
    ap.add_argument("--players", type=int, default=2)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--device", default="cpu", help="torch device for net agents (e.g. cuda)")
    ap.add_argument("--harmony", action=argparse.BooleanOptionalAction, default=True)
    ap.add_argument("--middle-kingdom", dest="middle_kingdom",
                    action=argparse.BooleanOptionalAction, default=True)
    args = ap.parse_args()

    a_name, b_name = make_agent(args.a).name, make_agent(args.b).name
    variants = (args.harmony, args.middle_kingdom)
    print(f"{a_name} vs {b_name} field ({args.games} games, {args.players}p, device {args.device})")
    mean, ci, n, dt = run_match(args.a, args.b, args.games, args.players, args.seed,
                                args.device, variants, tag=f"{a_name} vs {b_name} ")
    base = 1.0 / args.players
    print(f"\n{a_name}: {mean * 100:.1f}% +/- {ci * 100:.1f}  "
          f"({n} games, {dt / max(1, n):.2f} s/game, even = {base * 100:.0f}%)")
    verdict = ("hero stronger" if mean - ci > base else
               "hero weaker" if mean + ci < base else "inconclusive")
    print(f"verdict: {verdict}")


if __name__ == "__main__":
    main()
