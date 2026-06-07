//! The six Kingdomino terrain types (rulebook p.1).
//!
//! A domino square has a [`Terrain`] and a crown count (0..=3); a territory is a set of
//! orthogonally-connected squares of the *same* terrain (see `docs/engine-design.md` §7).
//! The castle / starting tile is **not** a terrain — its sides are wild — so it is
//! represented outside this enum (a board-cell tag), not as a `Terrain` variant.

/// Number of distinct terrain types.
pub const NUM_TERRAINS: usize = 6;

/// One of the six terrains a domino square can show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Terrain {
    Wheat = 0,
    Forest = 1,
    Lake = 2,
    Grassland = 3,
    Swamp = 4,
    Mine = 5,
}

impl Terrain {
    /// All terrains, in `repr` order. Useful for iteration in tests and ingest.
    pub const ALL: [Terrain; NUM_TERRAINS] = [
        Terrain::Wheat,
        Terrain::Forest,
        Terrain::Lake,
        Terrain::Grassland,
        Terrain::Swamp,
        Terrain::Mine,
    ];

    /// The terrain's discriminant as a `u8` (its cell-packing code is this `+ 1`, leaving
    /// `0` for "empty" — see `docs/engine-design.md` §3.1).
    pub fn index(self) -> u8 {
        self as u8
    }

    /// Inverse of [`index`](Self::index): map `0..NUM_TERRAINS` back to a terrain, or `None`.
    pub fn from_index(i: u8) -> Option<Terrain> {
        Terrain::ALL.get(i as usize).copied()
    }

    /// Parse a BoardGameArena terrain name (the vocabulary in `docs/bga/`). BGA calls the
    /// mine terrain `"mountain"`; everything else matches the rulebook. Returns `None` for
    /// an unknown name so ingest can flag bad data rather than silently mis-map it.
    pub fn from_bga(name: &str) -> Option<Terrain> {
        Some(match name {
            "field" => Terrain::Wheat, // BGA "field" == rulebook "wheat field"
            "forest" => Terrain::Forest,
            "lake" => Terrain::Lake,
            "grassland" => Terrain::Grassland,
            "swamp" => Terrain::Swamp,
            "mountain" => Terrain::Mine, // BGA "mountain" == rulebook "mines"
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_terrains_present_and_ordered() {
        assert_eq!(Terrain::ALL.len(), NUM_TERRAINS);
        for (i, t) in Terrain::ALL.iter().enumerate() {
            assert_eq!(t.index() as usize, i);
        }
    }
}
