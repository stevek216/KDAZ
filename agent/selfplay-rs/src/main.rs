//! Fully-Rust net self-play (step 3c): the whole loop — games, MCTS trees, encoding, and the
//! GPU forward (libtorch via tch) — runs in Rust, no Python in the hot path. Each round pumps
//! every game one simulation to a leaf (rayon), encodes the leaves (shared `kd-features`
//! encoder), runs one batched forward, gathers per-action logits, and backs up.
//!
//! Build/run (from agent/selfplay-rs):
//!   LIBTORCH_USE_PYTORCH=1 cargo build --release
//!   PATH=<torch/lib>:$PATH  kd-selfplay --model runs/random.ts.pt --games 2000 --sims 128 \
//!        --concurrent 1024 [--out corpus.jsonl] [--no-write]

mod pool;

use std::io::Write;
use std::time::Instant;

use rayon::prelude::*;
use tch::{CModule, Device, IValue, Kind, Tensor};

use kingdomino_engine::core::{Action, Phase, Variants};
use kingdomino_engine::rules::cell_of;
use kingdomino_features::encoder;

use pool::GameSearch;

#[cfg(windows)]
fn force_load_cuda() {
    for dll in ["torch_cuda.dll", "c10_cuda.dll"] {
        unsafe {
            if let Ok(lib) = libloading::Library::new(dll) {
                std::mem::forget(lib);
            }
        }
    }
}
#[cfg(not(windows))]
fn force_load_cuda() {}

fn tuple4(v: IValue) -> (Tensor, Tensor, Tensor, Tensor) {
    match v {
        IValue::Tuple(mut t) => {
            let take = |x: IValue| match x {
                IValue::Tensor(t) => t,
                _ => panic!("expected tensor"),
            };
            let v = take(t.remove(3));
            let dc = take(t.remove(2));
            let cl = take(t.remove(1));
            let pm = take(t.remove(0));
            (pm, cl, dc, v)
        }
        _ => panic!("expected a 4-tuple"),
    }
}

fn to_vec(t: &Tensor) -> Vec<f32> {
    Vec::<f32>::try_from(
        t.to_kind(Kind::Float)
            .to_device(Device::Cpu)
            .contiguous()
            .view([-1]),
    )
    .unwrap()
}

fn softmax(v: &[f32]) -> Vec<f32> {
    let mx = v.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let e: Vec<f32> = v.iter().map(|x| (x - mx).exp()).collect();
    let s: f32 = e.iter().sum::<f32>().max(1e-12);
    e.iter().map(|x| x / s).collect()
}

/// The net's logit for one action, gathered from this leaf's head slices.
fn action_logit(a: Action, phase: Phase, pm: &[f32], cl: &[f32], dc: f32) -> f32 {
    match a {
        Action::Place { anchor, rot } => {
            let (r, c) = cell_of(anchor);
            pm[rot as usize * 169 + r as usize * 13 + c as usize]
        }
        Action::Claim { slot } => {
            let lt = if matches!(phase, Phase::StartClaim) {
                slot as usize
            } else {
                4 + slot as usize
            };
            cl[lt]
        }
        _ => dc,
    }
}

fn arg<T: std::str::FromStr>(args: &[String], key: &str, default: T) -> T {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
fn flag(args: &[String], key: &str) -> bool {
    args.iter().any(|a| a == key)
}

fn main() {
    force_load_cuda();
    let args: Vec<String> = std::env::args().collect();
    let model_path = args
        .iter()
        .position(|a| a == "--model")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "runs/random.ts.pt".to_string());
    let total_games: usize = arg(&args, "--games", 2000);
    let n_sims: u32 = arg(&args, "--sims", 128);
    let concurrent: usize = arg(&args, "--concurrent", 1024);
    let c_puct: f32 = arg(&args, "--c-puct", 1.5);
    let temp_moves: u32 = arg(&args, "--temp-moves", 12);
    let alpha: f32 = arg(&args, "--alpha", 0.3);
    let eps: f32 = arg(&args, "--eps", 0.25);
    let seed: u64 = arg(&args, "--seed", 0);
    let players: u8 = arg(&args, "--players", 2);
    let variants = Variants {
        harmony: !flag(&args, "--no-harmony"),
        middle_kingdom: !flag(&args, "--no-middle-kingdom"),
    };
    let out_path = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let write = !flag(&args, "--no-write") && out_path.is_some();

    let dev = Device::cuda_if_available();
    let dtype = if flag(&args, "--bf16") { Kind::BFloat16 } else { Kind::Float };
    let mut model = CModule::load_on_device(&model_path, dev).expect("load traced model");
    model.set_eval();
    println!(
        "model {model_path}  device {dev:?}  dtype {dtype:?}  games {total_games} sims {n_sims} batch {concurrent}"
    );

    let pc = players as usize;
    let per_board = encoder::board_per_state(pc);
    let glen = encoder::glob_len(pc);
    let slots = concurrent.min(total_games).max(1);
    let mut games: Vec<GameSearch> = (0..slots)
        .map(|s| {
            GameSearch::new(
                seed,
                s,
                slots,
                total_games,
                players,
                variants,
                n_sims,
                c_puct,
                temp_moves,
                alpha,
                eps,
            )
        })
        .collect();

    let mut writer = if write {
        let p = out_path.as_ref().unwrap();
        if let Some(parent) = std::path::Path::new(p).parent() {
            std::fs::create_dir_all(parent).ok();
        }
        Some(std::io::BufWriter::new(
            std::fs::File::create(p).expect("open out"),
        ))
    } else {
        None
    };

    let t0 = Instant::now();
    let (mut total_leaves, mut total_dec, mut last) = (0u64, 0u64, Instant::now());
    loop {
        games.par_iter_mut().for_each(|g| g.pump());

        let mut pending = Vec::new();
        for (slot, g) in games.iter_mut().enumerate() {
            if !g.out_lines.is_empty() {
                total_dec += g.out_lines.len() as u64;
                if let Some(w) = writer.as_mut() {
                    for line in &g.out_lines {
                        writeln!(w, "{line}").ok();
                    }
                }
                g.out_lines.clear();
            }
            if g.has_pending() {
                pending.push(slot);
            }
        }
        if pending.is_empty() {
            if games.iter().all(|g| g.idle) {
                break;
            }
            continue;
        }
        let b = pending.len();
        total_leaves += b as u64;

        // ---- encode leaves (rayon) ----
        let states: Vec<_> = pending.iter().map(|&s| games[s].leaf().gs).collect();
        let mut board = vec![0f32; b * per_board];
        let mut lines = vec![0f32; b * encoder::LINES_LEN];
        let mut glob = vec![0f32; b * glen];
        board
            .par_chunks_mut(per_board)
            .zip(lines.par_chunks_mut(encoder::LINES_LEN))
            .zip(glob.par_chunks_mut(glen))
            .zip(states.par_iter())
            .for_each(|(((bc, lc), gc), gs)| encoder::encode_into(gs, bc, lc, gc));

        // ---- one batched forward (GPU) ----
        let bi = b as i64;
        let np = (pc * encoder::N_PLANES) as i64;
        let board_t = Tensor::from_slice(&board).reshape([bi, np, 13, 13]).to_kind(dtype).to_device(dev);
        let lines_t = Tensor::from_slice(&lines).reshape([bi, 8, encoder::LINE_FEATS as i64]).to_kind(dtype).to_device(dev);
        let glob_t = Tensor::from_slice(&glob).reshape([bi, glen as i64]).to_kind(dtype).to_device(dev);
        let out = model
            .forward_is(&[IValue::Tensor(board_t), IValue::Tensor(lines_t), IValue::Tensor(glob_t)])
            .unwrap();
        let (pm, cl, dc, value) = tuple4(out);
        let (pm_v, cl_v, dc_v, val_v) = (to_vec(&pm), to_vec(&cl), to_vec(&dc), to_vec(&value));

        // ---- gather per-action logits + value, back up (rayon-parallel across games) ----
        let mut k_of = vec![usize::MAX; games.len()];
        for (k, &slot) in pending.iter().enumerate() {
            k_of[slot] = k;
        }
        games.par_iter_mut().enumerate().for_each(|(slot, g)| {
            let k = k_of[slot];
            if k == usize::MAX {
                return;
            }
            let pm_off = &pm_v[k * 676..k * 676 + 676];
            let cl_off = &cl_v[k * 8..k * 8 + 8];
            let phase = g.leaf().gs.phase;
            let logits: Vec<f32> = g
                .leaf()
                .actions
                .iter()
                .map(|&a| action_logit(a, phase, pm_off, cl_off, dc_v[k]))
                .collect();
            let vr = softmax(&val_v[k * pc..k * pc + pc]);
            g.apply_eval(&logits, &vr);
        });

        if last.elapsed().as_secs_f64() > 1.0 {
            let done: usize = games.iter().map(|g| g.completed).sum();
            let el = t0.elapsed().as_secs_f64();
            print!(
                "\r  {done}/{total_games} games | {:.1} games/s | {:.0} leaf-evals/s   ",
                done as f64 / el,
                total_leaves as f64 / el
            );
            std::io::stdout().flush().ok();
            last = Instant::now();
        }
    }
    if let Some(w) = writer.as_mut() {
        w.flush().ok();
    }
    let el = t0.elapsed().as_secs_f64();
    let done: usize = games.iter().map(|g| g.completed).sum();
    println!("\n--- done ---");
    println!("games {done}  decisions {total_dec}  leaf-evals {total_leaves}  elapsed {el:.2}s");
    println!(
        "throughput: {:.2} games/s, {:.0} decisions/s, {:.0} leaf-evals/s",
        done as f64 / el,
        total_dec as f64 / el,
        total_leaves as f64 / el
    );
    if let Some(p) = out_path.filter(|_| write) {
        println!("wrote corpus to {p}");
    }
}
