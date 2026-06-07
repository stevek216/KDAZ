//! The action space and decision kinds (`docs/engine-design.md` §1, §4.2).
//!
//! `Action` is the union of player choices (`Claim`, `Place`, `Discard`) and engine-generated
//! chance outcomes (`Draw`, `StartOrder`). `apply_action` is deterministic and accepts either;
//! `apply_chance` samples a chance `Action` via the injected RNG.

use crate::components::DominoId;

/// A move in the game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Claim the domino in line slot `slot` (the `current_line` during the starting round,
    /// the `next_line` during a play round) with the acting seat's king.
    Claim { slot: u8 },
    /// Place the current domino: square `a` at backing-store cell `anchor` (= `row*STORE + col`)
    /// and square `b` toward direction `rot` (0=up, 1=right, 2=down, 3=left).
    Place { anchor: u16, rot: u8 },
    /// Discard the current domino (no legal placement exists). Forced — never offered
    /// alongside a `Place` (`docs/engine-design.md` §5).
    Discard,
    /// CHANCE outcome: the next drawn domino is `domino`. Engine-generated, not a player pick.
    Draw { domino: DominoId },
    /// CHANCE outcome: the starting-round claim order is the `perm`-th distinct seat ordering
    /// (`docs/engine-design.md` §6.2).
    StartOrder { perm: u8 },
}

/// What kind of node the game is at — tells the search how to treat it (`docs/engine-design.md` §1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Seat `to_act` chooses among `legal_actions`.
    Player(u8),
    /// A hidden-draw / starting-order node — expand/sample via `chance_outcomes` / `apply_chance`.
    Chance,
    /// The game is over (`terminal_value`).
    Terminal,
}
