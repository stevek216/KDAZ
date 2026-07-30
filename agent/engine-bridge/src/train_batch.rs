//! Training-batch encoding straight from packed corpus records.
//!
//! The trainer memory-maps a packed corpus (`kd-features/src/pack.rs`), slices the records for
//! a minibatch, and hands the raw bytes here. This decodes each `GameState`, **re-derives** its
//! legal actions with the engine, and builds exactly the tensors `kdagent.dataset.Batch`
//! carries — the same work the old Python `collate` did per record, but without ever
//! materializing a Python dict, and rayon-parallel across the batch.
//!
//! Re-deriving the actions is what lets a record omit them. The stored `n_actions` is checked
//! against the derived count: a mismatch means the corpus was written by an engine whose action
//! enumeration differs from this build, which would silently misalign every policy target, so
//! it is a hard error rather than a warning.

use numpy::ndarray::{Array1, Array2, Array3, Array4};
use numpy::{IntoPyArray, PyReadonlyArray1, PyReadonlyArray2, PyUntypedArrayMethods};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use rayon::prelude::*;

use kingdomino_engine::core::{legal_actions, Action, GameState, Phase};
use kingdomino_engine::rules::cell_of;
use kingdomino_features::{encoder, pack};

/// Policy-head action descriptor `(type, place_idx, claim_line_tok)`; type 0=place, 1=claim,
/// 2=discard. Must match `batch_selfplay::action_descriptor` and `kdagent.encoder`.
fn descriptor(a: Action, phase: Phase) -> (i8, i16, i8) {
    match a {
        Action::Place { anchor, rot } => {
            let (r, c) = cell_of(anchor);
            (0, rot as i16 * 169 + r as i16 * 13 + c as i16, 0)
        }
        Action::Claim { slot } => {
            let lt = if matches!(phase, Phase::StartClaim) {
                slot as i8
            } else {
                4 + slot as i8
            };
            (1, 0, lt)
        }
        _ => (2, 0, 0),
    }
}

/// One packed record as the legacy JSONL object (minus `policy`, which the caller holds
/// separately): `{obs, legal, to_act, value, scores, game}`.
///
/// Packed records are not human-readable, so this is both the inspection tool and the bridge
/// used to prove the packed path builds the same batches the JSON path did.
#[pyfunction]
pub fn packed_record_json(record: PyReadonlyArray1<'_, u8>) -> PyResult<String> {
    let rec = record
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("record must be C-contiguous"))?;
    let gs = pack::unpack_state(rec);
    let (value, scores, has_scores, _) = pack::unpack_targets(rec);
    let pc = gs.player_count as usize;
    let mut buf = Vec::new();
    legal_actions(&gs, &mut buf);
    let vals: Vec<f32> = value[..pc].to_vec();
    let scs: Vec<f32> = scores[..pc].to_vec();
    let scores_field = if has_scores {
        serde_json::to_string(&scs).unwrap()
    } else {
        "null".to_string()
    };
    Ok(format!(
        "{{\"obs\":{},\"legal\":{},\"to_act\":{},\"value\":{},\"scores\":{},\"game\":{}}}",
        crate::obs_json(&gs),
        crate::legal_json(&buf),
        gs.to_act,
        serde_json::to_string(&vals).unwrap(),
        scores_field,
        pack::record_game_id(rec),
    ))
}

/// Encode a minibatch of packed records into the training tensors.
///
/// `records` is `[B, record_size]` (C-contiguous), `policy` the concatenated visit
/// distributions for exactly these records, and `offsets` the `[B + 1]` cumulative index into
/// `policy`. Returns a dict with `board`, `lines`, `glob`, `a_type`, `a_pidx`, `a_ltok`,
/// `a_mask`, `policy`, `value_rel`, `score_rel`, `score_mask`, `root_rel`, `root_mask`, `pc` —
/// value/score/root vectors are seat-relative (the acting seat first); scores are raw totals,
/// the caller scales them. `root_rel` is the search's backed-up root value and `root_mask` is 0
/// for v1 corpora, which predate it.
#[pyfunction]
pub fn encode_packed_batch<'py>(
    py: Python<'py>,
    records: PyReadonlyArray2<'py, u8>,
    policy: PyReadonlyArray1<'py, f32>,
    offsets: PyReadonlyArray1<'py, i64>,
) -> PyResult<Bound<'py, PyDict>> {
    let flat = records.as_slice().map_err(|_| {
        pyo3::exceptions::PyValueError::new_err(
            "records must be C-contiguous (use np.ascontiguousarray)",
        )
    })?;
    let pol_all = policy
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("policy must be C-contiguous"))?;
    let offs = offsets
        .as_slice()
        .map_err(|_| pyo3::exceptions::PyValueError::new_err("offsets must be C-contiguous"))?;

    let b = records.shape()[0];
    let d = PyDict::new_bound(py);
    if b == 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "encode_packed_batch got an empty batch",
        ));
    }
    if offs.len() != b + 1 {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "offsets must have B+1 = {} entries, got {}",
            b + 1,
            offs.len()
        )));
    }
    let rsize = records.shape()[1];

    // Decode states and re-derive the action lists (the corpus stores neither).
    let mut states: Vec<GameState> = Vec::with_capacity(b);
    let mut acts: Vec<Vec<Action>> = Vec::with_capacity(b);
    let mut buf = Vec::new();
    for i in 0..b {
        let rec = &flat[i * rsize..(i + 1) * rsize];
        let gs = pack::unpack_state(rec);
        legal_actions(&gs, &mut buf);
        let stored = pack::record_n_actions(rec) as usize;
        if stored != buf.len() {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "record {i}: corpus says {stored} legal actions, this engine derives {}. The \
                 corpus was generated by a different action enumeration; regenerate it.",
                buf.len()
            )));
        }
        if offs[i + 1] - offs[i] != stored as i64 {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "record {i}: policy slice is {} long but the record has {stored} actions",
                offs[i + 1] - offs[i]
            )));
        }
        states.push(gs);
        acts.push(buf.clone());
    }

    let pc = states[0].player_count as usize;
    if let Some(bad) = states.iter().position(|s| s.player_count as usize != pc) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "record {bad} has player_count {} but the batch is {pc}p",
            states[bad].player_count
        )));
    }
    let amax = acts.iter().map(|a| a.len()).max().unwrap_or(1).max(1);

    // Action descriptors + padded policy targets.
    let mut a_type = vec![-1i8; b * amax];
    let mut a_pidx = vec![0i16; b * amax];
    let mut a_ltok = vec![0i8; b * amax];
    let mut a_mask = vec![0u8; b * amax];
    let mut pol = vec![0f32; b * amax];
    for (i, (gs, actions)) in states.iter().zip(&acts).enumerate() {
        for (j, &act) in actions.iter().enumerate() {
            let (t, p, l) = descriptor(act, gs.phase);
            a_type[i * amax + j] = t;
            a_pidx[i * amax + j] = p;
            a_ltok[i * amax + j] = l;
            a_mask[i * amax + j] = 1;
        }
        let src = &pol_all[offs[i] as usize..offs[i + 1] as usize];
        pol[i * amax..i * amax + src.len()].copy_from_slice(src);
    }

    // Seat-relative value / score targets (acting seat first), rotated like the encoder's
    // per-seat board planes so target and input agree on "self".
    let mut value_rel = vec![0f32; b * pc];
    let mut score_rel = vec![0f32; b * pc];
    let mut score_mask = vec![0u8; b];
    let mut root_rel = vec![0f32; b * pc];
    let mut root_mask = vec![0u8; b];
    for (i, gs) in states.iter().enumerate() {
        let rec = &flat[i * rsize..(i + 1) * rsize];
        let (value, scores, has_scores, _) = pack::unpack_targets(rec);
        let root = pack::unpack_root_value(rec); // None for v1 corpora
        let ta = gs.to_act as usize;
        for k in 0..pc {
            value_rel[i * pc + k] = value[(ta + k) % pc];
            score_rel[i * pc + k] = scores[(ta + k) % pc];
            if let Some(rv) = root {
                root_rel[i * pc + k] = rv[(ta + k) % pc];
            }
        }
        score_mask[i] = has_scores as u8;
        root_mask[i] = root.is_some() as u8;
    }

    // Board / line / global planes, parallel across the batch (the bulk of the work).
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
            .for_each(|(((bc, lc), gc), gs)| {
                encoder::encode_board(gs, bc, 1.0f32);
                encoder::encode_aux(gs, lc, gc);
            });
    });

    d.set_item(
        "board",
        Array4::from_shape_vec(
            (b, pc * encoder::N_PLANES, encoder::STORE, encoder::STORE),
            board,
        )
        .unwrap()
        .into_pyarray_bound(py),
    )?;
    d.set_item(
        "lines",
        Array3::from_shape_vec((b, 8, encoder::LINE_FEATS), lines)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "glob",
        Array2::from_shape_vec((b, glen), glob)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "a_type",
        Array2::from_shape_vec((b, amax), a_type)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "a_pidx",
        Array2::from_shape_vec((b, amax), a_pidx)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "a_ltok",
        Array2::from_shape_vec((b, amax), a_ltok)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "a_mask",
        Array2::from_shape_vec((b, amax), a_mask)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "policy",
        Array2::from_shape_vec((b, amax), pol)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "value_rel",
        Array2::from_shape_vec((b, pc), value_rel)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "score_rel",
        Array2::from_shape_vec((b, pc), score_rel)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "score_mask",
        Array1::from_vec(score_mask).into_pyarray_bound(py),
    )?;
    d.set_item(
        "root_rel",
        Array2::from_shape_vec((b, pc), root_rel)
            .unwrap()
            .into_pyarray_bound(py),
    )?;
    d.set_item(
        "root_mask",
        Array1::from_vec(root_mask).into_pyarray_bound(py),
    )?;
    d.set_item("pc", pc)?;
    Ok(d)
}
