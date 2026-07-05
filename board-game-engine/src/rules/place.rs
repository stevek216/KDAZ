//! Placement legality and legal-placement enumeration (`docs/engine-design.md` §5).
//!
//! A domino's square `a` goes at an anchor cell, square `b` at the `rot`-neighbor. A placement
//! is legal iff: both cells are empty and in the store, the whole kingdom still fits a
//! `GRID×GRID` box, and at least one new square is orthogonally adjacent to the **castle**
//! (wild) or to an existing square of the **same terrain**.

use crate::components::{DominoDef, Terrain};
use crate::core::action::Action;
use crate::core::state::{Board, STORE};

/// Orthogonal directions, indexed by `rot`: 0=up, 1=right, 2=down, 3=left.
pub const DIRS: [(i8, i8); 4] = [(-1, 0), (0, 1), (1, 0), (0, -1)];

/// The neighbor of backing-store cell `(r, c)` in direction `rot`, or `None` if off-grid.
fn neighbor(r: u8, c: u8, rot: u8) -> Option<(u8, u8)> {
    let (dr, dc) = DIRS[rot as usize];
    let nr = r as i8 + dr;
    let nc = c as i8 + dc;
    if nr < 0 || nc < 0 || nr >= STORE as i8 || nc >= STORE as i8 {
        None
    } else {
        Some((nr as u8, nc as u8))
    }
}

/// Encode a backing-store cell as a `Place.anchor` value.
pub fn anchor_of(r: u8, c: u8) -> u16 {
    r as u16 * STORE as u16 + c as u16
}

/// Decode a `Place.anchor` value back to `(row, col)`.
pub fn cell_of(anchor: u16) -> (u8, u8) {
    ((anchor / STORE as u16) as u8, (anchor % STORE as u16) as u8)
}

/// Does the square at `(r, c)` with terrain `t` have a valid connection — an orthogonal
/// neighbor (other than its own domino partner `skip`) that is the castle or the same terrain?
fn square_connects(board: &Board, r: u8, c: u8, t: Terrain, skip: (u8, u8)) -> bool {
    for rot in 0..4u8 {
        if let Some((nr, nc)) = neighbor(r, c, rot) {
            if (nr, nc) == skip {
                continue; // the domino's other half is not a "connection" to the kingdom
            }
            let cell = board.cell(nr, nc);
            if cell.is_castle() {
                return true; // castle sides are wild
            }
            if cell.terrain_of() == Some(t) {
                return true;
            }
        }
    }
    false
}

/// Is placing `def` with square `a` at `(r, c)` and square `b` toward `rot` legal?
pub fn placement_legal(board: &Board, def: &DominoDef, r: u8, c: u8, rot: u8) -> bool {
    if r as usize >= STORE || c as usize >= STORE {
        return false; // anchor off the backing store
    }
    let b = match neighbor(r, c, rot) {
        Some(b) => b,
        None => return false,
    };
    if !board.cell(r, c).is_empty() || !board.cell(b.0, b.1).is_empty() {
        return false; // both halves must land on empty cells
    }
    if !board.fits_bound(&[(r, c), b]) {
        return false; // 7×7 bound
    }
    // At least one half connects (castle-wild or same-terrain), which also forces contiguity.
    square_connects(board, r, c, def.a.terrain, b)
        || square_connects(board, b.0, b.1, def.b.terrain, (r, c))
}

/// The anchor scan window: the occupied bounding box expanded by 2, clamped to the store.
/// A legal placement has a square adjacent to an occupied cell (bbox+1) and its partner one
/// step further (bbox+2), so every legal anchor lies inside this window. The castle is always
/// occupied, so the bbox is never empty.
fn anchor_window(board: &Board) -> (u8, u8, u8, u8) {
    let hi = STORE as u8 - 1;
    (
        board.min_r.saturating_sub(2),
        (board.max_r + 2).min(hi),
        board.min_c.saturating_sub(2),
        (board.max_c + 2).min(hi),
    )
}

/// A "double" domino (both squares identical): `(anchor, rot)` and `(rot-neighbor, rot+2)`
/// produce byte-identical boards, so only rots 1 (right) and 2 (down) are enumerated —
/// one canonical representation per physical placement (anchor = the top/left cell).
fn rot_range(def: &DominoDef) -> std::ops::Range<u8> {
    if def.a.terrain == def.b.terrain && def.a.crowns == def.b.crowns {
        1..3
    } else {
        0..4
    }
}

/// Append every legal placement of `def` on `board` to `out` as `Action::Place`.
/// Double dominoes emit one action per physical placement (see `rot_range`).
pub fn legal_placements(board: &Board, def: &DominoDef, out: &mut Vec<Action>) {
    let (r0, r1, c0, c1) = anchor_window(board);
    for r in r0..=r1 {
        for c in c0..=c1 {
            for rot in rot_range(def) {
                if placement_legal(board, def, r, c, rot) {
                    out.push(Action::Place {
                        anchor: anchor_of(r, c),
                        rot,
                    });
                }
            }
        }
    }
}

/// Whether `def` has any legal placement on `board` (drives the discard rule).
pub fn has_any_placement(board: &Board, def: &DominoDef) -> bool {
    let (r0, r1, c0, c1) = anchor_window(board);
    for r in r0..=r1 {
        for c in c0..=c1 {
            for rot in rot_range(def) {
                if placement_legal(board, def, r, c, rot) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Square;
    use crate::core::state::{Board, CENTER, GRID};

    fn def(at: Terrain, ac: u8, bt: Terrain, bc: u8) -> DominoDef {
        DominoDef {
            number: 0,
            a: Square {
                terrain: at,
                crowns: ac,
            },
            b: Square {
                terrain: bt,
                crowns: bc,
            },
        }
    }

    #[test]
    fn first_domino_connects_to_wild_castle() {
        let board = Board::with_castle();
        let d = def(Terrain::Wheat, 0, Terrain::Forest, 0);
        // a just left of castle, b further left: a touches the castle -> legal.
        assert!(placement_legal(&board, &d, CENTER, CENTER - 1, 3));
        // some legal placement exists around the castle.
        assert!(has_any_placement(&board, &d));
    }

    #[test]
    fn disconnected_placement_is_illegal() {
        let board = Board::with_castle();
        let d = def(Terrain::Wheat, 0, Terrain::Forest, 0);
        // Far corner of the store, nowhere near the castle -> no connection.
        assert!(!placement_legal(&board, &d, 0, 0, 1));
    }

    #[test]
    fn connection_requires_matching_terrain() {
        let mut board = Board::with_castle();
        // Put a wheat square left of the castle: (CENTER, CENTER-1).
        board.place_square(CENTER, CENTER - 1, Terrain::Wheat, 0);
        // A forest|forest domino touching ONLY that wheat square (not the castle) is illegal.
        // Place a at (CENTER-1, CENTER-1) [above the wheat], b above it (rot up). a's only
        // kingdom neighbor is the wheat below — forest != wheat, castle not adjacent -> illegal.
        let forest = def(Terrain::Forest, 0, Terrain::Forest, 0);
        assert!(!placement_legal(&board, &forest, CENTER - 1, CENTER - 1, 0));
        // A wheat|x domino in the same spot DOES connect (wheat matches the wheat below).
        let wheat = def(Terrain::Wheat, 0, Terrain::Forest, 0);
        assert!(placement_legal(&board, &wheat, CENTER - 1, CENTER - 1, 0));
    }

    /// Brute-force reference: every `(r, c, rot)` over the whole store (the pre-window,
    /// pre-dedup enumeration). Doubles keep all 4 rots here, so it contains each physical
    /// placement of a double twice.
    fn brute_force(board: &Board, d: &DominoDef) -> Vec<(u8, u8, u8)> {
        let mut v = Vec::new();
        for r in 0..STORE as u8 {
            for c in 0..STORE as u8 {
                for rot in 0..4u8 {
                    if placement_legal(board, d, r, c, rot) {
                        v.push((r, c, rot));
                    }
                }
            }
        }
        v
    }

    /// Map a placement to its canonical physical form: the sorted pair of occupied cells.
    fn cells_of(r: u8, c: u8, rot: u8) -> [(u8, u8); 2] {
        let (nr, nc) = super::neighbor(r, c, rot).unwrap();
        let mut cells = [(r, c), (nr, nc)];
        cells.sort();
        cells
    }

    #[test]
    fn windowed_scan_matches_brute_force() {
        // Fresh castle board and a lopsided kingdom: the windowed, rot-deduped enumeration
        // must cover exactly the brute-force physical placement set.
        let mut board = Board::with_castle();
        board.place_square(CENTER, CENTER - 1, Terrain::Wheat, 0);
        board.place_square(CENTER - 1, CENTER - 1, Terrain::Forest, 1);
        board.place_square(CENTER - 1, CENTER, Terrain::Forest, 0);
        for d in [
            def(Terrain::Wheat, 0, Terrain::Forest, 0), // mixed: all 4 rots
            def(Terrain::Forest, 0, Terrain::Forest, 0), // double: rots 1,2 only
        ] {
            let mut out = Vec::new();
            legal_placements(&board, &d, &mut out);
            let got: std::collections::BTreeSet<_> = out
                .iter()
                .map(|a| match a {
                    Action::Place { anchor, rot } => {
                        let (r, c) = cell_of(*anchor);
                        (
                            cells_of(r, c, *rot),
                            d.a.terrain != d.b.terrain && *rot >= 2,
                        )
                    }
                    _ => unreachable!(),
                })
                .collect();
            let want: std::collections::BTreeSet<_> = brute_force(&board, &d)
                .iter()
                .map(|&(r, c, rot)| (cells_of(r, c, rot), d.a.terrain != d.b.terrain && rot >= 2))
                .collect();
            assert_eq!(got, want, "windowed/deduped scan diverged for {:?}", d);
            assert_eq!(has_any_placement(&board, &d), !out.is_empty());
        }
    }

    #[test]
    fn double_domino_emits_each_physical_placement_once() {
        let board = Board::with_castle();
        let double = def(Terrain::Wheat, 0, Terrain::Wheat, 0);
        let mixed = def(Terrain::Wheat, 0, Terrain::Forest, 0);
        let (mut out_d, mut out_m) = (Vec::new(), Vec::new());
        legal_placements(&board, &double, &mut out_d);
        legal_placements(&board, &mixed, &mut out_m);
        // Doubles: every physical placement exactly once (unique cell-pairs), rots only 1|2.
        let phys: std::collections::BTreeSet<_> = out_d
            .iter()
            .map(|a| match a {
                Action::Place { anchor, rot } => {
                    assert!(*rot == 1 || *rot == 2, "double emitted rot {rot}");
                    let (r, c) = cell_of(*anchor);
                    cells_of(r, c, *rot)
                }
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            phys.len(),
            out_d.len(),
            "double emitted a duplicate placement"
        );
        // A mixed domino on the same board has exactly twice as many actions (both
        // orientations of each cell-pair are distinct placements for it).
        assert_eq!(out_m.len(), 2 * out_d.len());
    }

    #[test]
    fn bound_rejects_overwide_kingdom() {
        let mut board = Board::with_castle();
        // Fill columns CENTER+1..=CENTER+(GRID-1) to the right of the castle: now the kingdom
        // spans the full 7 columns (castle col .. castle+6).
        for k in 1..GRID as u8 {
            board.place_square(CENTER, CENTER + k, Terrain::Wheat, 0);
        }
        let d = def(Terrain::Wheat, 0, Terrain::Wheat, 0);
        // A square one column LEFT of the castle is in-store and connects (touches the castle),
        // but would make the span 8 columns -> rejected by the 7×7 bound (not by connection).
        assert!(!placement_legal(&board, &d, CENTER, CENTER - 1, 0)); // a=(CENTER,CENTER-1), b above
                                                                      // Sanity: on a fresh board that same placement IS legal (so it's the bound rejecting it).
        let fresh = Board::with_castle();
        assert!(placement_legal(&fresh, &d, CENTER, CENTER - 1, 0));
    }
}
