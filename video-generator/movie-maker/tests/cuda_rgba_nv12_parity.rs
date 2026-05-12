//! M3-2 self-proof: CUDA `rgba_to_nv12` kernel produces output within
//! NV12 quantization tolerance of CPU `sws_scale`.
//!
//! Why a tolerance rather than byte-equality:
//!   * CPU sws_scale uses fixed-point integer math; our GPU kernel uses
//!     `float` BT.601 coefficients. Rounding diverges at low bits.
//!   * CPU sws_scale interpolates chroma differently than our box-average:
//!     it can apply a small amount of low-pass filtering depending on
//!     the SWS_FLAGS chosen.
//!   * NV12 itself is 8-bit limited-range — there is no "exact answer",
//!     only "within ±N levels of a reference".
//!
//! We allow up to 2 levels of difference per Y / U / V sample and assert
//! the *fraction* of pixels exceeding that bound is < 1%. This is loose
//! enough to absorb the math/interpolation differences but tight enough
//! to fail loudly if the kernel produces outright wrong colors (which
//! would break VMAF).
//!
//! Marked `#[ignore]` so non-GPU CI hosts skip cleanly:
//!   cargo test --release -p movie-maker --test cuda_rgba_nv12_parity \
//!       -- --ignored --nocapture

use ffmpeg_next as ffmpeg;
use movie_maker::cuda_pipeline::CudaConverter;

const W: u32 = 320;
const H: u32 = 240;

fn build_rgba_pattern() -> Vec<u8> {
    // Deterministic gradient with all three channels varied so the test
    // exercises the full BT.601 matrix (not just luma).
    let mut buf = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        for x in 0..W {
            let r = (x % 256) as u8;
            let g = (y % 256) as u8;
            let b = ((x ^ y) % 256) as u8;
            buf.extend_from_slice(&[r, g, b, 255]);
        }
    }
    buf
}

/// Reference: sws_scale RGBA → YUV420P (planar) → repack into NV12.
/// libswscale doesn't directly accept RGBA→NV12 in our build, so we go
/// via YUV420P and interleave U/V into the NV12 UV plane ourselves.
fn cpu_reference_nv12(rgba: &[u8]) -> (Vec<u8>, Vec<u8>) {
    ffmpeg::init().expect("ffmpeg init");
    let mut scaler = ffmpeg::software::scaling::Context::get(
        ffmpeg::format::Pixel::RGBA, W, H,
        ffmpeg::format::Pixel::YUV420P, W, H,
        ffmpeg::software::scaling::Flags::BILINEAR,
    ).expect("scaler");
    let mut src = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, W, H);
    let mut dst = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::YUV420P, W, H);

    // Copy host RGBA → src frame (handle linesize).
    let row_bytes = (W * 4) as usize;
    let stride = src.stride(0);
    let dst_buf = src.data_mut(0);
    if stride == row_bytes {
        dst_buf[..rgba.len()].copy_from_slice(rgba);
    } else {
        for y in 0..H as usize {
            dst_buf[y * stride..y * stride + row_bytes]
                .copy_from_slice(&rgba[y * row_bytes..(y + 1) * row_bytes]);
        }
    }
    scaler.run(&src, &mut dst).expect("scale");

    // Extract Y plane (collapse linesize).
    let y_data = dst.data(0);
    let y_stride = dst.stride(0);
    let mut y_out = Vec::with_capacity((W * H) as usize);
    for y in 0..H as usize {
        y_out.extend_from_slice(&y_data[y * y_stride..y * y_stride + W as usize]);
    }
    // Interleave U and V planes into NV12 UV.
    let u_data = dst.data(1);
    let v_data = dst.data(2);
    let u_stride = dst.stride(1);
    let v_stride = dst.stride(2);
    let mut uv_out = Vec::with_capacity((W * H / 2) as usize);
    for y in 0..(H as usize) / 2 {
        for x in 0..(W as usize) / 2 {
            uv_out.push(u_data[y * u_stride + x]);
            uv_out.push(v_data[y * v_stride + x]);
        }
    }
    (y_out, uv_out)
}

fn diff_stats(reference: &[u8], got: &[u8], label: &str) -> (usize, f64, u8) {
    assert_eq!(reference.len(), got.len(), "{label}: length mismatch");
    let mut over_2 = 0usize;
    let mut max = 0u8;
    let mut sum_abs: u64 = 0;
    for (a, b) in reference.iter().zip(got.iter()) {
        let d = (*a as i32 - *b as i32).unsigned_abs() as u8;
        if d > 2 { over_2 += 1; }
        if d > max { max = d; }
        sum_abs += d as u64;
    }
    let mean_abs = sum_abs as f64 / reference.len() as f64;
    println!(
        "  {label}: {} samples, max diff = {max}, mean |diff| = {mean_abs:.3}, samples >2 = {over_2} ({:.3}%)",
        reference.len(),
        100.0 * over_2 as f64 / reference.len() as f64
    );
    (over_2, mean_abs, max)
}

#[test]
#[ignore]
fn cuda_kernel_matches_sws_scale_within_nv12_tolerance() {
    let rgba = build_rgba_pattern();
    let (ref_y, ref_uv) = cpu_reference_nv12(&rgba);

    let mut conv = CudaConverter::new(W, H).expect("CudaConverter::new");
    conv.convert(&rgba).expect("convert");
    conv.synchronize().expect("sync");
    let gpu_y = conv.read_y_to_host().expect("read y");
    let gpu_uv = conv.read_uv_to_host().expect("read uv");

    println!();
    println!("CPU sws_scale (RGBA→YUV420P) vs GPU rgba_to_nv12 (BT.601 LR):");
    let (y_over, _y_mean, _y_max) = diff_stats(&ref_y, &gpu_y, "Y");
    let (uv_over, _uv_mean, _uv_max) = diff_stats(&ref_uv, &gpu_uv, "UV");

    let total_samples = ref_y.len() + ref_uv.len();
    let total_over = y_over + uv_over;
    let pct = 100.0 * total_over as f64 / total_samples as f64;
    println!("  combined: {total_over}/{total_samples} samples diverge by >2 ({pct:.3}%)");

    assert!(
        pct < 1.0,
        "GPU kernel divergence from CPU reference is {pct:.3}% (> 1% threshold)"
    );
}
