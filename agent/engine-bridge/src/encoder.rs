//! Rust port of the Python feature encoder (`kdagent/encoder.py`), operating directly on a
//! `GameState` — no JSON, no per-cell Python loops. This is the hot path for batched net
//! self-play: each leaf is encoded straight into a shared tensor buffer. Kept bit-for-bit
//! compatible with the Python encoder (a parity test guards it), so a net trained on the
//! Python path runs unchanged here.
//!
//! Layout (per state): board `[pc·N_PLANES, 13, 13]`, lines `[8, LINE_FEATS]`, glob `[GLOB]`,
//! seat-relative (self first). See `agent/docs/feature-schema.md`.

use kingdomino_engine::components::domino;
use kingdomino_engine::core::{GameState, Phase, NO_OWNER};
use kingdomino_engine::rules::score_board;

pub const STORE: usize = 13;
pub const GRID: usize = 7;
pub const N_TERRAIN: usize = 6;
pub const N_PLANES: usize = 13; // 6 terrain + 4 crown + castle + empty + reach
pub const LINE_FEATS: usize = 23;
pub const LINES_LEN: usize = 8 * LINE_FEATS;
const N_PHASE: usize = 3; // start_claim / place / claim
const PER_SEAT_GLOBAL: usize = 5;

const P_CASTLE: usize = 10;
const P_EMPTY: usize = 11;
const P_REACH: usize = 12;

pub fn board_per_state(pc: usize) -> usize {
    pc * N_PLANES * STORE * STORE
}
pub fn glob_len(pc: usize) -> usize {
    N_PHASE + 3 + 2 + PER_SEAT_GLOBAL * pc
}

fn phase_idx(p: Phase) -> Option<usize> {
    match p {
        Phase::StartClaim => Some(0),
        Phase::Place => Some(1),
        Phase::Claim => Some(2),
        _ => None,
    }
}

/// Encode one state into the (pre-zeroed) `board` / `lines` / `glob` slices.
pub fn encode_into(gs: &GameState, board: &mut [f32], lines: &mut [f32], glob: &mut [f32]) {
    let pc = gs.player_count as usize;
    let to_act = gs.to_act as usize;

    // ---- board planes, seat-relative ----
    for si in 0..pc {
        let b = &gs.boards[(to_act + si) % pc];
        let base = si * N_PLANES * STORE * STORE;
        let at = |p: usize, r: usize, c: usize| base + (p * STORE + r) * STORE + c;
        let (mnr, mxr) = (b.min_r as usize, b.max_r as usize);
        let (mnc, mxc) = (b.min_c as usize, b.max_c as usize);
        for r in 0..STORE {
            for c in 0..STORE {
                let cell = b.cell(r as u8, c as u8);
                if cell.is_castle() {
                    board[at(P_CASTLE, r, c)] = 1.0;
                } else if let Some(t) = cell.terrain_of() {
                    board[at(t.index() as usize, r, c)] = 1.0;
                    board[at(N_TERRAIN + cell.crowns() as usize, r, c)] = 1.0;
                } else {
                    board[at(P_EMPTY, r, c)] = 1.0;
                }
                let span_r = mxr.max(r) - mnr.min(r);
                let span_c = mxc.max(c) - mnc.min(c);
                if span_r < GRID && span_c < GRID {
                    board[at(P_REACH, r, c)] = 1.0;
                }
            }
        }
    }

    // ---- draft-line tokens (current_line 0..3, then next_line 4..7) ----
    encode_line(gs, &gs.current_line, true, lines);
    encode_line(gs, &gs.next_line, false, lines);

    // ---- global vector ----
    if let Some(pi) = phase_idx(gs.phase) {
        glob[pi] = 1.0;
    }
    glob[N_PHASE] = gs.round as f32 / 12.0;
    glob[N_PHASE + 1] = gs.turn_cursor as f32 / 3.0;
    glob[N_PHASE + 2] = gs.remaining.count_ones() as f32 / 48.0;
    glob[N_PHASE + 3] = gs.variants.harmony as u8 as f32;
    glob[N_PHASE + 4] = gs.variants.middle_kingdom as u8 as f32;
    let mut o = N_PHASE + 5;
    for si in 0..pc {
        let b = &gs.boards[(to_act + si) % pc];
        let sc = score_board(b, gs.variants);
        glob[o] = b.filled as f32 / 48.0;
        glob[o + 1] = (b.max_r - b.min_r) as f32 / (GRID as f32 - 1.0);
        glob[o + 2] = (b.max_c - b.min_c) as f32 / (GRID as f32 - 1.0);
        glob[o + 3] = sc.crown_score.min(100) as f32 / 100.0;
        glob[o + 4] = sc.largest_territory as f32 / (GRID * GRID) as f32;
        o += PER_SEAT_GLOBAL;
    }
}

fn encode_line(
    gs: &GameState,
    line: &[kingdomino_engine::core::Slot; 4],
    is_current: bool,
    lines: &mut [f32],
) {
    let base_tok = if is_current { 0 } else { 4 };
    let claim_active = matches!(gs.phase, Phase::StartClaim if is_current)
        || matches!(gs.phase, Phase::Claim if !is_current);
    for (i, &slot) in line.iter().enumerate() {
        let t = (base_tok + i) * LINE_FEATS;
        lines[t] = is_current as u8 as f32;
        lines[t + 1] = (!is_current) as u8 as f32;
        if slot.is_filled() {
            lines[t + 2] = 1.0;
            let def = domino(slot.domino);
            lines[t + 3 + def.a.terrain.index() as usize] = 1.0;
            lines[t + 9] = def.a.crowns as f32 / 3.0;
            lines[t + 10 + def.b.terrain.index() as usize] = 1.0;
            lines[t + 16] = def.b.crowns as f32 / 3.0;
            lines[t + 17] = def.number as f32 / 48.0;
            let rel = if slot.owner == NO_OWNER {
                2
            } else if slot.owner == gs.to_act {
                0
            } else {
                1
            };
            lines[t + 18 + rel] = 1.0;
        }
        if is_current && matches!(gs.phase, Phase::Place) && i == gs.turn_cursor as usize {
            lines[t + 21] = 1.0;
        }
        if claim_active && slot.is_filled() && !slot.is_claimed() {
            lines[t + 22] = 1.0;
        }
    }
}
