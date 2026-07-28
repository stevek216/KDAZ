//! Parity checks against the BoardGameArena implementation (`BGA Dump/kingdomino`).
//!
//! BGA's PHP backend is the reference for how the game is actually served. The functions
//! prefixed `bga_` below are direct transcriptions of its predicates (file/function cited on
//! each); the tests compare them against `rules::score` on boards produced by real games.
//!
//! Coordinate mapping: BGA's castle is `(0,0)` and ours is `(CENTER, CENTER)`, so a BGA
//! coordinate `x` corresponds to our store column `CENTER + x` (rows likewise). Symmetry and
//! span predicates are translation-invariant, so the mapping is exact.

use kingdomino_engine::components::Terrain;
use kingdomino_engine::core::action::{Action, Decision};
use kingdomino_engine::core::setup::new_game;
use kingdomino_engine::core::state::{Board, GameState, Phase, Variants, CENTER, GRID, STORE};
use kingdomino_engine::core::turn::{
    apply_action, apply_chance, current_decision, legal_actions, terminal_value,
};
use kingdomino_engine::rules::score::score_board;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

// ============================================================================
// BGA reference predicates (transcribed from kingdomino.game.php)
// ============================================================================

/// `finalScoring()`: the Middle Kingdom test. BGA seeds `minX/minY/maxX/maxY` at 0 (the
/// castle) and extends them over every occupied cell, then awards +10 iff
/// `-minX == maxX && -minY == maxY` — the occupied bounding box is **exactly symmetric**
/// about the castle. Note this is a symmetry test, not a "fits in a centered window" test.
fn bga_middle_kingdom(board: &Board) -> bool {
    let (mut min_r, mut max_r, mut min_c, mut max_c) = (CENTER, CENTER, CENTER, CENTER);
    for r in 0..STORE as u8 {
        for c in 0..STORE as u8 {
            if !board.cell(r, c).is_empty() {
                min_r = min_r.min(r);
                max_r = max_r.max(r);
                min_c = min_c.min(c);
                max_c = max_c.max(c);
            }
        }
    }
    (CENTER - min_r) == (max_r - CENTER) && (CENTER - min_c) == (max_c - CENTER)
}

/// `finalScoring()`: Harmony is awarded iff the player has **no discarded domino**
/// (`SELECT 1 FROM dominoes WHERE location = 'DISCARD' AND owner_player = ...` is empty),
/// rather than by inspecting the grid geometry.
fn bga_harmony(discards: u32) -> bool {
    discards == 0
}

/// `finalScoring()`: `player_score_aux = biggestTerritory * 100 + totalCrowns`, the
/// tiebreaker applied after `player_score`. `biggestTerritory` is the max territory size
/// (BGA counts the castle as its own size-1 territory); `totalCrowns` sums the crowns of
/// every scoring territory, which equals the crowns on the board.
fn bga_tiebreak(board: &Board) -> u32 {
    let mut visited = [[false; STORE]; STORE];
    let mut biggest = 1u32; // castle territory
    let mut total_crowns = 0u32;
    for r in 0..STORE as u8 {
        for c in 0..STORE as u8 {
            total_crowns += board.cell(r, c).crowns() as u32;
            let terrain = match board.cell(r, c).terrain_of() {
                Some(t) => t,
                None => continue,
            };
            if visited[r as usize][c as usize] {
                continue;
            }
            let mut stack = vec![(r, c)];
            visited[r as usize][c as usize] = true;
            let mut size = 0u32;
            while let Some((cr, cc)) = stack.pop() {
                size += 1;
                for (dr, dc) in [(-1i8, 0i8), (0, 1), (1, 0), (0, -1)] {
                    let (nr, nc) = (cr as i8 + dr, cc as i8 + dc);
                    if nr < 0 || nc < 0 || nr >= STORE as i8 || nc >= STORE as i8 {
                        continue;
                    }
                    let (nr, nc) = (nr as u8, nc as u8);
                    if !visited[nr as usize][nc as usize]
                        && board.cell(nr, nc).terrain_of() == Some(terrain)
                    {
                        visited[nr as usize][nc as usize] = true;
                        stack.push((nr, nc));
                    }
                }
            }
            biggest = biggest.max(size);
        }
    }
    biggest * 100 + total_crowns
}

/// `getKingdomTerritories()` + `getKingdomScore()`: Σ `size × crowns` over territories.
fn bga_crown_score(board: &Board) -> u32 {
    let mut visited = [[false; STORE]; STORE];
    let mut score = 0u32;
    for r in 0..STORE as u8 {
        for c in 0..STORE as u8 {
            let terrain = match board.cell(r, c).terrain_of() {
                Some(t) => t,
                None => continue,
            };
            if visited[r as usize][c as usize] {
                continue;
            }
            let mut stack = vec![(r, c)];
            visited[r as usize][c as usize] = true;
            let (mut size, mut crowns) = (0u32, 0u32);
            while let Some((cr, cc)) = stack.pop() {
                size += 1;
                crowns += board.cell(cr, cc).crowns() as u32;
                for (dr, dc) in [(-1i8, 0i8), (0, 1), (1, 0), (0, -1)] {
                    let (nr, nc) = (cr as i8 + dr, cc as i8 + dc);
                    if nr < 0 || nc < 0 || nr >= STORE as i8 || nc >= STORE as i8 {
                        continue;
                    }
                    let (nr, nc) = (nr as u8, nc as u8);
                    if !visited[nr as usize][nc as usize]
                        && board.cell(nr, nc).terrain_of() == Some(terrain)
                    {
                        visited[nr as usize][nc as usize] = true;
                        stack.push((nr, nc));
                    }
                }
            }
            score += size * crowns;
        }
    }
    score
}

// ============================================================================
// Harness: play random games and collect terminal boards + per-seat discards
// ============================================================================

struct Finished {
    gs: GameState,
    discards: [u32; 2],
}

fn play_random_game(seed: u64) -> Finished {
    let mut gs = new_game(2);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut buf = Vec::new();
    let mut discards = [0u32; 2];
    loop {
        match current_decision(&gs) {
            Decision::Terminal => break,
            Decision::Chance => {
                apply_chance(&mut gs, &mut rng);
            }
            Decision::Player(p) => {
                legal_actions(&gs, &mut buf);
                let pick = buf[rng.gen_range(0..buf.len())];
                if gs.phase == Phase::Place && pick == Action::Discard {
                    discards[p as usize] += 1;
                }
                apply_action(&mut gs, pick);
            }
        }
    }
    Finished { gs, discards }
}

// ============================================================================
// Tests
// ============================================================================

/// The territory flood-fill and `size × crowns` sum agree with BGA on real games.
#[test]
fn crown_scoring_matches_bga() {
    for seed in 0..300u64 {
        let f = play_random_game(seed);
        for seat in 0..2 {
            let ours = score_board(&f.gs.boards[seat], Variants::NONE).crown_score;
            let theirs = bga_crown_score(&f.gs.boards[seat]);
            assert_eq!(
                ours, theirs,
                "crown score diverged (seed {seed}, seat {seat})"
            );
        }
    }
}

/// Harmony: our `filled == GRID²−1` grid test and BGA's "never discarded" test are
/// equivalent, because every placement adds exactly 2 squares and a full deal is exactly
/// `GRID²−1` squares — so a player fills the grid iff they discarded nothing.
#[test]
fn harmony_matches_bga() {
    for seed in 0..300u64 {
        let f = play_random_game(seed);
        for seat in 0..2 {
            let ours = score_board(&f.gs.boards[seat], Variants::MIGHTY_DUEL).harmony == 5;
            let theirs = bga_harmony(f.discards[seat]);
            assert_eq!(ours, theirs, "harmony diverged (seed {seed}, seat {seat})");
        }
    }
}

/// Middle Kingdom: BGA requires the occupied bounding box to be *exactly symmetric* about
/// the castle; ours only requires it to fit inside a castle-centered `GRID×GRID` window.
/// Ours is strictly weaker, so it awards +10 on boards BGA scores 0.
#[test]
fn middle_kingdom_matches_bga() {
    let mut diverged = 0;
    let mut total = 0;
    for seed in 0..300u64 {
        let f = play_random_game(seed);
        for seat in 0..2 {
            let board = &f.gs.boards[seat];
            let ours = score_board(board, Variants::MIGHTY_DUEL).middle_kingdom == 10;
            let theirs = bga_middle_kingdom(board);
            total += 1;
            if ours != theirs {
                diverged += 1;
            }
        }
    }
    assert_eq!(
        diverged, 0,
        "Middle Kingdom diverged on {diverged}/{total} boards"
    );
}

/// End-to-end: the terminal value the agent trains on must induce the same win/loss/draw
/// as BGA's ranking — `player_score`, then `player_score_aux = biggestTerritory * 100 +
/// totalCrowns`. This exercises crown scoring, both variant bonuses, and both tiebreak
/// levels together, so it is the check that actually matters for training targets.
#[test]
fn terminal_ranking_matches_bga() {
    for seed in 0..4000u64 {
        let f = play_random_game(seed);
        let ours = terminal_value(&f.gs).expect("game is over");
        let bga: Vec<(u32, u32)> = (0..2)
            .map(|i| {
                let b = &f.gs.boards[i];
                let score = bga_crown_score(b)
                    + if bga_harmony(f.discards[i]) { 5 } else { 0 }
                    + if bga_middle_kingdom(b) { 10 } else { 0 };
                (score, bga_tiebreak(b))
            })
            .collect();
        let best = *bga.iter().max().expect("two seats");
        let winners = bga.iter().filter(|&&r| r == best).count() as f32;
        for (i, &r) in bga.iter().enumerate() {
            let want = if r == best { 1.0 / winners } else { 0.0 };
            assert_eq!(
                ours[i], want,
                "terminal value diverged (seed {seed}, seat {i}): ours={ours:?} bga={bga:?}"
            );
        }
    }
}

/// Pins the Middle Kingdom edge case that the pre-audit rule got wrong: a kingdom spanning
/// 1 column left and `GRID/2` right of the castle fits inside a castle-centered `GRID×GRID`
/// window, but its bounding box is off-center, so BGA awards nothing — and so do we.
#[test]
fn asymmetric_kingdom_inside_a_centered_window_earns_nothing() {
    let mut b = Board::with_castle();
    b.place_square(CENTER, CENTER - 1, Terrain::Wheat, 0);
    for k in 1..=(GRID / 2) as u8 {
        b.place_square(CENTER, CENTER + k, Terrain::Wheat, 0);
    }
    assert!(
        !bga_middle_kingdom(&b),
        "BGA rule: bounding box -1..+3 is not symmetric about the castle"
    );
    assert_eq!(
        score_board(&b, Variants::MIGHTY_DUEL).middle_kingdom,
        0,
        "our rule must agree"
    );
}

/// The 2-player starting-claim order is BGA's snake, not a random king draw.
///
/// `activateOwnerOfNextKing()` advances the active player on every claim *except* when
/// exactly 2 dominoes are owned, so the sequence is first, second, second, first — "a
/// balanced setup is done: first player gets first and last domino choices". Only which
/// seat is first is random. This reproduces that control flow and compares.
#[test]
fn starting_claim_order_matches_bga() {
    /// BGA `activateOwnerOfNextKing()`, replayed for `pc` seats starting from `first`.
    fn bga_claim_order(pc: u8, first: u8) -> Vec<u8> {
        let mut active = first;
        let mut order = Vec::new();
        for owned in 0..4u8 {
            if owned > 0 && !(pc == 2 && owned == 2) {
                active = (active + 1) % pc; // activeNextPlayer(): around the table
            }
            order.push(active);
        }
        order
    }
    assert_eq!(
        bga_claim_order(2, 0),
        vec![0, 1, 1, 0],
        "the documented snake"
    );

    // Every starting order our chance node can produce must be one BGA can produce.
    let gs = new_game(2);
    let outcomes = kingdomino_engine::core::turn::chance_outcomes(&GameState {
        phase: Phase::StartOrder,
        ..gs
    });
    assert_eq!(outcomes.len(), 2, "2 outcomes: which seat claims first");
    let bga_orders: Vec<Vec<u8>> = (0..2).map(|f| bga_claim_order(2, f)).collect();
    for (action, prob) in outcomes {
        let mut probe = GameState {
            phase: Phase::StartOrder,
            ..gs
        };
        apply_action(&mut probe, action);
        assert!(
            bga_orders.contains(&probe.claim_order.to_vec()),
            "claim order {:?} is not one BGA produces ({bga_orders:?})",
            probe.claim_order
        );
        assert!(
            (prob - 0.5).abs() < 1e-6,
            "each seat is first half the time"
        );
    }
}
