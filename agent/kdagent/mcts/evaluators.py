"""Leaf evaluators for MCTS. Each returns `(priors, value)`:
  - priors: probabilities over the node's legal actions (aligned to `legal_actions` order),
  - value: an **absolute** per-seat value vector (length = player count), summing to ~1.
"""
from __future__ import annotations

import json
import random

import numpy as np


class RolloutEvaluator:
    """Uniform priors + a random playout to terminal (classic UCT value). No network — used
    to validate the search logic independently of training. Chance nodes are sampled from the
    engine's true distribution via the evaluator's own RNG (reproducible from `seed`)."""

    def __init__(self, seed: int = 0):
        self.rng = random.Random(seed)

    def evaluate(self, game):
        a = game.num_actions()
        priors = np.full(a, 1.0 / a, dtype=np.float32)
        g = game.clone()
        steps = 0
        while not g.is_terminal():
            steps += 1
            assert steps < 100_000, "rollout failed to terminate"
            if g.is_chance():
                outs = json.loads(g.chance_outcomes())
                r, acc = self.rng.random(), 0.0
                for o in outs:
                    acc += o["prob"]
                    if r <= acc:
                        g.apply_chance_index(o["index"])
                        break
                else:
                    g.apply_chance_index(outs[-1]["index"])
            else:
                g.apply(self.rng.randrange(g.num_actions()))
        return priors, np.asarray(g.terminal_value(), dtype=np.float32)


class NetEvaluator:
    """Network priors + value. Softmax of the policy logits is the prior; softmax of the
    seat-relative value head is mapped to an absolute per-seat vector."""

    def __init__(self, net, table=None, device: str = "cpu"):
        from ..encoder import load_domino_table

        self.net = net.to(device).eval()
        self.table = load_domino_table(table)
        self.device = device

    def evaluate(self, game):
        import torch

        from ..encoder import encode

        es = encode(game, self.table)
        with torch.no_grad():
            logits, value = self.net.policy_value(es, self.device)
        priors = torch.softmax(logits, dim=-1).cpu().numpy().astype(np.float32)
        pc, to_act = es.player_count, game.to_act()
        rel = torch.softmax(value, dim=-1).cpu().numpy().astype(np.float32)  # seat-relative
        absval = np.zeros(pc, dtype=np.float32)
        for k in range(pc):
            absval[(to_act + k) % pc] = rel[k]
        return priors, absval
