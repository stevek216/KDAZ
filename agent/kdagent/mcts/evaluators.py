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
    seat-relative value head is mapped to an absolute per-seat vector.

    `value_blend` mixes the score head into the leaf value, exactly as the batched Rust path
    does (`kdagent.selfplay._gpu_forward`): (1-a)*P(win) + a*sigmoid(cal*margin). On gen11 at
    512 sims, a=0.75 scores 52.3% +/- 1.8 over 3000 games against a=0 — free strength, since
    the score head is already trained and search otherwise ignores it.

    Keeping the two paths in agreement matters: this one backs the web UI and the BGA advisor,
    and it is the one an A/B is least likely to cover (the batched path is what the arena and
    the training loop exercise). They are verified to agree to 1e-5 on the same position.
    """

    def __init__(self, net, table=None, device: str = "cpu",
                 value_blend: float = 0.0, score_cal: float = 6.139):
        from ..encoder import load_domino_table

        self.net = net.to(device).eval()
        self.table = load_domino_table(table)
        self.device = device
        self.value_blend = float(value_blend)
        self.score_cal = float(score_cal)

    def evaluate(self, game):
        import torch

        from ..encoder import encode

        es = encode(game, self.table)
        blend = self.value_blend
        with torch.no_grad():
            out = self.net.policy_value(es, self.device, with_score=blend > 0.0)
        logits, value = out[0], out[1]
        priors = torch.softmax(logits, dim=-1).cpu().numpy().astype(np.float32)
        pc, to_act = es.player_count, game.to_act()
        rel = torch.softmax(value, dim=-1).cpu().numpy().astype(np.float32)  # seat-relative
        if blend > 0.0 and pc == 2:
            score = out[2].float()
            p_score = float(torch.sigmoid(self.score_cal * (score[0] - score[1])))
            p = (1.0 - blend) * float(rel[0]) + blend * p_score
            rel = np.array([p, 1.0 - p], dtype=np.float32)
        absval = np.zeros(pc, dtype=np.float32)
        for k in range(pc):
            absval[(to_act + k) % pc] = rel[k]
        return priors, absval
