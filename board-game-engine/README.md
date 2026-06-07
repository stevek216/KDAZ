# Board Game Engine — Kingdomino AlphaZero

A game engine for **Kingdomino** (targeting the **Mighty Duel** 7×7 variant, plus the
**Harmony** and **Middle Kingdom** scoring variants), written in **Rust** for speed,
built as the foundation for an AlphaZero-style self-play reinforcement learning agent.

The architecture deliberately mirrors the Space Base AI project: a headless engine that
is the single source of rules truth, consumed by a Python search + learning layer and a
UI. See `../CLAUDE.md` for the project charter and `docs/engine-design.md` for the
state/action/scoring spec.

## Project Goals

1. **Game engine** — a fast, deterministic, fully-rules-accurate implementation of
   Kingdomino with a clean state/action interface (`legal_actions`, `apply_action`,
   `current_decision`, `chance_outcomes`/`apply_chance`, `terminal_value`).
2. **AlphaZero agent** — MCTS (with explicit chance nodes for the hidden draw) guided by
   a neural network, trained through self-play.

Rust is chosen so the engine can generate self-play games at high throughput; the design
keeps `GameState` cheaply `Copy` and free of global state so games run in parallel.

## Directory Structure

```
board-game-engine/
├── src/
│   ├── lib.rs         # Crate root: declares the modules below
│   ├── core/          # Core engine: game state, turn loop, action application
│   ├── components/    # Game pieces: terrains, dominoes, the kingdom board
│   ├── rules/         # Rule engine: legal actions, scoring, win conditions
│   └── utils/         # Helpers: serialization, RNG, logging
├── tests/             # Integration tests (unit tests live beside their code)
├── docs/              # Design notes (engine-design.md), rules references
├── examples/          # Example games (cargo run --example)
├── Cargo.toml
├── rustfmt.toml
├── README.md
└── CONTRIBUTING.md
```

## Getting Started

```bash
cargo build
cargo test
```

## Status

🚧 Early development — repo skeleton + design spec in place; engine implementation
proceeds in the chunks listed in `docs/engine-design.md` §9.
