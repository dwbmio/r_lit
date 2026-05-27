//! Hardware probe — figures out a sane worker pool size for the box
//! we're running on. Used by `Supervisor` at startup; can be overridden
//! by env var `GAMEREEL_WORKERS`.
//!
//! Strategy: shell out to `nvidia-smi` (avoids the nvml-wrapper dep
//! which would pull in another link to libnvidia-ml.so). If nvidia-smi
//! is missing or fails, fall back to a conservative default of 2.
//! This keeps `gamereel-farm` building cleanly on machines without
//! NVIDIA at all.

use std::process::Command;

#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub driver_version: String,
}

/// Returns the first NVIDIA GPU found, or None if nvidia-smi is
/// unavailable / no GPUs detected.
pub fn probe_first_gpu() -> Option<GpuInfo> {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
            "-i", "0",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let mut parts = line.trim().split(',').map(|s| s.trim());
    Some(GpuInfo {
        name: parts.next()?.to_string(),
        vram_total_mb: parts.next()?.parse().ok()?,
        vram_free_mb: parts.next()?.parse().ok()?,
        driver_version: parts.next()?.to_string(),
    })
}

/// Decide a worker pool size based on probed GPU. Order of precedence:
///   1. `GAMEREEL_WORKERS` env var if set and parseable.
///   2. `gpu` is Some → consult [`workers_for_gpu`].
///   3. Fallback to 1 (CPU-only or unprobed environment).
pub fn recommended_worker_count(gpu: Option<&GpuInfo>) -> usize {
    if let Ok(s) = std::env::var("GAMEREEL_WORKERS") {
        if let Ok(n) = s.parse::<usize>() {
            if n >= 1 {
                log::info!("recommended_worker_count: GAMEREEL_WORKERS={n} (override)");
                return n;
            }
        }
    }
    let n = gpu.map(workers_for_gpu).unwrap_or(1);
    log::info!(
        "recommended_worker_count: {n} (gpu={:?})",
        gpu.map(|g| g.name.as_str())
    );
    n
}

/// Per-GPU **throughput-optimized** worker count.
///
/// Numbers calibrated from `apps/hs-mvp/src/bin/farm_bench.rs` 100-job
/// runs. Headline result on the reference machine (RTX 3060):
///
///   workers=1  → 1004 fps,  p99  290 ms  (latency-best)
///   workers=2  → 1113 fps,  p99  547 ms  (throughput-best, +10%)
///   workers=4  → 1095 fps,  p99 1101 ms  (worse — NVENC saturated, p99 doubled)
///   workers=8  → 1062 fps,  p99 2329 ms  (worse — pure queue thrash)
///
/// On single-NVENC consumer GPUs the throughput peak is at workers=2.
/// Going higher buys nothing in fps and pays for it linearly in p99
/// latency — a really bad trade for "near-real-time" use cases.
///
/// For latency-sensitive workloads (sub-300ms per-video budget) call
/// [`workers_for_latency`] instead, which always returns 1.
pub fn workers_for_gpu(gpu: &GpuInfo) -> usize {
    let name = gpu.name.to_lowercase();
    // Single-NVENC consumer chips: throughput peak at 2.
    if name.contains("rtx 3060") || name.contains("rtx 4060") {
        return 2;
    }
    if name.contains("rtx 3070") || name.contains("rtx 4070")
        && !name.contains("ti") && !name.contains("super") {
        return 2;
    }
    // Dual-NVENC chips: untested, conservatively 4.
    if name.contains("rtx 4070 ti") || name.contains("rtx 4080") || name.contains("rtx 4090") {
        return 4;
    }
    if name.contains("a4000") || name.contains("a5000") || name.contains("a6000") {
        return 4;
    }
    // Unknown model → conservative 2.
    2
}

/// Latency-optimized companion to [`workers_for_gpu`]. Returns 1 always;
/// guarantees no queue waiting (each job goes to a worker that's
/// already idle by definition). Use this for "near-real-time" use
/// cases where p99 < 300 ms matters more than total throughput.
pub fn workers_for_latency(_gpu: &GpuInfo) -> usize { 1 }
