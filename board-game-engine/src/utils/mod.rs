//! Helpers: serialization, RNG, logging.
//!
//! Determinism rule (CLAUDE §7): all randomness flows through an injected, seedable RNG
//! (`rand_chacha`) so games replay exactly from a seed. The RNG is owned by the
//! search/self-play driver and passed in at chance nodes — never stored in `GameState`.

// Coming as needed:
// pub mod public;   // public-information view for the PyO3 bridge / UI (mirrors SpaceBase)
