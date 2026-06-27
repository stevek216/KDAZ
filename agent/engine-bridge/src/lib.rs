//! PyO3 bridge: exposes the Kingdomino engine to Python for the agent (MCTS, self-play,
//! training). The engine stays the single source of rules truth — this layer only drives it
//! and emits observations. It never reimplements a rule.
//!
//! Unlike Space Base, **all of `GameState` is public information**: the only hidden thing in
//! Kingdomino is the *order* of future draws, which lives in the chance sampler, not the
//! state. So `observation()` can serialize the state directly — no separate public-info view.
//!
//! Python-facing surface (module `kingdomino`):
//! - `Game(seed, players, harmony=True, middle_kingdom=True)` — control + search:
//!   `player_count` / `to_act` / `round` / `phase` / `is_terminal` / `is_chance`,
//!   `legal_actions()` (JSON), `apply(index)`, `chance_outcomes()` (JSON), `apply_chance()`
//!   (sample via the game's RNG), `apply_chance_index(i)`, `clone()`, `terminal_value()`,
//!   `observation()` (JSON).
//! - `domino_table()` — JSON of all 48 dominoes (the static join target for ids).

#![allow(clippy::useless_conversion)] // PyO3 codegen on fallible methods (known false positive)

use numpy::ndarray::{Array2, Array3, Array4};
use numpy::{IntoPyArray, PyArray2, PyArray3, PyArray4};
use pyo3::exceptions::PyIndexError;
use pyo3::prelude::*;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rayon::prelude::*;
use serde_json::{json, Value};

use kingdomino_engine::components::{domino, Square, DOMINOES};
use kingdomino_engine::core::{
    apply_action, apply_chance, chance_outcomes, current_decision, legal_actions, new_game_with,
    terminal_value, Action, Board, Decision, GameState, Phase, Variants, CENTER,
};
use kingdomino_engine::rules::{cell_of, score_board};

mod encoder;
mod mcts;

fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Draw => "draw",
        Phase::StartOrder => "start_order",
        Phase::StartClaim => "start_claim",
        Phase::Place => "place",
        Phase::Claim => "claim",
        Phase::GameOver => "game_over",
    }
}

fn square_json(s: Square) -> Value {
    json!({ "terrain": s.terrain.index(), "crowns": s.crowns })
}

/// JSON for an action, tagged with the `index` to pass back to `apply` / `apply_chance_index`.
fn action_json(a: Action, index: usize) -> Value {
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

fn line_json(line: &[kingdomino_engine::core::Slot]) -> Value {
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

fn observation_json(gs: &GameState) -> Value {
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

    // The domino the acting seat must place (only meaningful at a Place node).
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

/// The public observation as a JSON string (shared by `Game.observation` and the Rust
/// batched self-play, so both producers emit an identical schema).
pub(crate) fn obs_json(gs: &GameState) -> String {
    observation_json(gs).to_string()
}

/// A JSON array of legal actions built from an `Action` buffer (same schema as
/// `Game.legal_actions`), for the Rust self-play recorder.
pub(crate) fn legal_json(buf: &[Action]) -> String {
    let arr: Vec<Value> = buf
        .iter()
        .enumerate()
        .map(|(i, &a)| action_json(a, i))
        .collect();
    Value::Array(arr).to_string()
}

/// A thin, cheaply-clonable handle around one `GameState` plus its sampling RNG.
#[pyclass]
pub struct Game {
    gs: GameState,
    rng: ChaCha8Rng,
    options: Vec<Action>,
}

impl Game {
    fn refresh(&mut self) {
        self.options.clear();
        if let Decision::Player(_) = current_decision(&self.gs) {
            legal_actions(&self.gs, &mut self.options);
        }
    }
}

#[pymethods]
impl Game {
    #[new]
    #[pyo3(signature = (seed, players, harmony = true, middle_kingdom = true))]
    fn new(seed: u64, players: u8, harmony: bool, middle_kingdom: bool) -> Self {
        let gs = new_game_with(
            players,
            Variants {
                harmony,
                middle_kingdom,
            },
        );
        let mut g = Game {
            gs,
            rng: ChaCha8Rng::seed_from_u64(seed),
            options: Vec::new(),
        };
        g.refresh();
        g
    }

    fn player_count(&self) -> u8 {
        self.gs.player_count
    }
    fn to_act(&self) -> u8 {
        self.gs.to_act
    }
    fn round(&self) -> u8 {
        self.gs.round
    }
    fn phase(&self) -> &'static str {
        phase_str(self.gs.phase)
    }
    fn is_terminal(&self) -> bool {
        self.gs.phase == Phase::GameOver
    }
    fn is_chance(&self) -> bool {
        matches!(self.gs.phase, Phase::Draw | Phase::StartOrder)
    }
    fn num_actions(&self) -> usize {
        self.options.len()
    }

    /// JSON array of legal actions at a player node (empty `[]` at chance/terminal nodes).
    fn legal_actions(&self) -> String {
        let arr: Vec<Value> = self
            .options
            .iter()
            .enumerate()
            .map(|(i, &a)| action_json(a, i))
            .collect();
        Value::Array(arr).to_string()
    }

    /// Apply the `index`-th legal action (from `legal_actions`).
    fn apply(&mut self, index: usize) -> PyResult<()> {
        let a = *self
            .options
            .get(index)
            .ok_or_else(|| PyIndexError::new_err(format!("action index {index} out of range")))?;
        apply_action(&mut self.gs, a);
        self.refresh();
        Ok(())
    }

    /// JSON array of chance outcomes `[{..., "prob": p}]` at a chance node (empty otherwise).
    fn chance_outcomes(&self) -> String {
        let arr: Vec<Value> = chance_outcomes(&self.gs)
            .into_iter()
            .enumerate()
            .map(|(i, (a, p))| {
                let mut v = action_json(a, i);
                v["prob"] = json!(p);
                v
            })
            .collect();
        Value::Array(arr).to_string()
    }

    /// Sample one chance outcome with the game's RNG and apply it (what self-play does).
    /// Returns the applied outcome as JSON. Errors if not at a chance node.
    fn apply_chance(&mut self) -> PyResult<String> {
        if !self.is_chance() {
            return Err(PyIndexError::new_err("apply_chance at a non-chance node"));
        }
        let a = apply_chance(&mut self.gs, &mut self.rng);
        self.refresh();
        Ok(action_json(a, 0).to_string())
    }

    /// Apply a specific enumerated chance outcome by its index in `chance_outcomes()` (what
    /// MCTS does when expanding a chance node).
    fn apply_chance_index(&mut self, index: usize) -> PyResult<()> {
        let outs = chance_outcomes(&self.gs);
        let (a, _) = *outs
            .get(index)
            .ok_or_else(|| PyIndexError::new_err(format!("chance index {index} out of range")))?;
        apply_action(&mut self.gs, a);
        self.refresh();
        Ok(())
    }

    /// The per-seat outcome vector at a terminal state (length `player_count`), else `None`.
    fn terminal_value(&self) -> Option<Vec<f32>> {
        terminal_value(&self.gs).map(|v| v[..self.gs.player_count as usize].to_vec())
    }

    /// The full public observation as JSON.
    fn observation(&self) -> String {
        observation_json(&self.gs).to_string()
    }

    /// Greedy rollout-MCTS visit policy over the current legal actions (no exploration noise),
    /// aligned to `legal_actions()` order — a fast Rust move for the arena. Must be called at a
    /// player node with more than one legal action.
    fn mcts_policy(&self, n_sims: u32, c_puct: f32, seed: u64) -> Vec<f32> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        mcts::run_mcts(&self.gs, n_sims, c_puct, 0.0, 0.0, &mut rng)
    }

    /// An independent copy of the game (state + RNG), so search can branch without aliasing.
    fn clone(&self) -> Game {
        Game {
            gs: self.gs,
            rng: self.rng.clone(),
            options: self.options.clone(),
        }
    }
}

/// Batch-encode the current states of `games` into network input tensors, in Rust (rayon-
/// parallel, GIL released). Returns `(board [B, pc·N_PLANES, 13, 13], lines [B, 8, LINE_FEATS],
/// glob [B, GLOB])` — zero-copy numpy, identical to the Python `kdagent.encoder` output.
#[pyfunction]
#[allow(clippy::type_complexity)] // the three named numpy return arrays are the API
fn encode_batch<'py>(
    py: Python<'py>,
    games: Vec<Py<Game>>,
) -> PyResult<(
    Bound<'py, PyArray4<f32>>,
    Bound<'py, PyArray3<f32>>,
    Bound<'py, PyArray2<f32>>,
)> {
    let states: Vec<GameState> = games.iter().map(|g| g.borrow(py).gs).collect();
    let b = states.len();
    let pc = states.first().map_or(2, |s| s.player_count as usize);
    let per_board = encoder::board_per_state(pc);
    let glen = encoder::glob_len(pc);
    let mut board = vec![0f32; b * per_board];
    let mut lines = vec![0f32; b * encoder::LINES_LEN];
    let mut glob = vec![0f32; b * glen];
    py.allow_threads(|| {
        board
            .par_chunks_mut(per_board.max(1))
            .zip(lines.par_chunks_mut(encoder::LINES_LEN))
            .zip(glob.par_chunks_mut(glen.max(1)))
            .zip(states.par_iter())
            .for_each(|(((bc, lc), gc), gs)| encoder::encode_into(gs, bc, lc, gc));
    });
    let board = Array4::from_shape_vec(
        (b, pc * encoder::N_PLANES, encoder::STORE, encoder::STORE),
        board,
    )
    .unwrap()
    .into_pyarray_bound(py);
    let lines = Array3::from_shape_vec((b, 8, encoder::LINE_FEATS), lines)
        .unwrap()
        .into_pyarray_bound(py);
    let glob = Array2::from_shape_vec((b, glen), glob)
        .unwrap()
        .into_pyarray_bound(py);
    Ok((board, lines, glob))
}

/// JSON of the static 48-domino table: `[{number, a:{terrain,crowns}, b:{...}}, ...]`.
#[pyfunction]
fn domino_table() -> String {
    let arr: Vec<Value> = DOMINOES
        .iter()
        .map(|d| json!({ "number": d.number, "a": square_json(d.a), "b": square_json(d.b) }))
        .collect();
    Value::Array(arr).to_string()
}

#[pymodule]
fn kingdomino(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Game>()?;
    m.add_function(wrap_pyfunction!(domino_table, m)?)?;
    m.add_function(wrap_pyfunction!(encode_batch, m)?)?;
    m.add_function(wrap_pyfunction!(mcts::selfplay_batch, m)?)?;
    Ok(())
}
