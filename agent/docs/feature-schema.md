# Kingdomino — feature & decision schema (engine-grounded)

Status: **proposal, for refinement.** Input representation and action space for the
AlphaZero-style agent. Every feature derives from a concrete field of the bridge
`observation()` (which serializes the public `GameState`; see
`agent/engine-bridge/src/lib.rs`) or the static `domino_table()`. This file is the contract
between the engine and the encoder; if the engine's state/action shape changes, this file
changes with it. Mirrors the role of `../../SpaceBase/agent/docs/feature-schema.md`.

## 0. Principles

- **Effects, not IDs.** A domino is encoded from its two squares' `(terrain, crowns)`, never
  a learned embedding of its id/number. (The number matters only as draft-order priority.)
- **Everything is public.** Kingdomino's only hidden information is the *order* of future
  draws, which lives in the chance sampler, not `GameState`. So the whole observation is
  fair game; nothing needs masking. The remaining deck is exposed as a set (§5).
- **Seat-relative.** Ownership and the value head are indexed relative to the decision seat
  (`self`, `opp` for 2p; generalize to `self, next, …` for ≤4p), so one network serves
  every seat.
- **Engine masks are authoritative.** Token/cell "actionable" hints are semantic only;
  legality always comes from `legal_actions()`.
- **Spatial board + a few tokens.** Kingdomino is a tile-placement game; the kingdom is
  encoded as aligned 2-D planes (a CNN-friendly grid) and the draft as a short token set.

## 1. Frozen vocabularies (from the engine)

- **Terrain** (`components::Terrain`, the `terrain` ints in the observation): `0 wheat`,
  `1 forest`, `2 lake`, `3 grassland`, `4 swamp`, `5 mine`. (BGA's "field"/"mountain".)
- **Crowns**: `0..=3` per square.
- **Phase** (`phase` string): `draw`, `start_order` (chance); `start_claim`, `place`,
  `claim` (player); `game_over` (terminal).
- **Action types** (`legal_actions()` / `chance_outcomes()` `type` field): `claim{slot}`,
  `place{anchor,rot,row,col}`, `discard`; chance `draw{domino}`, `start_order{perm}`.

## 2. Board planes (the kingdom) — spatial

The backing store is `STORE×STORE = 13×13` with the castle fixed at the center `(6,6)`
(`board_json` emits `cells` as `{r,c,terrain,crowns}` plus `castle`, `min/max_r/c`,
`filled`). Because the castle is always centered in the array, planes are **aligned across
games** — ideal for a CNN. Per seat, build a `13×13` stack:

| plane(s) | source | encoding |
|---|---|---|
| terrain one-hot ×6 | `cells[*].terrain` | 1.0 in the cell's terrain plane |
| crowns ×4 (one-hot 0..3) | `cells[*].crowns` | or a single `crowns/3` scalar plane |
| castle ×1 | `castle` | 1.0 at `(6,6)` |
| empty ×1 | derived | 1.0 where no terrain/castle |
| placeable-region ×1 | derived from `min/max_r/c` | cells that keep the 7×7 bound reachable (a hint; legality still from the mask) |

Stack **self** first, then opponent(s) (seat-relative order). For 2p: `self ⊕ opp`.

## 3. Draft-line tokens (×8)

`current_line` (place-from) and `next_line` (claim-from), 4 slots each (`line_json`). One
token per slot:

- `line_one_hot` (current / next), `slot_index/3`.
- `present` (`domino != null`).
- two squares: `a_terrain_one_hot[6]`, `a_crowns/3`, `b_terrain_one_hot[6]`, `b_crowns/3`
  (join `domino` → `domino_table()`), and `number/48` (draft priority).
- `owner_relative` one-hot: `self`, `opp…`, `none` (`owner` re-indexed vs. decision seat).
- `is_current_place_target` — for the `current_line` slot at `turn_cursor` (the domino self
  must place now); also surfaced as `current_domino` in the observation.
- `claimable_now` — hint: this slot appears as a legal `claim` action this node.

## 4. Global / context token (×1)

- `phase_one_hot` over the player phases (`start_claim`, `place`, `claim`).
- `to_act_relative`, `round/12`, `turn_cursor/3`.
- `variants`: `harmony`, `middle_kingdom` flags.
- `deck_remaining/48`.
- Per seat (self, opp…): `filled/48`, bbox spans `(max_r-min_r)/6`, `(max_c-min_c)/6`, and
  cheap score hints from the observation `scores[*]` (`crown_score`, `largest_territory`,
  current `harmony`/`middle_kingdom` eligibility) — all public, all from the engine's own
  `score_board`.

## 5. Remaining deck — set, not order

`remaining` is the list of domino ids still in the pile (order is *not* knowable and not
emitted). Encode as a length-48 multi-hot, or summarize by remaining `(terrain, crowns)`
counts (how many crowns of each terrain are still to come) — useful for valuing future
draws. This is the engine's chance support for `draw` nodes; the net never predicts a draw.

## 6. Policy heads — mapped to the engine `Action` space

`legal_actions()` enumerates the concrete legal actions with an `index`; logits are masked
to that set and `apply(index)` executes the pick.

- **Place head (spatial):** logits over `anchor (13×13) × rot (4)` = 676, masked to the
  legal `place` actions (`anchor`,`rot` are in each action). This is the main head — it maps
  one-to-one onto board cells, so a conv policy head is natural.
- **Claim head (pointer):** over the 4 line slots (`start_claim` → `current_line`,
  `claim` → `next_line`), masked to legal `claim{slot}`.
- **Discard:** a single logit, only ever legal alone (forced when no placement fits), so
  effectively automatic.
- **Chance is not policy.** `draw` / `start_order` nodes are expanded by MCTS from
  `chance_outcomes()`; the net is never asked for a chance prior.

## 7. Value head

Pooled board+context embedding → seat-relative value vector (`value[self], value[opp], …`),
present-masked for ≤4 players. Max-n target = `terminal_value()` (highest total; ties →
largest territory; remaining ties shared `1/w`). During MCTS backup, map relative → absolute
seats via the node's `to_act`.

## 8. Open items / to verify

- **Board encoding choice:** start with the centered 13×13 planes (§2). If the fixed frame
  wastes capacity, switch to a tight 7×7 cropped to the current bbox (the engine already
  tracks `min/max_r/c`) — but the centered frame keeps the castle and Middle-Kingdom
  geometry positionally stable, which likely helps.
- **Place-head factorization:** a flat `13×13×4` head vs. an `anchor` pointer × 4-way `rot`
  sub-head — implementation choice; the mask is identical either way.
- **MCTS over chance:** the `draw` node has up to 48 outcomes (early game); the search must
  sample / progressively widen rather than fully expand, and layer determinization at the
  root for competitive play (CLAUDE §4). This is a search-layer concern, not the encoder's.
