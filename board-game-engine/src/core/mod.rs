//! Core engine: game state, the turn loop, and action application.
//!
//! State is transformed functionally — apply an action to a state and get the next
//! state — with no global mutable state, so the engine is safe to run across many
//! self-play games in parallel. See `docs/engine-design.md` §1, §3–§6.

pub mod action;
pub mod setup;
pub mod state;
pub mod turn;

pub use action::{Action, Decision};
pub use setup::new_game;
pub use state::Variants;
pub use state::{
    Board, Cell, GameState, Phase, Slot, CENTER, GRID, LINE, MAX_PLAYERS, NO_OWNER, STORE,
};
pub use turn::{
    apply_action, apply_chance, chance_outcomes, current_decision, is_chance, is_terminal,
    legal_actions, terminal_value,
};
