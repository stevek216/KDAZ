//! Public observation + legal-action JSON. The corpus schema and the UI/bridge observation
//! both come from here, so every producer (the PyO3 bridge and the Rust self-play binary)
//! emits an identical schema. Pure: derived from `GameState` (everything is public).

use serde_json::{json, Value};

use kingdomino_engine::components::{domino, Square};
use kingdomino_engine::core::{Action, Board, GameState, Phase, Slot, CENTER};
use kingdomino_engine::rules::{cell_of, score_board};

pub fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Draw => "draw",
        Phase::StartOrder => "start_order",
        Phase::StartClaim => "start_claim",
        Phase::Place => "place",
        Phase::Claim => "claim",
        Phase::GameOver => "game_over",
    }
}

pub fn square_json(s: Square) -> Value {
    json!({ "terrain": s.terrain.index(), "crowns": s.crowns })
}

/// JSON for an action, tagged with the `index` to pass back to `apply` / `apply_chance_index`.
pub fn action_json(a: Action, index: usize) -> Value {
    match a {
        Action::Claim { slot } => json!({ "index": index, "type": "claim", "slot": slot }),
        Action::Place { anchor, rot } => {
            let (row, col) = cell_of(anchor);
            json!({ "index": index, "type": "place", "anchor": anchor, "rot": rot, "row": row, "col": col })
        }
        Action::Discard => json!({ "index": index, "type": "discard" }),
        Action::Draw { domino: d } => {
            json!({ "index": index, "type": "draw", "domino": d, "number": d as u16 + 1 })
        }
        Action::StartOrder { perm } => {
            json!({ "index": index, "type": "start_order", "perm": perm })
        }
    }
}

fn board_json(b: &Board) -> Value {
    let mut cells = Vec::new();
    if b.present {
        for r in b.min_r..=b.max_r {
            for c in b.min_c..=b.max_c {
                let cell = b.cell(r, c);
                if let Some(t) = cell.terrain_of() {
                    cells.push(
                        json!({ "r": r, "c": c, "terrain": t.index(), "crowns": cell.crowns() }),
                    );
                }
            }
        }
    }
    json!({
        "present": b.present,
        "filled": b.filled,
        "min_r": b.min_r, "max_r": b.max_r, "min_c": b.min_c, "max_c": b.max_c,
        "castle": [CENTER, CENTER],
        "cells": cells,
    })
}

fn line_json(line: &[Slot]) -> Value {
    let slots: Vec<Value> = line
        .iter()
        .enumerate()
        .map(|(i, s)| {
            if s.is_filled() {
                json!({
                    "slot": i,
                    "domino": s.domino,
                    "number": s.domino as u16 + 1,
                    "owner": if s.is_claimed() { Some(s.owner) } else { None },
                })
            } else {
                json!({ "slot": i, "domino": Value::Null, "owner": Value::Null })
            }
        })
        .collect();
    Value::Array(slots)
}

pub fn observation_json(gs: &GameState) -> Value {
    let pc = gs.player_count as usize;
    let remaining: Vec<u8> = (0..48u8)
        .filter(|d| gs.remaining & (1u64 << d) != 0)
        .collect();
    let seats: Vec<Value> = (0..pc).map(|s| board_json(&gs.boards[s])).collect();
    let scores: Vec<Value> = (0..pc)
        .map(|s| {
            let sb = score_board(&gs.boards[s], gs.variants);
            json!({
                "crown_score": sb.crown_score,
                "harmony": sb.harmony,
                "middle_kingdom": sb.middle_kingdom,
                "total": sb.total,
                "largest_territory": sb.largest_territory,
            })
        })
        .collect();

    let current_domino = if gs.phase == Phase::Place {
        let def = domino(gs.current_line[gs.turn_cursor as usize].domino);
        json!({ "number": def.number, "a": square_json(def.a), "b": square_json(def.b) })
    } else {
        Value::Null
    };

    json!({
        "player_count": gs.player_count,
        "to_act": gs.to_act,
        "round": gs.round,
        "phase": phase_str(gs.phase),
        "turn_cursor": gs.turn_cursor,
        "is_terminal": gs.phase == Phase::GameOver,
        "is_chance": matches!(gs.phase, Phase::Draw | Phase::StartOrder),
        "variants": { "harmony": gs.variants.harmony, "middle_kingdom": gs.variants.middle_kingdom },
        "current_line": line_json(&gs.current_line),
        "next_line": line_json(&gs.next_line),
        "current_domino": current_domino,
        "remaining": remaining,
        "deck_remaining": gs.deck_remaining(),
        "seats": seats,
        "scores": scores,
    })
}

/// The public observation as a JSON string.
pub fn obs_json(gs: &GameState) -> String {
    observation_json(gs).to_string()
}

/// A JSON array of legal actions built from an `Action` buffer (same schema as the bridge's
/// `Game.legal_actions`).
pub fn legal_json(buf: &[Action]) -> String {
    let arr: Vec<Value> = buf
        .iter()
        .enumerate()
        .map(|(i, &a)| action_json(a, i))
        .collect();
    Value::Array(arr).to_string()
}
