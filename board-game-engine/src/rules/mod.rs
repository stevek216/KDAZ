//! Rule engine: legal action generation, scoring, and win conditions.
//!
//! Pure functions over [`crate::core`] state. The placement-legality and scoring logic
//! (territory flood-fill + Harmony / Middle Kingdom bonuses) live here. See
//! `docs/engine-design.md` §5 and §7.

pub mod place;
pub mod score;

pub use place::{anchor_of, cell_of, has_any_placement, legal_placements, placement_legal};
pub use score::{score_board, ScoreBreakdown};
