"""AlphaZero-style MCTS, adapted for Kingdomino (CLAUDE §4):
  - **Stochasticity**: explicit chance nodes for the hidden draw. Unlike Space Base's 21-way
    2d6 (fully enumerated), a Kingdomino draw has up to 48 outcomes, so we **sample** an
    outcome from the engine's true distribution (`chance_outcomes`) on each visit and descend
    into (creating if new) that outcome's child — sparse sampling that approximates the
    expectation, no learned chance model.
  - **Multiplayer**: **max-n** backups with **vector** values (one per seat). At a decision
    node the to-act seat selects by its own component; the full value vector backs up.

`run(game)` (called at a player decision node) returns the root visit distribution over the
node's legal actions (the policy target), the root value vector, and the tree root.

Search values live on the **[-1,1]** scale (win 1 / loss -1, so FPU Q=0 is neutral for
PUCT); evaluators and the engine speak [0,1], rescaled at the leaf. The root value returned
by `run` is therefore also [-1,1] (2p: the two seats sum to ~0).
"""
from __future__ import annotations

import json
import math
import random

import numpy as np


def _settle_forced(game) -> None:
    """Apply forced single-action player plies until the next real decision, chance, or
    terminal, so a no-choice node never becomes a tree node (deeper search per sim). Common in
    Kingdomino: a forced discard, or a placement/claim with exactly one legal option."""
    for _ in range(128):  # defensive bound; forced chains are short
        if game.is_terminal() or game.is_chance() or game.num_actions() != 1:
            return
        game.apply(0)


class Node:
    __slots__ = (
        "game", "terminal", "chance", "pc", "to_act", "expanded",
        "priors", "N", "W", "children", "value", "outcomes",
    )

    def __init__(self, game):
        self.game = game
        self.terminal = game.is_terminal()
        self.chance = (not self.terminal) and game.is_chance()
        self.pc = game.player_count()
        self.expanded = False
        self.children: dict = {}
        self.priors = self.N = self.W = self.value = self.outcomes = None
        self.to_act = None if (self.terminal or self.chance) else game.to_act()
        if self.terminal:
            # Rescale the engine's [0,1] outcome to the [-1,1] search scale (win 1 / loss -1)
            # so PUCT's FPU Q=0 reads as neutral, not "certain loss".
            self.value = 2.0 * np.asarray(game.terminal_value(), dtype=np.float32) - 1.0
        elif self.chance:
            # [(action_index, prob)] over the remaining-deck (or starting-order) outcomes.
            self.outcomes = [(o["index"], o["prob"]) for o in json.loads(game.chance_outcomes())]


class MCTS:
    def __init__(self, evaluator, n_sims: int = 128, c_puct: float = 1.5, seed: int = 0,
                 dirichlet_alpha: float = 0.3, noise_eps: float = 0.25):
        self.ev = evaluator
        self.n_sims = n_sims
        self.c_puct = c_puct
        self.rng = random.Random(seed)
        self.np_rng = np.random.default_rng(seed)
        self.alpha = dirichlet_alpha
        self.noise_eps = noise_eps

    def run(self, game, add_noise: bool = False):
        root = Node(game.clone())
        assert not root.terminal and not root.chance, "MCTS runs at a player decision node"
        self._expand(root)
        if add_noise and len(root.priors) > 1:
            noise = self.np_rng.dirichlet([self.alpha] * len(root.priors)).astype(np.float32)
            root.priors = (1 - self.noise_eps) * root.priors + self.noise_eps * noise
        for _ in range(self.n_sims):
            self._simulate(root)
        visits = root.N.astype(np.float64)
        policy = visits / visits.sum() if visits.sum() > 0 else np.full(len(visits), 1 / len(visits))
        root_value = root.W.sum(0) / max(int(root.N.sum()), 1)
        return policy.astype(np.float32), root_value.astype(np.float32), root

    def _expand(self, node: Node):
        priors, value = self.ev.evaluate(node.game)
        node.priors = np.asarray(priors, dtype=np.float32)
        node.N = np.zeros(len(node.priors), dtype=np.int64)
        node.W = np.zeros((len(node.priors), node.pc), dtype=np.float32)
        # Evaluators return [0,1] per-seat values (summing to ~1); rescale to [-1,1] here.
        node.value = 2.0 * np.asarray(value, dtype=np.float32) - 1.0
        node.expanded = True

    def _simulate(self, node: Node) -> np.ndarray:
        if node.terminal:
            return node.value
        if node.chance:
            idx = self._sample_outcome(node.outcomes)
            child = node.children.get(idx)
            if child is None:
                g = node.game.clone()
                g.apply_chance_index(idx)
                _settle_forced(g)
                child = node.children[idx] = Node(g)
            return self._simulate(child)
        if not node.expanded:
            self._expand(node)
            return node.value
        a = self._select(node)
        child = node.children.get(a)
        if child is None:
            g = node.game.clone()
            g.apply(a)
            _settle_forced(g)
            child = node.children[a] = Node(g)
        v = self._simulate(child)
        node.N[a] += 1
        node.W[a] += v
        return v

    def _select(self, node: Node) -> int:
        total = int(node.N.sum())
        if total == 0:
            return int(np.argmax(node.priors))
        q = node.W[:, node.to_act] / np.maximum(node.N, 1)  # to-act seat's mean value
        u = self.c_puct * node.priors * math.sqrt(total) / (1 + node.N)
        return int(np.argmax(q + u))

    def _sample_outcome(self, outcomes) -> int:
        r, acc = self.rng.random(), 0.0
        for idx, p in outcomes:
            acc += p
            if r <= acc:
                return idx
        return outcomes[-1][0]
