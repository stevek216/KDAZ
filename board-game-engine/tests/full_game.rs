//! End-to-end: a full Kingdomino game driven to terminal through the public engine API.
//! Cross-module behavior (setup + turn loop + placement rules) — see `docs/engine-design.md` §9
//! chunk 5. Unit-level invariants live beside the code; this file checks whole-game properties.

use kingdomino_engine::core::{
    apply_action, apply_chance, current_decision, legal_actions, new_game, Decision, GameState,
    GRID,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Play a uniformly-random game to terminal and return the final state.
fn play_random(players: u8, seed: u64) -> GameState {
    let mut gs = new_game(players);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut buf = Vec::new();
    let mut steps = 0;
    loop {
        steps += 1;
        assert!(
            steps < 100_000,
            "non-termination (players {players}, seed {seed})"
        );
        match current_decision(&gs) {
            Decision::Terminal => return gs,
            Decision::Chance => {
                apply_chance(&mut gs, &mut rng);
            }
            Decision::Player(_) => {
                legal_actions(&gs, &mut buf);
                assert!(!buf.is_empty());
                let pick = buf[rng.gen_range(0..buf.len())];
                apply_action(&mut gs, pick);
            }
        }
    }
}

#[test]
fn same_seed_is_fully_deterministic() {
    // The engine's only randomness is the injected RNG at chance nodes; identical seeds must
    // reproduce the game exactly (CLAUDE §7 determinism).
    for seed in 0..10 {
        let a = play_random(2, seed);
        let b = play_random(2, seed);
        assert_eq!(a, b, "seed {seed} not reproducible");
    }
}

#[test]
fn four_player_game_runs_to_terminal() {
    // Exercises the 4-player path: 1 king each, 24 distinct starting orders, 12 dominoes/seat.
    for seed in 0..10u64 {
        let gs = play_random(4, seed);
        assert_eq!(gs.deck_remaining(), 0, "seed {seed}");
        let mut total_squares = 0u32;
        for p in 0..4 {
            let b = &gs.boards[p];
            assert!(b.present);
            assert_eq!(
                b.filled % 2,
                0,
                "filled must be even (seed {seed}, seat {p})"
            );
            assert!(b.max_r - b.min_r < GRID as u8 && b.max_c - b.min_c < GRID as u8);
            total_squares += b.filled as u32;
        }
        // Every placed domino contributes 2 squares; the rest were discarded. Each seat drew
        // 12 dominoes (12 lines × 1 king) → 48 total, so placed+discarded across seats == 48.
        assert!(total_squares <= 96 && total_squares % 2 == 0, "seed {seed}");
    }
}
