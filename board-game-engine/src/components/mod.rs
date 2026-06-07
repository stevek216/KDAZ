//! Game components: terrains, dominoes, and the kingdom board.
//!
//! Domino *definitions* are a read-only static table (see `docs/engine-design.md` §2);
//! [`crate::core::state`] references them by id and stays cheaply `Copy`.

pub mod domino;
pub mod terrain;

pub use domino::{domino, DominoDef, DominoId, Square, DOMINOES, NO_DOMINO, NUM_DOMINOES};
pub use terrain::{Terrain, NUM_TERRAINS};

// Coming in the build-order chunks (docs/engine-design.md §9):
// pub mod board;    // the centered kingdom grid + bounding box (chunk 2)
