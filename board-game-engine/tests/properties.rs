//! Property tests (`docs/engine-design.md` §9 chunk 5): fuzz whole games and assert the
//! engine's invariants hold at every node and at terminal. Each case is driven by a generated
//! vector of "selectors" that pick among the legal actions / chance outcomes, so a failure
//! shrinks to a minimal action sequence.
//!
//! Several loops index parallel per-seat arrays by a shared seat index, which reads more
//! clearly than zipped iterators here — allow the lint for this test file.
#![allow(clippy::needless_range_loop)]

use kingdomino_engine::components::NUM_DOMINOES;
use kingdomino_engine::core::{
    apply_action, chance_outcomes, current_decision, legal_actions, new_game, terminal_value,
    Action, Decision, Phase, CENTER, GRID, MAX_PLAYERS,
};
use proptest::prelude::*;

/// Per-step invariants that must hold at every node of any reachable state.
fn check_state_invariants(gs: &kingdomino_engine::core::GameState) {
    for seat in 0..gs.player_count as usize {
        let b = &gs.boards[seat];
        assert!(b.present, "live seat {seat} must be present");
        // The castle is never overwritten or moved.
        assert!(
            b.cell(CENTER, CENTER).is_castle(),
            "castle intact (seat {seat})"
        );
        // The kingdom never exceeds the GRID×GRID bound.
        assert!(b.max_r - b.min_r < GRID as u8, "row bound (seat {seat})");
        assert!(b.max_c - b.min_c < GRID as u8, "col bound (seat {seat})");
        // Each placed domino adds exactly two squares, so `filled` is always even & bounded.
        assert!(b.filled % 2 == 0, "filled even (seat {seat})");
        assert!(
            (b.filled as usize) < GRID * GRID,
            "filled bounded (seat {seat})"
        );
    }
}

/// Drive one full game using `sels` to resolve every choice; assert invariants throughout
/// and conservation/value well-formedness at the end.
fn run_game(players: u8, sels: &[u32]) {
    let mut gs = new_game(players);
    let mut buf: Vec<Action> = Vec::new();
    let mut placed = [0u32; MAX_PLAYERS];
    let mut discarded = [0u32; MAX_PLAYERS];
    let mut idx = 0usize;
    let next_sel = |idx: &mut usize| {
        let v = sels.get(*idx).copied().unwrap_or(0);
        *idx += 1;
        v
    };

    let mut prev_filled = [0u16; MAX_PLAYERS];
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(steps < 100_000, "game failed to terminate");
        check_state_invariants(&gs);
        // `filled` is monotonic non-decreasing.
        for seat in 0..players as usize {
            assert!(
                gs.boards[seat].filled >= prev_filled[seat],
                "filled monotonic"
            );
            prev_filled[seat] = gs.boards[seat].filled;
        }

        match current_decision(&gs) {
            Decision::Terminal => break,
            Decision::Chance => {
                let outs = chance_outcomes(&gs);
                assert!(!outs.is_empty(), "chance node has outcomes");
                let psum: f32 = outs.iter().map(|(_, p)| *p).sum();
                assert!(
                    (psum - 1.0).abs() < 1e-3,
                    "chance probabilities sum to 1 (got {psum})"
                );
                let pick = outs[next_sel(&mut idx) as usize % outs.len()].0;
                apply_action(&mut gs, pick);
            }
            Decision::Player(p) => {
                legal_actions(&gs, &mut buf);
                assert!(!buf.is_empty(), "player node has at least one legal action");
                let pick = buf[next_sel(&mut idx) as usize % buf.len()];
                match (gs.phase, pick) {
                    (Phase::Place, Action::Place { .. }) => placed[p as usize] += 1,
                    (Phase::Place, Action::Discard) => discarded[p as usize] += 1,
                    _ => {}
                }
                apply_action(&mut gs, pick);
            }
        }
    }

    // --- terminal conservation + value ---
    assert_eq!(gs.deck_remaining(), 0, "deck fully drawn at terminal");
    let per_seat = NUM_DOMINOES as u32 / players as u32; // 24 for 2p, 12 for 4p
    for seat in 0..players as usize {
        assert_eq!(
            placed[seat] + discarded[seat],
            per_seat,
            "every claimed domino is placed or discarded (seat {seat})"
        );
        assert_eq!(
            gs.boards[seat].filled as u32,
            2 * placed[seat],
            "two squares per placed domino (seat {seat})"
        );
    }

    let v = terminal_value(&gs).expect("terminal state has a value");
    let sum: f32 = v.iter().sum();
    assert!(
        (sum - 1.0).abs() < 1e-5,
        "value vector sums to 1 (got {sum})"
    );
    for (seat, &x) in v.iter().enumerate() {
        assert!(
            (0.0..=1.0).contains(&x),
            "value in [0,1] (seat {seat}: {x})"
        );
    }
    assert!(
        terminal_value(&new_game(players)).is_none(),
        "non-terminal has no value"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    /// Any sequence of legal choices yields a game that respects every invariant and
    /// terminates with full domino conservation and a well-formed value vector.
    #[test]
    fn random_games_preserve_invariants(
        players in prop_oneof![Just(2u8), Just(4u8)],
        sels in prop::collection::vec(any::<u32>(), 0..1500),
    ) {
        run_game(players, &sels);
    }
}
