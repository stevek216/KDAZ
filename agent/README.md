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
- **DONE — trainer** (`kdagent/train.py`, `kdagent/dataset.py`): learns from a JSONL corpus.
  Loss = policy cross-entropy (MCTS visits vs the net's masked per-action policy) + value
  cross-entropy (seat-relative outcome vs the max-n head). `collate` re-encodes raw records
  and pads the variable action lists into index tensors so logits gather batch-wide; CUDA +
  `load_net`-compatible checkpoints (`.best.pt`/`.last.pt`), `--init-from` warm-start. Tested
  (`kdagent/test_train.py`: collation, a batch overfits, checkpoint round-trip). e.g.
  `python -m kdagent.train --corpus data/selfplay/rollout.jsonl --epochs 5 --device cuda`.
- **DONE — arena** (`kdagent/arena.py`): relative strength via seat-rotated lineups, scored by
  the engine's `terminal_value` (even = 1/players). Agents: `random`, `mcts:SIMS` (fast Rust
  rollout via `Game.mcts_policy`), `net:CKPT` (greedy policy), `netmcts:SIMS:CKPT` (net-guided
  MCTS). Win-rate + 95% CI + verdict. Tested (`kdagent/test_arena.py`): scores sum to 1, and
  **mcts:64 beats random 100%**. e.g.
  `python -m kdagent.arena --a netmcts:64:runs/net.best.pt --b mcts:64 --games 200 --device cuda`.
- **TODO** — the closed generate→train→evaluate→promote loop. Root determinization (PIMC) later.

Setup: `python -m venv .venv && .venv/Scripts/python -m pip install -r requirements.txt`,
then `maturin develop --release` in `engine-bridge/`.

Key Kingdomino-specific adaptations to design here (not in the engine):
- **Chance-node handling under hidden info** — sampling / progressive widening of draws
  and **determinization** (information-set / PIMC) at the search root so the policy can't
  peek at hidden tiles (see `../CLAUDE.md` §4).
- **Placement policy head** — pointer over board-cell tokens × a 4-way rotation, masked
  by the engine's `legal_actions` (engine-design.md §4.2 Q2).
