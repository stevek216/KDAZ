# Kingdomino AI — Project Charter

This file is the durable context for the project. It captures **goals, invariants,
and conventions** that outlive any single conversation. Code structure, fixed bugs,
and git history live in the code and history — not here. Keep this short and true.

We coordinate through repo artifacts (this file + design docs), not long chat
memory. Workflow is **plan → implement in chunks → review the implementation
against the spec**.

This project deliberately **reuses the architecture of the Space Base AI project**
(`../SpaceBase`): a headless Rust engine as the single source of rules truth, an
AlphaZero-style search + learning layer in Python that only *consumes* the engine, and
a UI to play and assess the agent. Where Kingdomino differs from Space Base, this file
and `docs/engine-design.md` call it out explicitly.

---

## 1. What we're building

An AI to play **Kingdomino**, targeting the **Mighty Duel** variant (2 players, all 48
dominoes, a **7×7** kingdom) plus two scoring variants — **Harmony** (+5 for a complete
gap-free grid) and **Middle Kingdom** (+10 for a centered castle). Long-term goal: a
strong **self-play-trained** player, plus a polished UI to play against it and assess
its strength.

Three pieces (mirrors Space Base):
1. A **headless game engine** (Rust) — the single source of rules truth.
2. A **search + learning** layer (AlphaZero-style; see §4) that calls the engine.
3. A **UI** for a human to play the AI and watch games.

## 2. Priorities (in order — resolve trade-offs this way)

1. **Correctness of the game engine.** Nonnegotiable. A wrong rule poisons every
   game generated. When unsure of a rule, stop and ask; cite the rulebook
   (`Docs/rules.pdf`).
2. **Speed.** Self-play generates huge numbers of games × many simulations each.
   The engine hot path (clone state, generate legal actions, apply action) must be
   lean and allocation-free.
3. **A polished, pleasant UI** for playing the AI and assessing strength.

Speed never buys a correctness compromise; UI polish never buys an engine compromise.

## 3. Engine design decisions (settled — treat as invariants)

- **Headless core is the single source of rules truth.** The UI, self-play loop,
  search (MCTS), and trainer all call into it. **The UI must NEVER reimplement any
  rule.** If the UI needs a rules answer, it asks the engine.
- **`GameState` must be cheaply clonable — ideally `Copy`.** Fixed-size arrays; no
  `Vec`/`Box`/`HashMap` in hot-path state. **Cheap cloning is the #1 speed lever** for
  the search loop — protect it in review. (Kingdomino state is *small*: a couple of
  packed grids + a draft line + a deck bitmask. This is much cheaper to clone than
  Space Base's deployed stacks — keep it that way.)
- **Dominoes are data, evaluated against a static table.** A `DominoDef` carries the
  two squares' `(Terrain, crowns)` and the draft number. **Adding/altering tiles is
  data entry, not new code.** The table is **frozen against the real 48-domino set**,
  not guessed (see §6 — sourcing the authoritative tile data is chunk 1).
- **Explicit, seeded RNG threaded through the chance entry point.** No global `rand`.
  Games are fully reproducible from a seed (`rand_chacha`).
- **Hidden draw pile modeled as explicit chance nodes** (decided 2026-06-06). The
  shuffled deck is *not* fixed perfect-info at game start — revealing the next dominoes
  is a `Chance` node the search expands/samples, so the agent never "sees the future."
  This is the main divergence from Space Base (which fixes deck order from the seed) and
  is deliberate: the hidden draft is central to Kingdomino strategy. See
  `docs/engine-design.md` §6 for the single-draw chance-node model and the
  determinization note for search.
- **Player-count scaffolding kept, 2p committed (decided 2026-06-06).** The target is
  strictly **2-player Mighty Duel**, but the cheap parts of generality are retained:
  per-seat **max-n / vector-valued** backups and value head, `[T; MAX_PLAYERS]`
  per-player arrays, and grid size as a `const`. This leaves the door open to base 5×5 /
  2–4p later at low cost without forcing that complexity now.

### Rules facts that shape the state machine (from `Docs/rules.pdf`)

- **6 terrains** (wheat field, forest, lake, grassland, swamp, mine) + the castle
  starting tile (a single square whose 4 sides are **wild** — any terrain connects to
  it). Squares carry **0–3 crowns**.
- **48 dominoes**, each a pair of terrain squares with a unique **draft number** 1–48.
  The number side orders the draft line (ascending).
- **Mighty Duel turn structure:** 2 players, **2 kings each**, 4 dominoes drawn per
  round. A round draws a fresh line of 4; play order = the kings' positions on the
  *current* (already-claimed) line, smallest number first. On a king's turn its owner
  (1) **places** the domino that king claimed last round, then (2) **claims** a domino
  from the next line with that king. A player owns 2 kings, so acts twice per round.
  12 line-fills total → each player drafts 24 dominoes = 48 squares + 1 castle = **7×7**.
- **Placement (Connection Rules):** a domino's 2 squares go on 2 empty orthogonally
  adjacent cells; the whole kingdom (incl. castle) must always fit within a **7×7**
  bounding box; at least one new square must touch an existing same-terrain square **or**
  the wild castle. A claimed domino with **no legal placement is discarded** (no points);
  it **cannot** be discarded if any legal placement exists.
- **Scoring:** for each **territory** (orthogonally-connected same-terrain squares),
  `size × crowns`; sum over territories. Crownless territories score 0. Then variant
  bonuses: **Harmony** +5 (full gap-free 7×7), **Middle Kingdom** +10 (castle centered).
- **Win:** highest score. Tie → **largest single territory** (most connected squares);
  still tied → **shared victory**.

## 4. Algorithm direction (background — not the engine's concern)

The engine is **algorithm-agnostic**. Primary plan: **AlphaZero-style MCTS**, adapted for:
- **Stochasticity** — explicit **chance nodes** for the hidden draw (§3). Unlike Space
  Base's 21-outcome 2d6, a draw is over the remaining deck and is **sampled / partially
  expanded**, not fully enumerated; competitive 2p play layers **determinization**
  (information-set / PIMC) at the search root so the policy can't peek at hidden tiles.
- **Multiplayer-ready** — **max-n** backups, **vector-valued value head**, one entry per
  seat, even though the target uses 2 seats (2p max-n = minimax).

## 5. Deferred / not worth worrying about yet

- **Base 5×5 game and 2–4 players.** Scaffolded for (§3) but not a target; revisit only
  if we want it after a strong Mighty Duel agent exists.
- **Dynasty variant** (best of 3 games) — a trivial outer wrapper over terminal scores;
  add late.
- **Effect/placement ordering subtleties** — none significant in Kingdomino vs. Space
  Base; placement is a single atomic choice.

## 6. Open questions (resolve before they block downstream work)

- **Authoritative domino data — SOURCED & INGESTED (2026-06-07).** All 48 dominoes were
  captured from the BoardGameArena client and live at
  `board-game-engine/docs/bga/kingdomino_dominoes_bga.json` (schema/provenance in
  `docs/bga/README.md`); the `const DOMINOES` table in `src/components/domino.rs` is
  transcribed from it and guarded by tally tests. BGA's `mountain` = the mine terrain.
  **Do not fabricate tile data** (Space Base was burned by a fabricated placeholder CSV).
- **Middle Kingdom / Harmony** — settled (2026-06-07) that both are **purely additive
  bonuses that never constrain legal play** (a board may have gaps / an off-center castle;
  it just forfeits the points). The only detail left is the exact geometric test for
  "complete" / "centered" — see `docs/engine-design.md` §7; verify against BoardGameArena
  scoring before freezing the scoring tests.

## 7. Conventions

- **Engine language: Rust.** Crate: `board-game-engine/` (lib `kingdomino_engine`).
- **Determinism.** All randomness flows through an injected, seedable RNG so games
  replay exactly from a seed.
- **No hidden/global mutable state.** Logic takes a state and returns the next state;
  safe for parallel self-play across cores.
- **Allocation-aware hot paths.** Legal-move generation and action application run
  millions of times — prefer stack data and reuse buffers.
- **Document rules decisions.** When you encode a Kingdomino rule, cite the rulebook
  reference in `docs/` so the engine stays auditable.
- **Tests:** unit tests beside the code (`#[cfg(test)] mod tests`); `tests/` for
  cross-module, end-to-end behavior (e.g. play a full game from a fixed seed).
- **Pre-PR checks:** `cargo test`, `cargo fmt --all -- --check`,
  `cargo clippy --all-targets -- -D warnings`.

## 8. Repo layout

```
Kingdomino/
├── CLAUDE.md                 # this file — durable project charter
├── Docs/rules.pdf            # the rulebook (source of rules truth for citations)
├── board-game-engine/        # the Rust engine crate (single source of rules truth)
│   ├── src/{core,components,rules,utils}/
│   ├── docs/engine-design.md # GameState + action + scoring spec (review against this)
│   ├── examples/  tests/
├── agent/                    # search + learning (AlphaZero-style) — mirrors SpaceBase (to come)
└── web/                      # browser UI (Rust→WASM + static front-end) — to come
```
