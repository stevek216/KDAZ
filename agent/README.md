# Kingdomino agent (search + learning) — to come

The AlphaZero-style player for Kingdomino: the neural network, MCTS search (with explicit
chance nodes for the hidden draw), self-play loop, and trainer. The Rust engine in
`../board-game-engine/` is the single source of rules truth — this layer only *consumes*
it and never reimplements a rule.

## Status

In progress (mirrors the Space Base agent, `../../SpaceBase/agent/`).

- **DONE — PyO3 bridge** (`engine-bridge/`, Python module `kingdomino`): a `Game` control
  API over the engine (`legal_actions`/`apply`/`chance_outcomes`/`apply_chance`/`clone`/
  `terminal_value`/`observation`) plus `domino_table()`. Build with
  `../.venv/Scripts/python -m maturin develop --release`; `smoke_test.py` drives full games.
- **DONE — feature schema** (`docs/feature-schema.md`): the engine↔encoder contract
  (board planes + draft tokens + global; spatial place head, claim pointer; max-n value).
- **TODO** — encoder (NumPy/Torch), network, MCTS (with chance-node handling +
  determinization), self-play corpus (append-only JSONL of `{obs, legal, policy, value}`),
  trainer, arena.

Setup: `python -m venv .venv && .venv/Scripts/python -m pip install -r requirements.txt`,
then `maturin develop --release` in `engine-bridge/`.

Key Kingdomino-specific adaptations to design here (not in the engine):
- **Chance-node handling under hidden info** — sampling / progressive widening of draws
  and **determinization** (information-set / PIMC) at the search root so the policy can't
  peek at hidden tiles (see `../CLAUDE.md` §4).
- **Placement policy head** — pointer over board-cell tokens × a 4-way rotation, masked
  by the engine's `legal_actions` (engine-design.md §4.2 Q2).
