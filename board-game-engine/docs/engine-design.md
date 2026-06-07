# Engine Design — `GameState` + actions + scoring (first cut)

Status: **proposal, for refinement.** This is the spec we review implementations
against. It covers the core interface, the domino/terrain model, the `GameState` layout,
the turn/decision state machine, the hidden-draw chance model, and scoring. See
`../../CLAUDE.md` for the durable invariants this obeys. Rule citations are to the
rulebook (`Docs/rules.pdf`).

This engine deliberately mirrors the Space Base engine's shape (`../../../SpaceBase/
board-game-engine/docs/engine-design.md`): pure functions over a `Copy` state, a static
data table for game content, explicit chance nodes, and max-n vector values. Read that
doc for the rationale behind patterns reused here.

Notation: `N` = active player count (target **2**; scaffolded ≤ `MAX_PLAYERS`).
`GRID` = kingdom side (target **7**, Mighty Duel; base game would be 5). `LINE` = draft
line width = `4`. `NUM_KINGS` = `4` (Mighty Duel: 2 per player).

---

## 1. The core interface

The whole engine is a handful of pure functions over an opaque, cheaply-clonable state.
This is the only surface MCTS / self-play / UI touch (identical in spirit to Space Base):

```rust
fn current_decision(&GameState) -> Decision;          // who acts, or "chance", or "terminal"
fn legal_actions(&GameState, &mut ActionBuf);         // fills a reused buffer, no alloc
fn apply_action(&mut GameState, Action);              // deterministic; player & engine steps
fn chance_outcomes(&GameState) -> impl Iterator<Item=(Action, f32)>; // hidden-draw distribution
fn apply_chance(&mut GameState, &mut Rng) -> Action;   // sample one outcome (the only RNG use)
fn terminal_value(&GameState) -> Option<[f32; MAX_PLAYERS]>; // max-n vector when over
```

```rust
enum Decision {
    Player(u8),     // seat `to_act` chooses among legal_actions
    Chance,         // a hidden-draw node — search expands/samples (see §6)
    Terminal,
}
```

As in Space Base, `apply_action` is **deterministic** (it can apply a *specific*
enumerated chance outcome too), and `apply_chance` is the single RNG entry point. The
RNG lives in the search/self-play driver, not in `GameState`, so the state stays `Copy`.

> **Key divergence from Space Base.** Its chance node (2d6) has 21 outcomes and is fully
> enumerated. Kingdomino's chance node is a draw from the remaining deck — far too many
> outcomes to enumerate. `chance_outcomes` therefore returns the **single-draw** uniform
> distribution (one entry per remaining domino), and the search **samples / progressively
> widens** rather than fully expanding (§6). The engine exposes both so the search may
> choose; it never assumes full expansion.

---

## 2. Domino & terrain model — data, not code

Tile *definitions* are a read-only static table; they are **not** part of `GameState` and
so do not affect clone cost. `GameState` references dominoes by an 8-bit id.

```rust
#[repr(u8)]
enum Terrain { Wheat, Forest, Lake, Grassland, Swamp, Mine }  // 6 terrains (rulebook p.1)

type DominoId = u8;                  // 0..47 == draft number 1..48 minus one
const NO_DOMINO: DominoId = u8::MAX; // NONE sentinel

struct Square { terrain: Terrain, crowns: u8 }  // crowns 0..=3

struct DominoDef {
    number: u8,        // 1..=48, the draft-order rank (number side)
    a: Square,         // the two terrain squares (ordered; placement puts `a` at the anchor)
    b: Square,
}

static DOMINOES: &[DominoDef; 48] = &[ /* sourced from authoritative data — chunk 1 */ ];
```

Because defs are static and shared, they can be as rich as needed with zero hot-path
cost. **Adding/altering a tile = editing a row** and rebuilding. `DominoId == number - 1`
so the draft line (sorted ascending by number) is sorted ascending by id — convenient.

> **Q-data — SOURCED & INGESTED (2026-06-07).** All 48 dominoes were captured from the
> BoardGameArena client (`gameui.gamedatas.dominoesDescription`) and saved to
> `docs/bga/kingdomino_dominoes_bga.json` (provenance + schema in `docs/bga/README.md`).
> The `const DOMINOES` table in `src/components/domino.rs` is transcribed from it and
> guarded by tests that re-derive the published tallies (squares per terrain
> 26/22/18/14/10/6 = 96; crowns per terrain 5/6/6/6/6/10 = 39). BGA's `mountain` is the
> mine terrain (`Terrain::from_bga`). `gridSize` confirmed `7`.

---

## 3. `GameState` layout

Design goals (CLAUDE §3): ideally `Copy`, fixed-size, no heap. Per-player data is
`[T; MAX_PLAYERS]` indexed by seat; only `player_count` seats are live. Kingdomino state
is naturally tiny.

```rust
struct GameState {
    // --- meta ---
    player_count: u8,
    phase: Phase,
    to_act: u8,                    // whose DECISION is pending
    round: u8,                     // line-fill index (0..=11 for Mighty Duel)
    rng_unused: (),                // (RNG is injected at chance nodes; not stored — Q like SB Q4)

    // --- the draft ---
    current_line: [Slot; LINE],    // already-claimed dominoes being placed this round
    next_line:    [Slot; LINE],    // dominoes being claimed this round
    turn_cursor:  u8,              // index into current_line (play order = ascending slot)
    king_owner:   [u8; NUM_KINGS], // king -> owning seat (Mighty Duel: 2 kings/seat)
    remaining:    u64,             // bitmask over the 48 dominoes still in the deck
    draw_buf:     [DominoId; LINE],// dominoes drawn so far for the line being filled
    draw_count:   u8,              // how many of `LINE` have been drawn

    // --- per-player kingdoms ---
    boards: [Board; MAX_PLAYERS],
}

struct Slot { domino: DominoId, king: u8 }  // king == NO_KING until claimed
```

`Slot.king` records which king (hence which seat, via `king_owner`) claimed the domino;
play order within a round walks `current_line` by ascending slot index (= ascending
domino number, rulebook p.2). `MAX_PLAYERS` is a small const (4 covers the base game);
the target sets `player_count = 2`.

### 3.1 The board (per player) — the kingdom grid

The 7×7 bound is a **sliding window**, not a fixed frame: the castle can end up anywhere
within the final 7×7 extent. To keep placement trivial and `Copy`, store a generous
fixed grid with the **castle fixed at the center** and enforce the 7×7 extent as a
bounding-box check.

```rust
const STORE: usize = 2 * GRID - 1;          // 13 for GRID=7 — room to grow ±(GRID-1) from center
const CENTER: u8 = (STORE / 2) as u8;        // castle cell (6,6) for STORE=13

struct Board {
    cells: [[Cell; STORE]; STORE],           // 13×13 = 169 cells; castle pre-placed at CENTER
    // occupied bounding box, maintained incrementally for the 7×7 bound + scoring/variants:
    min_r: u8, max_r: u8, min_c: u8, max_c: u8,
    filled: u8,                              // count of occupied terrain cells (excl. castle)
}

// Packed: terrain (0=empty, 1..6, 7=castle) in low 3 bits, crowns (0..3) in next 2 bits.
#[derive(Clone, Copy)]
struct Cell(u8);
```

`Board` ≈ 169 + a few bytes; `[Board; 4]` ≈ 700 B. The whole `GameState` is well under
1 KB and trivially `Copy` — a `gamestate_is_copy_and_bounded` test guards it (mirrors
Space Base).

**The 7×7 bound.** A placement is legal only if, after placing, both
`max_r - min_r < GRID` and `max_c - min_c < GRID` (the occupied extent, castle included,
fits in `GRID×GRID`). Because the castle sits at `CENTER` of a `(2·GRID−1)` store and
growth is at most `GRID−1` in any direction, every legal kingdom fits without origin
bookkeeping.

> **Q1 — store size vs. 7×7 frame.** Chosen: the `(2·GRID−1)²` centered store (simple,
> `Copy`, no origin math). Alternative: a tight `GRID×GRID` array with a dynamic origin
> offset — smaller but fiddlier and bug-prone. Given clone cost is already trivial,
> simplicity wins. Revisit only if a profiler ever says these bytes matter.

---

## 4. Turn / decision state machine

A game is a sequence of **line-fills** (rounds). Mighty Duel: 12 fills, 48 dominoes.
Decisions are `Claim` and `Place`; the draw is `Chance`. `legal_actions` is a pure
function of `(phase, to_act, boards, lines)`.

```rust
enum Phase {
    Draw,          // CHANCE: fill the line being drawn (next_line, or both lines at setup)
    StartOrder,    // CHANCE: random claim order for the starting round (rulebook p.2)
    StartClaim,    // starting round: each king claims a current_line domino, in StartOrder
    Place,         // a king's owner places that king's current_line domino (or discards)
    Claim,         // that same owner claims a next_line domino with that king
    GameOver,      // terminal
}
```

### 4.1 Flow

**Setup.**
1. `Draw` (chance) fills line 1 (the first `current_line`).
2. `StartOrder` (chance) picks the order the 4 kings claim line 1 (the rulebook's
   "pull kings from a hand"). With 2 kings/seat there are `4!/(2!·2!) = 6` distinct
   seat-orders; modeled as a chance node (§6.2).
3. `StartClaim` × 4: each king, in StartOrder, claims an unclaimed line-1 domino
   (`Action::Claim{slot}`). This sets the kings on `current_line`.
4. `Draw` (chance) fills line 2 (the first `next_line`).

**Each play round** (driven by `current_line` kings in ascending slot order via
`turn_cursor`): for the king at the cursor, owner `p = king_owner[king]`:
- `Place` — `p` places the domino that king claimed (the `current_line` slot's domino)
  into board `p`: `Action::Place{anchor, rot}`, or the forced `Action::Discard` when no
  legal placement exists (§5).
- `Claim` — `p` claims an unclaimed `next_line` domino with that king:
  `Action::Claim{slot}`. (Skipped in the final round — no next line.)
- advance `turn_cursor`; when all `LINE` kings have acted → `Draw` the next line, then
  `next_line` becomes `current_line`, reset cursor, `round += 1`.

**Final round.** When the deck is empty there is no `next_line`: the last `current_line`
is only **placed** (no `Claim`), in order, then → `GameOver`.

Deterministic bookkeeping (rotate lines, advance cursor, increment round, detect game
end) folds into the tail of `apply_action`, never producing a spurious decision node —
same discipline as Space Base's deterministic tail.

### 4.2 Action space

```rust
enum Action {
    Claim { slot: u8 },            // claim next_line[slot] (or current_line[slot] in StartClaim)
    Place { anchor: u16, rot: u8 },// place current domino: square `a` at cell `anchor`, `b` toward rot
    Discard,                       // forced: only when no legal Place exists
    Draw  { domino: DominoId },    // CHANCE outcome (engine-generated; not a player pick)
    StartOrder { perm: u8 },       // CHANCE outcome: index into the starting-claim orders
}
```

- `anchor` is a cell index `0..STORE*STORE`; `rot ∈ {0,1,2,3}` places square `b` to the
  N/E/S/W of `a`. The two squares are distinct, so all 4 rotations are distinct
  placements. `legal_actions` emits only legal `(anchor, rot)` pairs (both cells empty &
  in store, 7×7 bound holds, ≥1 connection — §5). This is a small set (≤ a few hundred,
  usually far fewer), so enumeration is cheap and needs no alloc beyond the reused buffer.
- `Claim{slot}` is legal only for unclaimed slots holding a real domino.

> **Q2 — placement encoding for the policy head.** `(anchor, rot)` is the engine-native
> form. For the agent's pointer-style policy (`agent/docs/feature-schema.md`, to come),
> placements map naturally onto board-cell tokens; the engine mask stays authoritative.
> Confirm the exact factorization (anchor pointer × 4-way rot head vs. flat enumerated
> list) when the encoder is designed — it does not affect the engine.

---

## 5. Placement legality (the crux of `legal_actions`)

A `Place{anchor, rot}` for the current domino `(a, b)` is legal iff **all** hold
(rulebook p.2–3):

1. **Both cells empty & in store.** `anchor` and its `rot`-neighbor are within the
   `STORE` grid and currently `empty`.
2. **7×7 bound.** Extending the occupied bounding box (castle included) to cover both new
   cells keeps `max_r-min_r < GRID` and `max_c-min_c < GRID`.
3. **Connection.** At least one of the two new cells is orthogonally adjacent to an
   existing occupied cell that is **either** the **castle** (wild — any terrain connects)
   **or** a terrain cell of the **same terrain** as the new cell touching it. (The
   rulebook's "≥2 connecting squares of the same terrain, one on each" = at least one
   same-terrain adjacency across the seam; the domino's own two squares need not both
   connect.)

**Discard rule.** If `legal_actions` finds **no** legal `Place`, the only legal action is
`Action::Discard` (the domino is removed, scores nothing). A domino **cannot** be
discarded if any legal placement exists (rulebook p.3) — so `Discard` is emitted *only*
when the placement set is empty. This makes discard a forced, decision-free move (the
engine may auto-apply it; it never appears alongside placements).

> **Q3 — connection via castle precise reading.** The castle's 4 sides are wild
> (rulebook p.2: "The 4-sides of the starting tile are wild; any terrain can be connected
> to them"). Interpreted as: a new cell orthogonally adjacent to the castle always
> satisfies the connection requirement regardless of terrain. Confirm there's no edge
> case where a domino touches *only* the castle diagonally (diagonal never connects).

---

## 6. Chance nodes — the hidden draw

The deck is hidden (CLAUDE §3): outcomes are not pre-fixed from the seed. A `Draw` node
reveals dominoes from `remaining` uniformly at random.

### 6.1 Single-draw model

A line of `LINE` dominoes is filled by `LINE` **single-draw** chance steps, accumulated
in `draw_buf`/`draw_count`. Each step:
- `chance_outcomes` = `{ (Draw{d}, 1/k) : d ∈ remaining }`, where `k = remaining.count_ones()`.
- `apply_action(Draw{d})` clears bit `d`, pushes `d` into `draw_buf`.
- When `draw_count == LINE`, the engine **sorts** `draw_buf` ascending by id (= number)
  into the target line, clears the buffer, and advances to the next decision phase.

Rationale: a single chance node has ≤ 48 outcomes (bounded, uniform) — clean to **sample**
(`apply_chance`) and to **partially expand** in search. Modeling the whole 4-draw as one
node would have `C(k,4)` outcomes (≈ 194k at the start) — infeasible to enumerate.

> **Permutation redundancy (documented).** Because the line is sorted, the *order* of the
> 4 single-draws is irrelevant — different draw orders collapse to the same line. For
> self-play sampling this is harmless. For search, expanding all single-draw children is
> still large; the search is expected to **sample / progressively widen** chance nodes
> rather than fully expand (an agent-layer concern, noted in CLAUDE §4).

### 6.2 Starting-claim order

`StartOrder` is a chance node choosing the order the 4 kings claim the first line. With 2
kings/seat the strategically-relevant object is the **seat order** (which seat claims
1st/2nd/3rd/4th), of which there are `4!/(2!2!) = 6`; modeled as 6 equiprobable outcomes
(`StartOrder{perm}`), or the 24 king-permutations if king identity is ever needed. This
is the only chance event besides draws.

> **Q4 — is StartOrder worth a chance node, or fold into setup?** It only affects the
> opening. Kept explicit so self-play sees varied openings and the search treats it
> faithfully; cheap either way.

---

## 7. Scoring & terminal value

At `GameOver`, score each board:

```
score(board) = Σ_territory (size × crowns)
             + harmony_bonus     // +5 if enabled & full gap-free GRID×GRID
             + middle_bonus      // +10 if enabled & castle centered
```

**Harmony and Middle Kingdom are purely additive end-scoring bonuses — they never
constrain legal play (rulebook p.4, clarified 2026-06-07).** Placement legality (§5) is
identical with or without them: a player may freely build a kingdom with gaps or an
off-center castle; they simply forfeit the corresponding bonus. The two are independent —
a board can earn both, either, or neither.

- **Territory** = a maximal set of orthogonally-connected cells of the **same terrain**
  (castle excluded; it has no terrain). Flood-fill over `cells`. `size` = cell count,
  `crowns` = total crowns in the territory. Crownless territories contribute 0
  (rulebook p.3). Multiple disconnected same-terrain territories score separately.
- **Harmony** (+5): the kingdom is a complete gap-free grid — the occupied bounding box
  spans exactly `GRID` in both axes **and** every cell in it is filled, i.e.
  `filled + 1(castle) == GRID*GRID` (rulebook p.4). Requires zero discards. (No effect on
  legality — an incomplete or gappy kingdom is still legal, just unrewarded.)
- **Middle Kingdom** (+10): the castle is **centered** — the occupied bounding box spans
  exactly `GRID` in both axes and the castle sits at its center cell (offset `GRID/2` from
  each edge of the box). Gaps allowed. Independent of Harmony.

Variant toggles live in a small `Rules`/config carried alongside (or as `const` features
for the target build); the target enables both Harmony and Middle Kingdom.

> **Q5 — variant bonus definitions (mostly resolved).** Non-enforcement is **settled**
> (2026-06-07): both are additive bonuses, never constraints. The remaining detail is the
> exact geometric test for *centered* / *complete*. The bounding-box readings above are the
> natural interpretation (and imply a kingdom whose footprint is smaller than `GRID×GRID`
> can earn neither bonus — consistent with "complete grid" / "centered in the grid"). To be
> verified empirically against **BoardGameArena's** scoring on a live game before freezing
> the scoring tests (CLAUDE §6).

### 7.1 Terminal value

`terminal_value -> Option<[f32; MAX_PLAYERS]>` (max-n vector, one entry per seat; CLAUDE
§4). Highest total score wins; **tie → largest single territory** (most connected
same-terrain squares); still tied → **shared victory** (rulebook p.3).

> **Q6 — value convention.** Proposal (swappable at trainer time, like Space Base):
> `1.0` to the winner, `0.0` to the loser, `0.5` each on a fully-shared victory. (Space
> Base used `1.0` to all tied winners; for a 2p minimax setting `0.5` is the natural
> zero-sum midpoint. Decide at trainer time — the engine just reports the ranking.)

---

## 8. Open questions to resolve before / during coding

- **Q-data** — source & verify the 48-domino table (chunk 1). Blocks full-game tests.
- **Q1** — board store size (centered `(2·GRID−1)²` chosen).
- **Q2** — placement encoding for the policy head (engine-neutral).
- **Q3** — castle-wild connection precise reading.
- **Q4** — StartOrder as a chance node (kept).
- **Q5** — Harmony / Middle Kingdom exact definitions (confirm vs. rulebook/FAQ).
- **Q6** — terminal value convention (trainer-time choice).

## 9. Suggested build order (chunks)

1. **Domino data + ingest/validation — DONE (2026-06-07).** `Terrain`/`Square`/`DominoDef`
   and the sourced `DOMINOES` table in `src/components/`, with tally-guard tests (counts,
   sequential numbers, crown ranges, published terrain/crown totals). Mirrors Space Base
   chunk 1.
2. **`GameState` skeleton + setup — DONE (2026-06-07).** `core/state.rs` (the `Copy`
   `GameState`: packed `Cell`, centered `Board` with bounding box, draft `Slot` lines, 48-bit
   `remaining` deck, `Phase`) + `core/setup.rs` `new_game(player_count)`, which places castles,
   loads the full deck, and positions the game at the first `Phase::Draw` chance node. (The
   draws/`StartOrder`/`StartClaim` are driven by the chunk-3 turn loop, since the RNG lives in
   the driver, not `new_game`.) Guarded by Copy/size + cell-packing + bbox + setup tests.
3. **The turn loop — DONE (2026-06-07).** `core/action.rs` (`Action`/`Decision`),
   `rules/place.rs` (placement legality + enumeration, §5), and `core/turn.rs`
   (`current_decision` / `legal_actions` / `apply_action` / `chance_outcomes` / `apply_chance`
   for `Draw`, `StartOrder`, `StartClaim`, `Place`/`Discard`, `Claim`, line rotation,
   round/cursor advance, end-of-game). Single-draw chance model (§6.1) + distinct starting
   orders via `next_permutation` (§6.2). Random self-play runs to terminal for 2p and 4p with
   domino conservation, the 7×7 bound, and seed determinism asserted (`tests/full_game.rs`).
   `terminal_value` is **not** here yet — it needs scoring (chunk 4); the game reaches
   `Phase::GameOver` and `Decision::Terminal`, but its value vector is chunk 4.
4. **Scoring + terminal value** — territory flood-fill, variant bonuses, tie-break.
5. **Property tests** — square conservation (48 = placed + discarded across both boards),
   7×7 bound never violated, every claimed domino placed-or-discarded, a full game from a
   fixed seed reproduces, scoring matches hand-computed examples (incl. the rulebook's
   23-point sample on p.4).
6. **PyO3 bridge + agent** — mirror `agent/` from Space Base once the engine is green.
```

