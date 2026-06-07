//! Core game state: the cheaply-clonable `GameState` and its parts.
//!
//! See `docs/engine-design.md` §3. Invariants (CLAUDE §3): `GameState` is `Copy`,
//! fixed-size, no heap. Per-player data is `[T; MAX_PLAYERS]`; only `player_count` seats
//! are live. Dominoes are referenced by [`DominoId`] into the static
//! [`crate::components::DOMINOES`] table; the deck is a 48-bit `remaining` mask (the draw is
//! an explicit chance node, so order is never stored — §6).

use crate::components::{DominoId, Terrain, NO_DOMINO};

/// Player-count scaffolding cap (base game allows up to 4; the Mighty Duel target uses 2).
pub const MAX_PLAYERS: usize = 4;

/// Kingdom side length. Target = Mighty Duel 7×7 (base game would be 5). See CLAUDE §3.
pub const GRID: usize = 7;

/// Backing-store side: a centered `(2·GRID−1)²` grid with the castle fixed at the middle,
/// so the kingdom can grow up to `GRID−1` in any direction without origin bookkeeping
/// (`docs/engine-design.md` §3.1). The `GRID×GRID` extent is enforced as a bounding-box
/// check, not by the array size.
pub const STORE: usize = 2 * GRID - 1;

/// The castle's fixed row/column in the backing store (center of `STORE`).
pub const CENTER: u8 = (STORE / 2) as u8;

/// Draft line width — always 4 dominoes drawn per round, in every variant/player count.
pub const LINE: usize = 4;

/// `Slot.owner` sentinel: the slot's domino is not yet claimed.
pub const NO_OWNER: u8 = u8::MAX;

/// A bitmask of all 48 dominoes present (the full deck).
pub const FULL_DECK: u64 = (1u64 << 48) - 1;

/// Optional end-scoring bonuses (`docs/engine-design.md` §7). Both are **purely additive** —
/// they never constrain legal play, only the final score. Independent of each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Variants {
    /// Harmony: +5 if the kingdom is a complete gap-free `GRID×GRID`.
    pub harmony: bool,
    /// Middle Kingdom: +10 if the castle is centered in the kingdom's bounding box.
    pub middle_kingdom: bool,
}

impl Variants {
    /// The target configuration: Mighty Duel with both bonuses enabled.
    pub const MIGHTY_DUEL: Variants = Variants {
        harmony: true,
        middle_kingdom: true,
    };
    /// Plain scoring, no bonuses (base game).
    pub const NONE: Variants = Variants {
        harmony: false,
        middle_kingdom: false,
    };
}

/// One backing-store cell, packed into a byte: terrain code in the low 3 bits
/// (`0` = empty, `1..=6` = `Terrain::index() + 1`, `7` = castle), crowns in bits 3–4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Cell(u8);

const CASTLE_CODE: u8 = 7;

impl Cell {
    /// An empty cell.
    pub const EMPTY: Cell = Cell(0);
    /// The castle / starting tile (wild; no terrain, no crowns).
    pub const CASTLE: Cell = Cell(CASTLE_CODE);

    /// A terrain square with `crowns` (0..=3) crowns.
    pub fn terrain(terrain: Terrain, crowns: u8) -> Cell {
        debug_assert!(crowns <= 3);
        Cell((terrain.index() + 1) | (crowns << 3))
    }

    pub fn is_empty(self) -> bool {
        self.0 & 0b111 == 0
    }

    pub fn is_castle(self) -> bool {
        self.0 & 0b111 == CASTLE_CODE
    }

    /// The cell's terrain, or `None` for empty/castle (neither has a scorable terrain).
    pub fn terrain_of(self) -> Option<Terrain> {
        match self.0 & 0b111 {
            code @ 1..=6 => Terrain::from_index(code - 1),
            _ => None,
        }
    }

    /// Crowns on this cell (0 for empty/castle).
    pub fn crowns(self) -> u8 {
        (self.0 >> 3) & 0b11
    }
}

/// One player's kingdom: the centered backing-store grid plus the incrementally-maintained
/// occupied bounding box (castle included) used for the `GRID×GRID` bound and scoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    pub cells: [[Cell; STORE]; STORE],
    /// Occupied bounding box (inclusive), in backing-store coordinates. Valid only when the
    /// board has its castle (`min_r..=max_r`, `min_c..=max_c`).
    pub min_r: u8,
    pub max_r: u8,
    pub min_c: u8,
    pub max_c: u8,
    /// Count of placed *terrain* squares (excludes the castle). 0..=48.
    pub filled: u16,
    /// Whether this seat is live (has a castle). Dead scaffolding seats are `false`.
    pub present: bool,
}

impl Board {
    /// A dead/absent seat: no castle, no cells.
    pub fn empty() -> Self {
        Self {
            cells: [[Cell::EMPTY; STORE]; STORE],
            min_r: CENTER,
            max_r: CENTER,
            min_c: CENTER,
            max_c: CENTER,
            filled: 0,
            present: false,
        }
    }

    /// A fresh kingdom with the castle at the center of the **backing store** — an origin
    /// trick so the kingdom can grow up to `GRID-1` in any direction (no negative indices).
    /// This is *not* a game constraint: the castle may end up anywhere in the final kingdom
    /// (the `GRID×GRID` bound is checked against the real occupied bbox via [`fits_bound`]);
    /// keeping it centered is optional and only earns the Middle Kingdom bonus at scoring.
    pub fn with_castle() -> Self {
        let mut b = Self::empty();
        b.cells[CENTER as usize][CENTER as usize] = Cell::CASTLE;
        b.present = true;
        b
    }

    /// Read the cell at backing-store `(r, c)`.
    pub fn cell(&self, r: u8, c: u8) -> Cell {
        self.cells[r as usize][c as usize]
    }

    /// Place a terrain square at `(r, c)`, updating the bounding box and `filled`. The caller
    /// is responsible for legality (emptiness, bound, connection — `rules`, chunk 3); this is
    /// the low-level mutation. `(r, c)` must be inside the store.
    pub fn place_square(&mut self, r: u8, c: u8, terrain: Terrain, crowns: u8) {
        debug_assert!((r as usize) < STORE && (c as usize) < STORE);
        debug_assert!(self.cell(r, c).is_empty(), "placing on a non-empty cell");
        self.cells[r as usize][c as usize] = Cell::terrain(terrain, crowns);
        self.min_r = self.min_r.min(r);
        self.max_r = self.max_r.max(r);
        self.min_c = self.min_c.min(c);
        self.max_c = self.max_c.max(c);
        self.filled += 1;
    }

    /// Would adding cells at the given backing-store coordinates keep the whole kingdom
    /// (castle + existing squares + these) within a `GRID×GRID` box? (`docs/engine-design.md`
    /// §5 rule 2.) Pure check — does not mutate.
    pub fn fits_bound(&self, coords: &[(u8, u8)]) -> bool {
        let (mut lo_r, mut hi_r, mut lo_c, mut hi_c) =
            (self.min_r, self.max_r, self.min_c, self.max_c);
        for &(r, c) in coords {
            lo_r = lo_r.min(r);
            hi_r = hi_r.max(r);
            lo_c = lo_c.min(c);
            hi_c = hi_c.max(c);
        }
        (hi_r - lo_r) < GRID as u8 && (hi_c - lo_c) < GRID as u8
    }
}

/// A draft-line slot: a domino and who (if anyone) has claimed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub domino: DominoId,
    /// Claiming seat, or [`NO_OWNER`] until claimed. (We track the claiming seat per slot
    /// rather than a separate king→seat map — equivalent for play, since round order is just
    /// the claimed slots in ascending order; see `docs/engine-design.md` §3.)
    pub owner: u8,
}

impl Slot {
    pub const EMPTY: Slot = Slot {
        domino: NO_DOMINO,
        owner: NO_OWNER,
    };

    pub fn is_filled(self) -> bool {
        self.domino != NO_DOMINO
    }

    pub fn is_claimed(self) -> bool {
        self.owner != NO_OWNER
    }
}

/// The decision/chance phases of the turn state machine (`docs/engine-design.md` §4).
/// Deterministic bookkeeping (line rotation, round/cursor advance, end detection) folds into
/// `apply_action` and never appears as its own node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// CHANCE: draw dominoes one at a time into the line being filled (§6.1).
    Draw,
    /// CHANCE: random claim order for the starting round (§6.2).
    StartOrder,
    /// Starting round: a seat claims a `current_line` domino, in the drawn order.
    StartClaim,
    /// A seat places the domino its king claimed last round (or discards — §5).
    Place,
    /// That same seat claims a `next_line` domino with that king.
    Claim,
    /// Terminal.
    GameOver,
}

/// The complete game state. `Copy` and fixed-size by construction (CLAUDE §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameState {
    pub player_count: u8,
    /// Which optional scoring bonuses are active (affects `terminal_value` only).
    pub variants: Variants,
    pub phase: Phase,
    /// Whose decision is pending (seat index). Not meaningful at chance nodes.
    pub to_act: u8,
    /// Line-fill index, 0-based (0..=11 for a Mighty Duel game).
    pub round: u8,

    /// Already-claimed dominoes being placed this round.
    pub current_line: [Slot; LINE],
    /// Dominoes being claimed this round (next round's `current_line`).
    pub next_line: [Slot; LINE],
    /// Index into `current_line` of the king whose turn it is (play order = ascending slot).
    /// During `StartClaim` it indexes `claim_order` instead (the starting-round claim step).
    pub turn_cursor: u8,
    /// Starting-round claim order: the seat that claims 1st, 2nd, … (set by the `StartOrder`
    /// chance node, §6.2). Only meaningful during setup.
    pub claim_order: [u8; LINE],

    /// Dominoes still in the draw pile (bit `d` set ⇒ domino id `d` remains). The draw is a
    /// chance node, so only membership is stored, never order (§6).
    pub remaining: u64,
    /// Dominoes drawn so far for the line currently being filled (sorted into a line once
    /// `draw_count == LINE`).
    pub draw_buf: [DominoId; LINE],
    pub draw_count: u8,

    /// Per-seat kingdoms (only `player_count` are `present`).
    pub boards: [Board; MAX_PLAYERS],
}

impl GameState {
    /// A zeroed shell with no live seats; [`crate::core::setup::new_game`] fills it.
    pub(crate) fn blank() -> Self {
        Self {
            player_count: 0,
            variants: Variants::NONE,
            phase: Phase::Draw,
            to_act: 0,
            round: 0,
            current_line: [Slot::EMPTY; LINE],
            next_line: [Slot::EMPTY; LINE],
            turn_cursor: 0,
            claim_order: [0; LINE],
            remaining: 0,
            draw_buf: [NO_DOMINO; LINE],
            draw_count: 0,
            boards: [Board::empty(); MAX_PLAYERS],
        }
    }

    /// How many dominoes remain in the draw pile.
    pub fn deck_remaining(&self) -> u32 {
        self.remaining.count_ones()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_copy<T: Copy>() {}

    #[test]
    fn gamestate_is_copy_and_bounded() {
        // The #1 speed lever (CLAUDE §3): cheap cloning. Guard it.
        assert_copy::<GameState>();
        let sz = std::mem::size_of::<GameState>();
        // Kingdomino state is tiny (the boards dominate); tripwire well above it.
        assert!(sz <= 4096, "GameState unexpectedly large: {sz} bytes");
    }

    #[test]
    fn cell_packing_roundtrips() {
        assert!(Cell::EMPTY.is_empty());
        assert!(Cell::CASTLE.is_castle());
        assert_eq!(Cell::CASTLE.terrain_of(), None);
        for t in Terrain::ALL {
            for crowns in 0..=3 {
                let c = Cell::terrain(t, crowns);
                assert!(!c.is_empty() && !c.is_castle());
                assert_eq!(c.terrain_of(), Some(t));
                assert_eq!(c.crowns(), crowns);
            }
        }
    }

    #[test]
    fn board_castle_centered_and_placement_updates_bbox() {
        let mut b = Board::with_castle();
        assert!(b.present);
        assert!(b.cell(CENTER, CENTER).is_castle());
        assert_eq!(
            (b.min_r, b.max_r, b.min_c, b.max_c),
            (CENTER, CENTER, CENTER, CENTER)
        );
        assert_eq!(b.filled, 0);

        // Place a square just left of the castle.
        b.place_square(CENTER, CENTER - 1, Terrain::Wheat, 1);
        assert_eq!(
            b.cell(CENTER, CENTER - 1).terrain_of(),
            Some(Terrain::Wheat)
        );
        assert_eq!(b.cell(CENTER, CENTER - 1).crowns(), 1);
        assert_eq!(b.filled, 1);
        assert_eq!((b.min_c, b.max_c), (CENTER - 1, CENTER));
    }

    #[test]
    fn off_center_castle_kingdom_is_legal() {
        // Build a fully one-sided kingdom: all squares to the RIGHT of the castle, up to the
        // 7-wide bound. The castle ends at the left edge of the bbox (not centered) — this is
        // a legal kingdom; it just won't earn the Middle Kingdom bonus. Centering is never a
        // placement constraint, only a scoring bonus.
        let mut b = Board::with_castle();
        for k in 1..GRID as u8 {
            // each extends the bbox one more column to the right; stays within the bound
            assert!(
                b.fits_bound(&[(CENTER, CENTER + k)]),
                "extend right by {k} should fit"
            );
            b.place_square(CENTER, CENTER + k, Terrain::Forest, 0);
        }
        // bbox spans the full 7 columns, castle is at the left edge (min_c), not the middle.
        assert_eq!(b.max_c - b.min_c, GRID as u8 - 1);
        assert_eq!(b.min_c, CENTER); // castle column == left edge → off-center, and that's fine
                                     // One more column would exceed the 7×7 bound and is correctly rejected.
        assert!(!b.fits_bound(&[(CENTER, CENTER + GRID as u8)]));
    }

    #[test]
    fn fits_bound_enforces_grid_span() {
        let b = Board::with_castle();
        // Castle at CENTER; a cell GRID-1 away keeps span == GRID-1 (< GRID) -> ok.
        assert!(b.fits_bound(&[(CENTER, CENTER + (GRID as u8 - 1))]));
        // One further would make span == GRID -> not ok.
        assert!(!b.fits_bound(&[(CENTER, CENTER + GRID as u8)]));
    }
}
