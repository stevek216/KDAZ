//! Minimal tch smoke test: confirm libtorch links, CUDA is available, and a tensor op runs
//! on the GPU. The full self-play loop is built on top once this proves the toolchain works.

use tch::{Device, Kind, Tensor};

/// Force-load torch_cuda.dll (and friends) so libtorch registers its CUDA backend on Windows.
/// Must run before any tch CUDA query. `torch/lib` must be on PATH so the deps resolve.
#[cfg(windows)]
fn force_load_cuda() {
    for dll in ["torch_cuda.dll", "c10_cuda.dll"] {
        unsafe {
            match libloading::Library::new(dll) {
                Ok(lib) => std::mem::forget(lib), // keep it loaded for the process lifetime
                Err(e) => eprintln!("warn: could not load {dll}: {e}"),
            }
        }
    }
}
#[cfg(not(windows))]
fn force_load_cuda() {}

fn main() {
    force_load_cuda();
    println!("libtorch via tch — cuda available: {}", tch::Cuda::is_available());
    let dev = Device::cuda_if_available();
    println!("device: {dev:?}");
    let t = Tensor::randn([1024, 26, 13, 13], (Kind::Float, dev));
    let conv = Tensor::randn([64, 26, 3, 3], (Kind::Float, dev));
    let out = t.conv2d(&conv, None::<Tensor>, [1], [1], [1], 1);
    tch::Cuda::synchronize(0);
    println!("conv2d out shape: {:?}  mean: {:.5}", out.size(), f64::try_from(out.mean(Kind::Float)).unwrap());
    println!("OK");
}
