//! Core engine: game state, the turn loop, and action application.
//!
//! State is transformed functionally — apply an action to a state and get the next
//! state — with no global mutable state, so the engine is safe to run across many
//! self-play games in parallel. See `docs/engine-design.md` §1, §3–§6.

pub mod setup;
pub mod state;

pub use setup::new_game;
pub use state::{
    Board, Cell, GameState, Phase, Slot, CENTER, GRID, LINE, MAX_PLAYERS, NO_OWNER, STORE,
};

// Coming in the build-order chunks (docs/engine-design.md §9):
// pub mod action;   // Action / Decision enums (chunk 3)
// pub mod turn;     // current_decision / legal_actions / apply_action / chance / terminal (chunk 3–4)
