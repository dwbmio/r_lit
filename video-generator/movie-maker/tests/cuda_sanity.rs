//! M3-1 sanity: prove cudarc can JIT-compile a trivial CUDA kernel,
//! launch it on the RTX 3060, and read the result back.
//!
//! This isolates the CUDA toolchain from the rest of the M3 pipeline:
//! if this passes, libcuda.so + libnvrtc.so are reachable, the GPU is
//! addressable, and our cudarc dependency works. If it fails, M3-2..6
//! are blocked at the toolchain layer, not the kernel layer.
//!
//! Marked `#[ignore]` because CI runners without an NVIDIA GPU should
//! not be forced to skip noisily — opt in with:
//!   cargo test -p movie-maker --test cuda_sanity -- --ignored --nocapture

use cudarc::driver::{CudaContext, PushKernelArg};
#[allow(unused_imports)]
use cudarc::nvrtc::Ptx;

const KERNEL: &str = r#"
extern "C" __global__ void add_one(float* x, int n) {
    int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < n) {
        x[i] = x[i] + 1.0f;
    }
}
"#;

#[test]
#[ignore]
fn cudarc_compiles_loads_launches_a_trivial_kernel() {
    // 1) Library loading + device.
    let ctx = CudaContext::new(0).expect("CudaContext::new(0) — driver / device unreachable");
    let stream = ctx.default_stream();
    println!("CUDA device 0 acquired via cudarc::driver");

    // 2) Compile kernel via NVRTC.
    let ptx = cudarc::nvrtc::compile_ptx(KERNEL).expect("nvrtc compile_ptx");
    println!("NVRTC compiled add_one to PTX ({} bytes)", ptx.to_src().len());

    // 3) Load module + function.
    let module = ctx.load_module(ptx).expect("load_module");
    let func = module.load_function("add_one").expect("load_function");

    // 4) Allocate, upload, launch, download.
    let n: usize = 1_000_000;
    let host_in: Vec<f32> = (0..n).map(|i| i as f32).collect();
    let mut dev = stream
        .memcpy_stod(&host_in)
        .expect("memcpy host→device");

    let block = 256u32;
    let grid = ((n as u32) + block - 1) / block;
    let cfg = cudarc::driver::LaunchConfig {
        grid_dim: (grid, 1, 1),
        block_dim: (block, 1, 1),
        shared_mem_bytes: 0,
    };
    unsafe {
        stream
            .launch_builder(&func)
            .arg(&mut dev)
            .arg(&(n as i32))
            .launch(cfg)
            .expect("kernel launch");
    }

    let host_out: Vec<f32> = stream
        .clone_dtoh(&dev)
        .expect("clone device→host");
    stream.synchronize().expect("synchronize");

    // 5) Verify every element got exactly +1.0.
    let mut mismatches = 0usize;
    for (i, (got, want)) in host_out
        .iter()
        .zip(host_in.iter().map(|x| x + 1.0))
        .enumerate()
    {
        if (got - want).abs() > 1e-5 {
            mismatches += 1;
            if mismatches < 5 {
                eprintln!("[{i}] got {got}, want {want}");
            }
        }
    }
    assert_eq!(mismatches, 0, "{mismatches} elements diverged from x+1");
    println!(
        "OK: {n} elements, all got +1.0 ({} block × {} grid)",
        block, grid
    );
}
