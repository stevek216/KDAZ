//! The turn / decision state machine (`docs/engine-design.md` §4, §6).
//!
//! Pure functions over [`GameState`]: `current_decision`, `legal_actions`, `apply_action`
//! (deterministic, also applies a *given* chance outcome), `chance_outcomes` (the hidden-draw
//! distribution), and `apply_chance` (the single RNG entry point). Deterministic bookkeeping —
//! sorting a drawn line, rotating `next_line` into `current_line`, advancing the cursor, and
//! detecting the end of the game — folds into `apply_action` and never produces its own node.

use rand::Rng;

use crate::components::{domino, DominoDef, DominoId};
use crate::core::action::{Action, Decision};
use crate::core::state::{GameState, Phase, Slot, LINE, MAX_PLAYERS, NO_OWNER};
use crate::rules::place::{cell_of, legal_placements};
use crate::rules::score::score_board;

// =================================================================================
// Decision classification
// =================================================================================

/// What kind of node the game is at.
pub fn current_decision(gs: &GameState) -> Decision {
    match gs.phase {
        Phase::Draw | Phase::StartOrder => Decision::Chance,
        Phase::StartClaim | Phase::Place | Phase::Claim => Decision::Player(gs.to_act),
        Phase::GameOver => Decision::Terminal,
    }
}

pub fn is_terminal(gs: &GameState) -> bool {
    gs.phase == Phase::GameOver
}

pub fn is_chance(gs: &GameState) -> bool {
    matches!(gs.phase, Phase::Draw | Phase::StartOrder)
}

// =================================================================================
// Terminal value (max-n vector, §7.1)
// =================================================================================

/// The per-seat outcome vector at a terminal state, or `None` if the game is not over.
/// Entry `i` is seat `i`'s value (absolute seat index; dead seats are `0.0`).
///
/// Ranking (rulebook p.3): highest total score wins; ties broken by the **largest single
/// territory**; a remaining tie is a **shared victory**. Co-winners split `1.0` evenly
/// (`1/w` each — so 2p win/loss = 1.0/0.0 and a 2p shared = 0.5/0.5). The convention is
/// swappable at trainer time; the engine just reports the ranking (CLAUDE §4, Q6).
pub fn terminal_value(gs: &GameState) -> Option<[f32; MAX_PLAYERS]> {
    if gs.phase != Phase::GameOver {
        return None;
    }
    let pc = gs.player_count as usize;
    // (total score, largest territory) per seat — lexicographic comparison = the rulebook order.
    let mut ranked = [(0u32, 0u32); MAX_PLAYERS];
    for (seat, slot) in ranked.iter_mut().enumerate().take(pc) {
        let s = score_board(&gs.boards[seat], gs.variants);
        *slot = (s.total, s.largest_territory);
    }
    let best = *ranked[..pc].iter().max().expect("at least one seat");
    let winners = ranked[..pc].iter().filter(|&&r| r == best).count() as f32;
    let mut out = [0.0f32; MAX_PLAYERS];
    for (seat, &r) in ranked[..pc].iter().enumerate() {
        if r == best {
            out[seat] = 1.0 / winners;
        }
    }
    Some(out)
}

/// The domino the acting seat must place this turn (valid in `Phase::Place`).
fn current_domino(gs: &GameState) -> &'static DominoDef {
    domino(gs.current_line[gs.turn_cursor as usize].domino)
}

// =================================================================================
// Legal actions (player nodes only)
// =================================================================================

/// Fill `out` with the legal actions at a player node. Clears `out` first. At chance/terminal
/// nodes it leaves `out` empty (use `chance_outcomes` / `apply_chance` for chance).
pub fn legal_actions(gs: &GameState, out: &mut Vec<Action>) {
    out.clear();
    match gs.phase {
        Phase::StartClaim => unclaimed_slots(&gs.current_line, out),
        Phase::Claim => unclaimed_slots(&gs.next_line, out),
        Phase::Place => {
            let board = &gs.boards[gs.to_act as usize];
            legal_placements(board, current_domino(gs), out);
            if out.is_empty() {
                out.push(Action::Discard); // forced discard only when nothing fits
            }
        }
        Phase::Draw | Phase::StartOrder | Phase::GameOver => {}
    }
}

fn unclaimed_slots(line: &[Slot; LINE], out: &mut Vec<Action>) {
    for (i, slot) in line.iter().enumerate() {
        if slot.is_filled() && !slot.is_claimed() {
            out.push(Action::Claim { slot: i as u8 });
        }
    }
}

// =================================================================================
// Chance: the hidden draw + starting order (§6)
// =================================================================================

/// The chance distribution at the current node: `(outcome, probability)` pairs.
/// - `Draw`: one entry per remaining domino, each `1/k` (single-draw model, §6.1).
/// - `StartOrder`: one entry per distinct seat ordering, equiprobable (§6.2; valid for the
///   supported equal-kings counts 2 and 4).
pub fn chance_outcomes(gs: &GameState) -> Vec<(Action, f32)> {
    match gs.phase {
        Phase::Draw => {
            let k = gs.remaining.count_ones();
            let p = 1.0 / k as f32;
            (0..48u8)
                .filter(|d| gs.remaining & (1u64 << d) != 0)
                .map(|d| (Action::Draw { domino: d }, p))
                .collect()
        }
        Phase::StartOrder => {
            let count = count_start_orders(gs);
            let p = 1.0 / count as f32;
            (0..count)
                .map(|i| (Action::StartOrder { perm: i as u8 }, p))
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Sample and apply one chance outcome using the injected RNG (the only RNG use in the engine).
/// Returns the applied `Action`. Panics if called at a non-chance node.
pub fn apply_chance(gs: &mut GameState, rng: &mut impl Rng) -> Action {
    let action = match gs.phase {
        Phase::Draw => {
            let k = gs.remaining.count_ones();
            let mut pick = rng.gen_range(0..k);
            let mut chosen = 0u8;
            for d in 0..48u8 {
                if gs.remaining & (1u64 << d) != 0 {
                    if pick == 0 {
                        chosen = d;
                        break;
                    }
                    pick -= 1;
                }
            }
            Action::Draw { domino: chosen }
        }
        Phase::StartOrder => {
            let count = count_start_orders(gs);
            Action::StartOrder {
                perm: rng.gen_range(0..count) as u8,
            }
        }
        _ => panic!("apply_chance called at a non-chance node ({:?})", gs.phase),
    };
    apply_action(gs, action);
    action
}

// --- starting-order enumeration (distinct permutations of the seat multiset) ---

/// Kings per seat = draft width / player count (2 for Mighty Duel, 1 for 4p).
fn kings_per_seat(gs: &GameState) -> usize {
    LINE / gs.player_count as usize
}

/// The sorted seat multiset for the starting order, e.g. `[0,0,1,1]` for 2p.
fn start_multiset(gs: &GameState) -> [u8; LINE] {
    let kpp = kings_per_seat(gs);
    let mut m = [0u8; LINE];
    let mut idx = 0;
    for seat in 0..gs.player_count {
        for _ in 0..kpp {
            m[idx] = seat;
            idx += 1;
        }
    }
    m
}

/// In-place next lexicographic permutation of a 4-element array; `false` if already last.
fn next_permutation(a: &mut [u8; LINE]) -> bool {
    let n = LINE;
    let mut i = n - 1;
    while i > 0 && a[i - 1] >= a[i] {
        i -= 1;
    }
    if i == 0 {
        return false;
    }
    let mut j = n - 1;
    while a[j] <= a[i - 1] {
        j -= 1;
    }
    a.swap(i - 1, j);
    a[i..].reverse();
    true
}

fn count_start_orders(gs: &GameState) -> u32 {
    let mut m = start_multiset(gs);
    let mut n = 1;
    while next_permutation(&mut m) {
        n += 1;
    }
    n
}

fn nth_start_order(gs: &GameState, i: u8) -> [u8; LINE] {
    let mut m = start_multiset(gs);
    for _ in 0..i {
        next_permutation(&mut m);
    }
    m
}

// =================================================================================
// apply_action (deterministic)
// =================================================================================

/// Apply `action` to `gs`, advancing to the next node. Deterministic: at a chance node it
/// applies the *given* outcome (so search can apply a specific enumerated draw/order). The
/// action must be legal for the current node.
pub fn apply_action(gs: &mut GameState, action: Action) {
    match gs.phase {
        Phase::Draw => match action {
            Action::Draw { domino } => apply_draw(gs, domino),
            _ => panic!("expected Draw at Phase::Draw, got {action:?}"),
        },
        Phase::StartOrder => match action {
            Action::StartOrder { perm } => {
                gs.claim_order = nth_start_order(gs, perm);
                gs.turn_cursor = 0;
                gs.phase = Phase::StartClaim;
                gs.to_act = gs.claim_order[0];
            }
            _ => panic!("expected StartOrder at Phase::StartOrder, got {action:?}"),
        },
        Phase::StartClaim => match action {
            Action::Claim { slot } => apply_start_claim(gs, slot),
            _ => panic!("expected Claim at Phase::StartClaim, got {action:?}"),
        },
        Phase::Place => match action {
            Action::Place { anchor, rot } => {
                place_current_domino(gs, anchor, rot);
                after_place(gs);
            }
            Action::Discard => after_place(gs),
            _ => panic!("expected Place/Discard at Phase::Place, got {action:?}"),
        },
        Phase::Claim => match action {
            Action::Claim { slot } => {
                gs.next_line[slot as usize].owner = gs.to_act;
                advance_cursor(gs);
            }
            _ => panic!("expected Claim at Phase::Claim, got {action:?}"),
        },
        Phase::GameOver => panic!("apply_action called at GameOver"),
    }
}

fn apply_draw(gs: &mut GameState, d: DominoId) {
    debug_assert!(
        gs.remaining & (1u64 << d) != 0,
        "drawing a domino not in the deck"
    );
    gs.remaining &= !(1u64 << d);
    gs.draw_buf[gs.draw_count as usize] = d;
    gs.draw_count += 1;
    if gs.draw_count as usize == LINE {
        complete_line(gs);
    }
}

/// A full line has been drawn: sort it ascending by id (= draft number) and install it. The
/// first line ever fills `current_line` (→ starting order); every later line fills `next_line`
/// (→ a play round).
fn complete_line(gs: &mut GameState) {
    gs.draw_buf.sort_unstable();
    let line: [Slot; LINE] = core::array::from_fn(|i| Slot {
        domino: gs.draw_buf[i],
        owner: NO_OWNER,
    });
    gs.draw_count = 0;
    if gs.current_line.iter().all(|s| !s.is_filled()) {
        gs.current_line = line;
        gs.phase = Phase::StartOrder;
    } else {
        gs.next_line = line;
        start_play_round(gs);
    }
}

fn apply_start_claim(gs: &mut GameState, slot: u8) {
    gs.current_line[slot as usize].owner = gs.to_act;
    gs.turn_cursor += 1;
    if (gs.turn_cursor as usize) < LINE {
        gs.to_act = gs.claim_order[gs.turn_cursor as usize];
    } else {
        // Starting round done; draw the second line (becomes next_line), then play begins.
        gs.phase = Phase::Draw;
    }
}

fn place_current_domino(gs: &mut GameState, anchor: u16, rot: u8) {
    let def = current_domino(gs);
    let (r, c) = cell_of(anchor);
    let (dr, dc) = crate::rules::place::DIRS[rot as usize];
    let (br, bc) = ((r as i8 + dr) as u8, (c as i8 + dc) as u8);
    let (a, b) = (def.a, def.b);
    let board = &mut gs.boards[gs.to_act as usize];
    board.place_square(r, c, a.terrain, a.crowns);
    board.place_square(br, bc, b.terrain, b.crowns);
}

/// Position the game at the first `Place` of a play round (current_line is fully claimed).
fn start_play_round(gs: &mut GameState) {
    gs.turn_cursor = 0;
    gs.phase = Phase::Place;
    gs.to_act = gs.current_line[0].owner;
}

/// Is this a final, place-only round (no line to claim from)? True once the deck is exhausted
/// and the last line has been promoted to `current_line`.
fn is_final_round(gs: &GameState) -> bool {
    gs.next_line.iter().all(|s| !s.is_filled())
}

/// After a Place/Discard: in a normal round the same seat then claims; in the final round there
/// is nothing to claim, so move straight to the next seat.
fn after_place(gs: &mut GameState) {
    if is_final_round(gs) {
        advance_cursor(gs);
    } else {
        gs.phase = Phase::Claim; // to_act unchanged: same king places then claims
    }
}

/// Advance to the next king's Place in the current round, or end the round.
fn advance_cursor(gs: &mut GameState) {
    gs.turn_cursor += 1;
    if (gs.turn_cursor as usize) < LINE {
        gs.phase = Phase::Place;
        gs.to_act = gs.current_line[gs.turn_cursor as usize].owner;
    } else {
        end_round(gs);
    }
}

/// End the round: promote `next_line` to `current_line`, then either draw the next line, start
/// the final place-only round, or end the game.
fn end_round(gs: &mut GameState) {
    gs.current_line = gs.next_line;
    gs.next_line = [Slot::EMPTY; LINE];
    gs.turn_cursor = 0;
    gs.round += 1;
    if gs.current_line.iter().all(|s| !s.is_filled()) {
        gs.phase = Phase::GameOver; // the final place-only round just finished
    } else if gs.remaining > 0 {
        gs.phase = Phase::Draw; // draw the next line into next_line, then play
    } else {
        // Deck empty: the promoted line is the last; place it with no claim, then end.
        start_play_round(gs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::setup::new_game;
    use crate::core::state::{GRID, MAX_PLAYERS};
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn terminal_value_ranks_and_shares() {
        use crate::components::Terrain;
        use crate::core::state::CENTER;

        // Not terminal -> None.
        let gs = new_game(2);
        assert!(terminal_value(&gs).is_none());

        // Build a finished 2p game by hand: seat 0 outscores seat 1.
        let mut gs = new_game(2);
        gs.phase = Phase::GameOver;
        // seat 0: a 3-forest territory with 2 crowns = 6 points.
        for k in 0..3u8 {
            let crowns = if k == 0 { 2 } else { 0 };
            gs.boards[0].place_square(CENTER, CENTER - 1 - k, Terrain::Forest, crowns);
        }
        // seat 1: a 2-lake territory with 1 crown = 2 points.
        for k in 0..2u8 {
            let crowns = if k == 0 { 1 } else { 0 };
            gs.boards[1].place_square(CENTER, CENTER + 1 + k, Terrain::Lake, crowns);
        }
        let v = terminal_value(&gs).unwrap();
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 0.0);
        assert_eq!(v[2], 0.0); // dead seat

        // A full tie (identical boards) -> shared victory, 0.5 each.
        let mut tie = new_game(2);
        tie.phase = Phase::GameOver;
        for seat in 0..2 {
            tie.boards[seat].place_square(CENTER, CENTER - 1, Terrain::Forest, 1);
        }
        let vt = terminal_value(&tie).unwrap();
        assert_eq!(vt[0], 0.5);
        assert_eq!(vt[1], 0.5);
    }

    #[test]
    fn start_order_counts() {
        // 2p: 4!/(2!2!) = 6 distinct claim orders.
        let gs = new_game(2);
        assert_eq!(count_start_orders(&gs), 6);
        // 4p: 4! = 24.
        let gs4 = new_game(4);
        assert_eq!(count_start_orders(&gs4), 24);
        // nth_start_order is a stable, distinct enumeration.
        let mut seen = std::collections::HashSet::new();
        for i in 0..6 {
            assert!(seen.insert(nth_start_order(&gs, i)));
        }
    }

    /// A full random 2p game runs to terminal, and every drawn domino is accounted for
    /// (placed or discarded), with both kingdoms always inside the 7×7 bound.
    #[test]
    fn random_self_play_reaches_terminal() {
        for seed in 0..40u64 {
            let mut gs = new_game(2);
            let mut rng = ChaCha8Rng::seed_from_u64(seed);
            let mut buf = Vec::new();
            let mut placed = [0u32; MAX_PLAYERS];
            let mut discarded = [0u32; MAX_PLAYERS];

            let mut steps = 0;
            loop {
                steps += 1;
                assert!(steps < 100_000, "game did not terminate (seed {seed})");
                match current_decision(&gs) {
                    Decision::Terminal => break,
                    Decision::Chance => {
                        apply_chance(&mut gs, &mut rng);
                    }
                    Decision::Player(p) => {
                        legal_actions(&gs, &mut buf);
                        assert!(
                            !buf.is_empty(),
                            "no legal actions for seat {p} (seed {seed})"
                        );
                        let pick = buf[rng.gen_range(0..buf.len())];
                        match (gs.phase, pick) {
                            (Phase::Place, Action::Place { .. }) => placed[p as usize] += 1,
                            (Phase::Place, Action::Discard) => discarded[p as usize] += 1,
                            _ => {}
                        }
                        apply_action(&mut gs, pick);
                    }
                }
            }

            // Deck fully drawn; every seat consumed exactly 24 dominoes (12 lines × 2 kings).
            assert_eq!(gs.deck_remaining(), 0, "seed {seed}");
            for p in 0..2 {
                assert_eq!(
                    placed[p] + discarded[p],
                    24,
                    "seat {p} domino count (seed {seed})"
                );
                assert_eq!(
                    gs.boards[p].filled as u32,
                    2 * placed[p],
                    "seat {p} squares (seed {seed})"
                );
                let b = &gs.boards[p];
                assert!(b.max_r - b.min_r < GRID as u8, "row bound (seed {seed})");
                assert!(b.max_c - b.min_c < GRID as u8, "col bound (seed {seed})");
            }
        }
    }
}
