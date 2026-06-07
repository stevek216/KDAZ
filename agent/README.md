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
- **DONE — encoder** (`kdagent/encoder.py`): `observation()` → tensors (per-seat 13×13
  board planes, 8 draft-line tokens, global vector) + an action batch aligned to
  `legal_actions()` (place = row/col/rot, claim = slot). `encode_obs` re-encodes raw corpus
  inputs. Tested in `kdagent/test_encoder.py` (shapes, plane partition, action alignment,
  raw round-trip, finite/bounded over random play).
- **DONE — network** (`kdagent/net.py`): a conv tower over the board planes + token/global
  MLPs, with a spatial place head (rotation × cell), a claim pointer over line tokens, a
  discard scalar, and a seat-relative (max-n) value head. `policy_value(enc)` returns
  per-action logits + value (the MCTS leaf interface). Runs on CPU/CUDA and is end-to-end
  trainable (`kdagent/test_net.py`).
- **DONE — MCTS** (`kdagent/mcts/`): AlphaZero-style search with max-n vector backups and
  explicit chance nodes **sampled** from the engine's true distribution (the draw has up to
  48 outcomes, so sparse-sampled, not fully expanded); forced single-action plies collapse.
  `RolloutEvaluator` (no net, validates search) and `NetEvaluator` (wraps the net). Tested
  (`kdagent/mcts/test_mcts.py`): valid policy/value, full games to terminal under both
  evaluators, seed determinism.
- **DONE — self-play corpus generator** (`kdagent/selfplay.py`): writes one JSONL record per
  decision (`{obs, legal, policy, to_act, value}`). Two backends:
  - `--backend rust` (default): **pure-Rust rollout MCTS, batched across cores** (rayon) via
    `kingdomino.selfplay_batch` (`engine-bridge/src/mcts.rs`) — no Python/FFI per step. ~67×
    the Python throughput (≈120k sims/s vs ≈1.8k on this machine).
  - `--backend python`: single-process MCTS (`rollout` or `net --ckpt`) for net-guided play.
  Same schema from both (re-encodable by the training encoder). `--no-write` = pure timing.
  e.g. `python -m kdagent.selfplay --games 64 --sims 128 --out data/selfplay/rollout.jsonl`.
- **TODO** — trainer (learn from the corpus) and arena (relative strength). Root
  determinization (PIMC) for competitive play layers on later.

Setup: `python -m venv .venv && .venv/Scripts/python -m pip install -r requirements.txt`,
then `maturin develop --release` in `engine-bridge/`.

Key Kingdomino-specific adaptations to design here (not in the engine):
- **Chance-node handling under hidden info** — sampling / progressive widening of draws
  and **determinization** (information-set / PIMC) at the search root so the policy can't
  peek at hidden tiles (see `../CLAUDE.md` §4).
- **Placement policy head** — pointer over board-cell tokens × a 4-way rotation, masked
  by the engine's `legal_actions` (engine-design.md §4.2 Q2).
