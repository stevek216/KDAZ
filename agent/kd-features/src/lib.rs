//! Shared agent-side logic for Kingdomino: the canonical **feature encoder** (`GameState` →
//! network tensors) and the public **observation / legal-action JSON**. Used by both the PyO3
//! bridge (`engine-bridge`) and the fully-Rust self-play binary (`selfplay-rs`), so there is
//! ONE encoder (parity-critical — must match what the net was trained on) and ONE corpus
//! schema. The pure engine remains the single source of rules truth; this is agent logic.

pub mod encoder;
pub mod serialize;
