# Kingdomino agent (search + learning) — to come

The AlphaZero-style player for Kingdomino: the neural network, MCTS search (with explicit
chance nodes for the hidden draw), self-play loop, and trainer. The Rust engine in
`../board-game-engine/` is the single source of rules truth — this layer only *consumes*
it and never reimplements a rule.

## Status

Not started. The engine comes first (`../board-game-engine/docs/engine-design.md` §9).
This layer will **mirror the Space Base agent** (`../../SpaceBase/agent/`): a PyO3
`engine-bridge`, a token + attention encoder driven by an engine-grounded
`docs/feature-schema.md`, a pointer-style policy + max-n vector value head, a self-play
corpus (append-only JSONL of `{obs, legal, policy, value}`), a trainer, and an arena.

Key Kingdomino-specific adaptations to design here (not in the engine):
- **Chance-node handling under hidden info** — sampling / progressive widening of draws
  and **determinization** (information-set / PIMC) at the search root so the policy can't
  peek at hidden tiles (see `../CLAUDE.md` §4).
- **Placement policy head** — pointer over board-cell tokens × a 4-way rotation, masked
  by the engine's `legal_actions` (engine-design.md §4.2 Q2).
