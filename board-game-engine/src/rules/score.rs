//! End-of-game scoring (`docs/engine-design.md` §7).
//!
//! A **territory** is a maximal orthogonally-connected group of same-terrain squares (the
//! castle has no terrain and never joins one). Each territory scores `size × crowns`;
//! crownless territories score 0. Optional bonuses (additive, never constraints): **Harmony**
//! (+5 for a complete gap-free `GRID×GRID`) and **Middle Kingdom** (+10 for a centered castle).

use crate::core::state::{Board, Variants, CENTER, GRID, STORE};
use crate::rules::place::DIRS;

/// A board's scored breakdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ScoreBreakdown {
    /// Sum over territories of `size × crowns`.
    pub crown_score: u32,
    /// Harmony bonus (0 or 5).
    pub harmony: u32,
    /// Middle Kingdom bonus (0 or 10).
    pub middle_kingdom: u32,
    /// `crown_score + harmony + middle_kingdom`.
    pub total: u32,
    /// Size of the largest single territory (the tie-breaker, §7.1).
    pub largest_territory: u32,
}

/// Score `board` under `variants`.
pub fn score_board(board: &Board, variants: Variants) -> ScoreBreakdown {
    let (crown_score, largest_territory) = territories(board);
    let harmony = if variants.harmony && is_complete_grid(board) {
        5
    } else {
        0
    };
    let middle_kingdom = if variants.middle_kingdom && is_castle_centered(board) {
        10
    } else {
        0
    };
    ScoreBreakdown {
        crown_score,
        harmony,
        middle_kingdom,
        total: crown_score + harmony + middle_kingdom,
        largest_territory,
    }
}

/// Flood-fill every territory; return `(Σ size×crowns, largest territory size)`.
fn territories(board: &Board) -> (u32, u32) {
    let mut visited = [[false; STORE]; STORE];
    let mut crown_score = 0u32;
    let mut largest = 0u32;
    // Only the occupied bounding box can hold terrain squares.
    for r in board.min_r..=board.max_r {
        for c in board.min_c..=board.max_c {
            let terrain = match board.cell(r, c).terrain_of() {
                Some(t) => t,
                None => continue,
            };
            if visited[r as usize][c as usize] {
                continue;
            }
            // DFS this territory with a fixed-capacity stack (no heap on the scoring path).
            let mut stack = [(0u8, 0u8); STORE * STORE];
            let mut top = 0;
            stack[top] = (r, c);
            top += 1;
            visited[r as usize][c as usize] = true;
            let (mut size, mut crowns) = (0u32, 0u32);
            while top > 0 {
                top -= 1;
                let (cr, cc) = stack[top];
                let cell = board.cell(cr, cc);
                size += 1;
                crowns += cell.crowns() as u32;
                for (dr, dc) in DIRS {
                    let nr = cr as i8 + dr;
                    let nc = cc as i8 + dc;
                    if nr < 0 || nc < 0 || nr >= STORE as i8 || nc >= STORE as i8 {
                        continue;
                    }
                    let (nr, nc) = (nr as u8, nc as u8);
                    if !visited[nr as usize][nc as usize]
                        && board.cell(nr, nc).terrain_of() == Some(terrain)
                    {
                        visited[nr as usize][nc as usize] = true;
                        stack[top] = (nr, nc);
                        top += 1;
                    }
                }
            }
            crown_score += size * crowns;
            largest = largest.max(size);
        }
    }
    (crown_score, largest)
}

/// Harmony: the kingdom completely fills a `GRID×GRID` box (castle + every terrain cell, no
/// gaps). Since the bound caps the kingdom at `GRID×GRID = GRID²` cells, having `GRID²−1`
/// terrain squares (plus the castle) means a perfect, gap-free tiling.
fn is_complete_grid(board: &Board) -> bool {
    board.filled as usize == GRID * GRID - 1
}

/// Middle Kingdom: the castle (always stored at `CENTER`) sits exactly in the middle of the
/// occupied bounding box — i.e. the kingdom extends `GRID/2` in every direction from it (the
/// box is a full `GRID×GRID` with the castle dead center). Gaps inside are allowed.
fn is_castle_centered(board: &Board) -> bool {
    let half = (GRID / 2) as u8;
    board.min_r == CENTER - half
        && board.max_r == CENTER + half
        && board.min_c == CENTER - half
        && board.max_c == CENTER + half
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Terrain;
    use crate::core::state::{Board, CENTER, GRID};

    #[test]
    fn rulebook_territory_examples() {
        // Rulebook p.3: 7 forest squares with 3 crowns -> 21; 9 lake squares, 0 crowns -> 0.
        let mut b = Board::with_castle();
        // A 7-cell forest territory in a row, with 3 crowns spread across it.
        for k in 0..7u8 {
            let crowns = if k < 3 { 1 } else { 0 };
            b.place_square(CENTER - 1, (CENTER - 3) + k, Terrain::Forest, crowns);
        }
        // A disconnected 9-cell lake territory (two rows), 0 crowns.
        for k in 0..5u8 {
            b.place_square(CENTER + 1, (CENTER - 2) + k, Terrain::Lake, 0);
        }
        for k in 0..4u8 {
            b.place_square(CENTER + 2, (CENTER - 2) + k, Terrain::Lake, 0);
        }
        let s = score_board(&b, Variants::NONE);
        assert_eq!(
            s.crown_score, 21,
            "7 forest × 3 crowns = 21; lake (0 crowns) = 0"
        );
        assert_eq!(
            s.largest_territory, 9,
            "the 9-cell lake is the largest territory"
        );
        assert_eq!(s.total, 21);
    }

    #[test]
    fn same_terrain_unconnected_scores_separately() {
        let mut b = Board::with_castle();
        // Two separate single forest crowned squares, not adjacent -> two territories of size 1.
        b.place_square(CENTER, CENTER - 1, Terrain::Forest, 1); // touches castle (left)
        b.place_square(CENTER, CENTER + 1, Terrain::Forest, 1); // touches castle (right)
        let s = score_board(&b, Variants::NONE);
        // Each is size 1 × 1 crown = 1; total 2 (they don't merge across the castle).
        assert_eq!(s.crown_score, 2);
        assert_eq!(s.largest_territory, 1);
    }

    #[test]
    fn harmony_only_when_grid_is_full() {
        // Fill every non-castle cell of the 7×7 around the centered castle with wheat.
        let mut b = Board::with_castle();
        let half = (GRID / 2) as u8;
        for r in (CENTER - half)..=(CENTER + half) {
            for c in (CENTER - half)..=(CENTER + half) {
                if !(r == CENTER && c == CENTER) {
                    b.place_square(r, c, Terrain::Wheat, 0);
                }
            }
        }
        assert_eq!(b.filled as usize, GRID * GRID - 1);
        let s = score_board(&b, Variants::MIGHTY_DUEL);
        assert_eq!(s.harmony, 5);
        assert_eq!(s.middle_kingdom, 10); // a full grid is also centered
        assert_eq!(s.crown_score, 0); // one big crownless wheat territory
        assert_eq!(s.largest_territory, (GRID * GRID - 1) as u32);

        // Remove one cell (a gap) -> no Harmony, but the bbox is still centered -> Middle stays.
        let mut gappy = Board::with_castle();
        let half = (GRID / 2) as u8;
        for r in (CENTER - half)..=(CENTER + half) {
            for c in (CENTER - half)..=(CENTER + half) {
                let is_castle = r == CENTER && c == CENTER;
                let is_gap = r == CENTER - half && c == CENTER - half; // leave one corner empty
                if is_castle || is_gap {
                    continue;
                }
                gappy.place_square(r, c, Terrain::Wheat, 0);
            }
        }
        let s2 = score_board(&gappy, Variants::MIGHTY_DUEL);
        assert_eq!(s2.harmony, 0, "a gap forfeits Harmony");
        assert_eq!(s2.middle_kingdom, 10, "still centered despite the gap");
    }

    #[test]
    fn middle_kingdom_requires_centered_castle() {
        // One-sided kingdom: all squares to the right -> castle at the edge, not centered.
        let mut b = Board::with_castle();
        for k in 1..GRID as u8 {
            b.place_square(CENTER, CENTER + k, Terrain::Wheat, 0);
        }
        let s = score_board(&b, Variants::MIGHTY_DUEL);
        assert_eq!(
            s.middle_kingdom, 0,
            "off-center castle earns no Middle Kingdom"
        );
        assert_eq!(s.harmony, 0);
    }

    #[test]
    fn variants_off_disables_bonuses() {
        let mut b = Board::with_castle();
        let half = (GRID / 2) as u8;
        for r in (CENTER - half)..=(CENTER + half) {
            for c in (CENTER - half)..=(CENTER + half) {
                if !(r == CENTER && c == CENTER) {
                    b.place_square(r, c, Terrain::Wheat, 0);
                }
            }
        }
        let s = score_board(&b, Variants::NONE);
        assert_eq!(s.harmony, 0);
        assert_eq!(s.middle_kingdom, 0);
    }
}
