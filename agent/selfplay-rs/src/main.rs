//! Step 3b: load the traced net (TorchScript), parity-check against PyTorch's saved output,
//! and measure the libtorch forward throughput in Rust. The full self-play loop builds on this.
//!
//! Build/run (from agent/selfplay-rs):
//!   LIBTORCH_USE_PYTORCH=1 cargo build --release
//!   PATH=<torch/lib>:$PATH  kd-selfplay <traced.ts.pt> <parity_dir>

use std::time::Instant;

use tch::{CModule, Device, IValue, Kind, Tensor};

/// Force-load torch_cuda.dll so libtorch registers its CUDA backend on Windows.
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
                _ => panic!("expected tensor in tuple"),
            };
            let v = take(t.remove(3));
            let dc = take(t.remove(2));
            let cl = take(t.remove(1));
            let pm = take(t.remove(0));
            (pm, cl, dc, v)
        }
        _ => panic!("expected a 4-tuple from the traced model"),
    }
}

fn max_abs_diff(a: &Tensor, b: &Tensor) -> f64 {
    f64::try_from((a - b).abs().max()).unwrap()
}

fn main() {
    force_load_cuda();
    let args: Vec<String> = std::env::args().collect();
    let model_path = args.get(1).map(String::as_str).unwrap_or("runs/traced.ts.pt");
    let parity_dir = args.get(2).map(String::as_str).unwrap_or("runs/parity");

    let dev = Device::cuda_if_available();
    println!("device: {dev:?}  (cuda: {})", tch::Cuda::is_available());

    let mut model = CModule::load_on_device(model_path, dev).expect("load traced model");
    model.set_eval();

    let load = |n: &str| Tensor::read_npy(format!("{parity_dir}/{n}.npy")).unwrap().to_device(dev);
    let board = load("board");
    let lines = load("lines");
    let glob = load("glob");

    // ---- parity vs PyTorch ----
    let out = model
        .forward_is(&[IValue::Tensor(board.shallow_clone()), IValue::Tensor(lines.shallow_clone()),
                      IValue::Tensor(glob.shallow_clone())])
        .unwrap();
    let (pm, cl, dc, v) = tuple4(out);
    let d_pm = max_abs_diff(&pm, &load("place_map"));
    let d_cl = max_abs_diff(&cl, &load("claim_logits"));
    let d_dc = max_abs_diff(&dc, &load("discard"));
    let d_v = max_abs_diff(&v, &load("value"));
    let worst = d_pm.max(d_cl).max(d_dc).max(d_v);
    println!("parity max|Δ|: place {d_pm:.2e} claim {d_cl:.2e} discard {d_dc:.2e} value {d_v:.2e}");
    assert!(worst < 1e-4, "parity failed: max diff {worst:.2e}");
    println!("parity OK (< 1e-4)");

    // ---- throughput at a few batch sizes ----
    println!("\nforward throughput (fp32, resident inputs):");
    for &b in &[256usize, 512, 1024, 2048] {
        let board = Tensor::rand([b as i64, board.size()[1], 13, 13], (Kind::Float, dev));
        let lines = Tensor::rand([b as i64, 8, lines.size()[2]], (Kind::Float, dev));
        let glob = Tensor::rand([b as i64, glob.size()[1]], (Kind::Float, dev));
        let inp = |a: &Tensor| IValue::Tensor(a.shallow_clone());
        let run = || {
            let _ = model.forward_is(&[inp(&board), inp(&lines), inp(&glob)]).unwrap();
        };
        for _ in 0..10 {
            run();
        }
        tch::Cuda::synchronize(0);
        let t0 = Instant::now();
        let iters = 200;
        for _ in 0..iters {
            run();
        }
        tch::Cuda::synchronize(0);
        let ms = t0.elapsed().as_secs_f64() / iters as f64 * 1000.0;
        println!("  batch {b:>4}: {ms:6.3} ms/batch   ({:>10.0} samples/s)", b as f64 / (ms / 1000.0));
    }
}
