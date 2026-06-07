//! Kingdomino game engine.
//!
//! A fast, deterministic implementation of the board game Kingdomino (targeting the
//! Mighty Duel 7×7 variant plus the Harmony and Middle Kingdom scoring variants), built
//! as the foundation for an AlphaZero-style self-play agent.
//!
//! See `docs/engine-design.md` for the `GameState` / action / scoring spec and
//! `../CLAUDE.md` for the durable project charter.
//!
//! # Module layout
//!
//! - [`core`] — game state, the turn loop, and action application.
//! - [`components`] — game pieces: terrains, dominoes, the kingdom board.
//! - [`rules`] — legal action generation, scoring, and win conditions.
//! - [`utils`] — helpers: serialization, RNG, logging.

pub mod components;
pub mod core;
pub mod rules;
pub mod utils;
