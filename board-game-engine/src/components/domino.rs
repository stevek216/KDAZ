//! The 48 base-game dominoes — static, authoritative tile data.
//!
//! Transcribed from `docs/bga/kingdomino_dominoes_bga.json` (captured from BoardGameArena;
//! see `docs/engine-design.md` §2 and CLAUDE §6 — **never fabricate this data**). The table
//! is `const` (all literals), so it costs nothing at runtime and `GameState` references a
//! domino only by its [`DominoId`]. The [`tests`] module re-derives the published terrain
//! and crown tallies and fails if a transcription error slips in.

use crate::components::terrain::Terrain;

/// Number of base-game dominoes.
pub const NUM_DOMINOES: usize = 48;

/// Index into [`DOMINOES`]. `DominoId == number - 1`, so the draft line (sorted ascending
/// by number) is sorted ascending by id (`docs/engine-design.md` §2).
pub type DominoId = u8;

/// NONE sentinel for an empty domino slot.
pub const NO_DOMINO: DominoId = u8::MAX;

/// One terrain square of a domino: a terrain and its crown count (0..=3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Square {
    pub terrain: Terrain,
    pub crowns: u8,
}

/// A domino definition: its draft-order number and its two terrain squares. `a` is placed
/// at the chosen anchor cell and `b` toward the chosen rotation (`docs/engine-design.md`
/// §4.2). `a`/`b` correspond to BGA's `left`/`right`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DominoDef {
    pub number: u8,
    pub a: Square,
    pub b: Square,
}

/// Look up a domino definition by id (`0..NUM_DOMINOES`).
pub fn domino(id: DominoId) -> &'static DominoDef {
    &DOMINOES[id as usize]
}

const fn sq(terrain: Terrain, crowns: u8) -> Square {
    Square { terrain, crowns }
}

const fn d(number: u8, a: Square, b: Square) -> DominoDef {
    DominoDef { number, a, b }
}

use Terrain::{Forest, Grassland, Lake, Mine, Swamp, Wheat};

/// The authoritative 48-domino table (id `0..47` ↔ number `1..48`). Source of truth:
/// `docs/bga/kingdomino_dominoes_bga.json`.
pub static DOMINOES: [DominoDef; NUM_DOMINOES] = [
    d(1, sq(Wheat, 0), sq(Wheat, 0)),
    d(2, sq(Wheat, 0), sq(Wheat, 0)),
    d(3, sq(Forest, 0), sq(Forest, 0)),
    d(4, sq(Forest, 0), sq(Forest, 0)),
    d(5, sq(Forest, 0), sq(Forest, 0)),
    d(6, sq(Forest, 0), sq(Forest, 0)),
    d(7, sq(Lake, 0), sq(Lake, 0)),
    d(8, sq(Lake, 0), sq(Lake, 0)),
    d(9, sq(Lake, 0), sq(Lake, 0)),
    d(10, sq(Grassland, 0), sq(Grassland, 0)),
    d(11, sq(Grassland, 0), sq(Grassland, 0)),
    d(12, sq(Swamp, 0), sq(Swamp, 0)),
    d(13, sq(Wheat, 0), sq(Forest, 0)),
    d(14, sq(Wheat, 0), sq(Lake, 0)),
    d(15, sq(Wheat, 0), sq(Grassland, 0)),
    d(16, sq(Wheat, 0), sq(Swamp, 0)),
    d(17, sq(Forest, 0), sq(Lake, 0)),
    d(18, sq(Forest, 0), sq(Grassland, 0)),
    d(19, sq(Wheat, 1), sq(Forest, 0)),
    d(20, sq(Wheat, 1), sq(Lake, 0)),
    d(21, sq(Wheat, 1), sq(Grassland, 0)),
    d(22, sq(Wheat, 1), sq(Swamp, 0)),
    d(23, sq(Wheat, 1), sq(Mine, 0)),
    d(24, sq(Forest, 1), sq(Wheat, 0)),
    d(25, sq(Forest, 1), sq(Wheat, 0)),
    d(26, sq(Forest, 1), sq(Wheat, 0)),
    d(27, sq(Forest, 1), sq(Wheat, 0)),
    d(28, sq(Forest, 1), sq(Lake, 0)),
    d(29, sq(Forest, 1), sq(Grassland, 0)),
    d(30, sq(Lake, 1), sq(Wheat, 0)),
    d(31, sq(Lake, 1), sq(Wheat, 0)),
    d(32, sq(Lake, 1), sq(Forest, 0)),
    d(33, sq(Lake, 1), sq(Forest, 0)),
    d(34, sq(Lake, 1), sq(Forest, 0)),
    d(35, sq(Lake, 1), sq(Forest, 0)),
    d(36, sq(Wheat, 0), sq(Grassland, 1)),
    d(37, sq(Lake, 0), sq(Grassland, 1)),
    d(38, sq(Wheat, 0), sq(Swamp, 1)),
    d(39, sq(Grassland, 0), sq(Swamp, 1)),
    d(40, sq(Mine, 1), sq(Wheat, 0)),
    d(41, sq(Wheat, 0), sq(Grassland, 2)),
    d(42, sq(Lake, 0), sq(Grassland, 2)),
    d(43, sq(Wheat, 0), sq(Swamp, 2)),
    d(44, sq(Grassland, 0), sq(Swamp, 2)),
    d(45, sq(Mine, 2), sq(Wheat, 0)),
    d(46, sq(Swamp, 0), sq(Mine, 2)),
    d(47, sq(Swamp, 0), sq(Mine, 2)),
    d(48, sq(Wheat, 0), sq(Mine, 3)),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::terrain::NUM_TERRAINS;

    #[test]
    fn table_has_48_entries_with_sequential_numbers() {
        assert_eq!(DOMINOES.len(), NUM_DOMINOES);
        for (i, def) in DOMINOES.iter().enumerate() {
            assert_eq!(
                def.number as usize,
                i + 1,
                "domino at id {i} has wrong number"
            );
        }
    }

    #[test]
    fn crowns_in_range() {
        for def in &DOMINOES {
            for s in [def.a, def.b] {
                assert!(
                    s.crowns <= 3,
                    "domino {} has crowns {} > 3",
                    def.number,
                    s.crowns
                );
            }
        }
    }

    /// Re-derive the published BGA tallies (docs/bga/README.md) so a transcription slip in
    /// the table above is caught immediately.
    #[test]
    fn terrain_and_crown_tallies_match_bga() {
        let mut squares = [0u32; NUM_TERRAINS];
        let mut crowns = [0u32; NUM_TERRAINS];
        for def in &DOMINOES {
            for s in [def.a, def.b] {
                squares[s.terrain.index() as usize] += 1;
                crowns[s.terrain.index() as usize] += s.crowns as u32;
            }
        }
        // [Wheat, Forest, Lake, Grassland, Swamp, Mine] order (Terrain repr).
        assert_eq!(squares, [26, 22, 18, 14, 10, 6], "square-per-terrain tally");
        assert_eq!(crowns, [5, 6, 6, 6, 6, 10], "crown-per-terrain tally");
        assert_eq!(squares.iter().sum::<u32>(), 96);
        assert_eq!(crowns.iter().sum::<u32>(), 39);
    }
}
