//! Game setup: build a fresh `GameState` at its first node.
//!
//! Because the draw is an explicit chance node and the RNG is owned by the search/self-play
//! driver (CLAUDE §3, `docs/engine-design.md` §1), `new_game` does **not** perform any draws
//! itself. It places each live seat's castle, loads the full 48-domino deck, and positions
//! the game at the first `Phase::Draw` chance node (filling the starting line). The turn
//! loop (chunk 3) drives draws → starting claims → play rounds from there.

use crate::core::state::{Board, GameState, Phase, FULL_DECK, MAX_PLAYERS};

/// Create a new game for `player_count` seats (2..=MAX_PLAYERS). The Mighty Duel target uses
/// 2. Panics on an out-of-range count — that is a caller bug, not valid input.
pub fn new_game(player_count: u8) -> GameState {
    assert!(
        (2..=MAX_PLAYERS as u8).contains(&player_count),
        "player_count must be 2..={MAX_PLAYERS}"
    );
    let mut gs = GameState::blank();
    gs.player_count = player_count;
    for seat in 0..MAX_PLAYERS {
        gs.boards[seat] = if (seat as u8) < player_count {
            Board::with_castle()
        } else {
            Board::empty()
        };
    }
    gs.remaining = FULL_DECK;
    gs.round = 0;
    gs.phase = Phase::Draw; // first chance node: fill the starting line
    gs.to_act = 0;
    gs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::CENTER;

    #[test]
    fn new_game_two_player_initial_state() {
        let gs = new_game(2);
        assert_eq!(gs.player_count, 2);
        assert_eq!(gs.phase, Phase::Draw);
        assert_eq!(gs.round, 0);
        assert_eq!(gs.deck_remaining(), 48);
        // Both live seats have a centered castle; the rest are absent.
        assert!(gs.boards[0].present && gs.boards[1].present);
        assert!(!gs.boards[2].present && !gs.boards[3].present);
        for seat in 0..2 {
            assert!(gs.boards[seat].cell(CENTER, CENTER).is_castle());
            assert_eq!(gs.boards[seat].filled, 0);
        }
        // No dominoes drawn or claimed yet.
        assert_eq!(gs.draw_count, 0);
        assert!(gs.current_line.iter().all(|s| !s.is_filled()));
        assert!(gs.next_line.iter().all(|s| !s.is_filled()));
    }

    #[test]
    #[should_panic]
    fn rejects_too_many_players() {
        new_game(MAX_PLAYERS as u8 + 1);
    }
}
