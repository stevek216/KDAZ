//! Rule engine: legal action generation, scoring, and win conditions.
//!
//! Pure functions over [`crate::core`] state. The placement-legality and scoring logic
//! (territory flood-fill + Harmony / Middle Kingdom bonuses) live here. See
//! `docs/engine-design.md` §5 and §7.

// Coming in the build-order chunks (docs/engine-design.md §9):
// pub mod place;    // placement legality: empty/in-bounds + 7×7 bound + connection (chunk 3)
// pub mod score;    // territory flood-fill, variant bonuses, tie-break ranking (chunk 4)
