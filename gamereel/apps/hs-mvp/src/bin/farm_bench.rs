//! M5 farm benchmark: how does throughput scale with worker count on
//! the actual hs-mvp scene?
//!
//! Sweeps worker counts and renders N jobs per sweep. For each sweep:
//!   * total wall time
//!   * per-job mean / median / p99 wall (queue + render)
//!   * throughput in fps and videos/min
//!
//! Output: stdout table + /tmp/m5-farm-bench.json for diff.
//!
//! Usage:
//!   farm_bench [--jobs N] [--workers w1,w2,...] [--out path]
//! Defaults: --jobs 30 --workers 1,2,4,6,8 --out /tmp/m5-farm-bench.json

use gamereel_farm::{
    job::{JobPriority, RenderJob},
    pool::WorkerPool,
    probe::{probe_first_gpu, workers_for_gpu},
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn parse_args() -> (usize, Vec<usize>, PathBuf) {
    let mut jobs = 30usize;
    let mut workers = vec![1, 2, 4, 6, 8];
    let mut out = PathBuf::from("/tmp/m5-farm-bench.json");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--jobs" => {
                jobs = args[i + 1].parse().expect("--jobs N");
                i += 2;
            }
            "--workers" => {
                workers = args[i + 1].split(',').map(|s| s.parse().unwrap()).collect();
                i += 2;
            }
            "--out" => {
                out = PathBuf::from(&args[i + 1]);
                i += 2;
            }
            other => {
                eprintln!("unknown arg: {other}");
                std::process::exit(2);
            }
        }
    }
    (jobs, workers, out)
}

fn make_jobs(count: usize, scene: &PathBuf, root: &PathBuf, tmp_dir: &PathBuf) -> Vec<RenderJob> {
    (0..count)
        .map(|i| RenderJob {
            id: format!("job-{i:03}"),
            scene_meta_path: scene.clone(),
            output_path: tmp_dir.join(format!("out-{i:03}.mp4")),
            source_root: Some(root.clone()),
            width: Some(720),
            height: Some(1080),
            fps: Some(30),
            duration_s: Some(10),
            priority: JobPriority::Normal,
            tag: serde_json::Value::Null,
        })
        .collect()
}

#[derive(serde::Serialize)]
struct SweepResult {
    workers: usize,
    jobs: usize,
    wall_ms: u128,
    fps_e2e: f64,
    videos_per_min: f64,
    median_job_wall_ms: u128,
    p99_job_wall_ms: u128,
    mean_render_loop_ms: f64,
    succeeded: usize,
    failed: usize,
}

fn run_sweep(workers: usize, jobs: Vec<RenderJob>) -> SweepResult {
    let n = jobs.len();
    println!("\n--- sweep workers={workers} jobs={n} ---");
    let t = Instant::now();
    let mut pool = WorkerPool::spawn(workers, 720, 1080).expect("pool spawn");

    // Submit + collect interleaved so the pool stays full.
    let mut submitted = 0usize;
    let mut collected = 0usize;
    let mut wall_per_job: Vec<u128> = Vec::with_capacity(n);
    let mut render_loop_per_job: Vec<u128> = Vec::with_capacity(n);
    let mut failed = 0usize;

    let mut iter = jobs.into_iter();
    let mut pending: Option<RenderJob> = None; // job pulled from iter but not yet submitted
    while collected < n {
        // Try to fill the pool.
        loop {
            if pending.is_none() && submitted < n {
                pending = iter.next();
            }
            let Some(job) = pending.as_ref() else { break };
            match pool.submit(job) {
                Ok(()) => {
                    submitted += 1;
                    pending = None;
                }
                Err(_) => break, // queue full, collect first
            }
        }
        // Block for a result.
        match pool.collect_with_timeout(Duration::from_secs(60)).expect("collect") {
            Some(Ok(r)) => {
                wall_per_job.push(r.wall.as_millis());
                render_loop_per_job.push(r.render_loop.as_millis());
                collected += 1;
            }
            Some(Err(e)) => {
                eprintln!("  ✗ {e}");
                failed += 1;
                collected += 1;
            }
            None => panic!("collect_with_timeout: 60s elapsed without result"),
        }
        if collected % 10 == 0 || collected == n {
            print!("\r  progress: {}/{} ({} failed)        ", collected, n, failed);
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
    }
    println!();
    pool.shutdown().expect("shutdown");

    let wall = t.elapsed();
    wall_per_job.sort();
    let p99_idx = ((wall_per_job.len() as f64) * 0.99) as usize;
    let p99 = wall_per_job.get(p99_idx.saturating_sub(1)).copied().unwrap_or(0);
    let median = wall_per_job.get(wall_per_job.len() / 2).copied().unwrap_or(0);
    let total_frames = n as f64 * 300.0;
    let fps = total_frames / wall.as_secs_f64();
    let videos_per_min = (n as f64) * 60.0 / wall.as_secs_f64();
    let mean_render_loop_ms = if render_loop_per_job.is_empty() {
        0.0
    } else {
        render_loop_per_job.iter().sum::<u128>() as f64 / render_loop_per_job.len() as f64
    };

    let result = SweepResult {
        workers,
        jobs: n,
        wall_ms: wall.as_millis(),
        fps_e2e: fps,
        videos_per_min,
        median_job_wall_ms: median,
        p99_job_wall_ms: p99,
        mean_render_loop_ms,
        succeeded: n - failed,
        failed,
    };
    println!(
        "  wall={} ms  →  {:.1} fps e2e  ({:.0} videos/min)  median {} ms  p99 {} ms",
        result.wall_ms, result.fps_e2e, result.videos_per_min, result.median_job_wall_ms, result.p99_job_wall_ms,
    );
    result
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .init();

    let (jobs_per_sweep, worker_counts, out_path) = parse_args();
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let scene = project_root.join("tests/hs-proj/scene.meta");
    let tmp = std::env::temp_dir().join("m5-farm-bench");
    std::fs::create_dir_all(&tmp).expect("mkdir tmp");

    println!("M5 farm bench");
    println!("  scene:        {}", scene.display());
    println!("  jobs/sweep:   {jobs_per_sweep}");
    println!("  worker sweep: {worker_counts:?}");
    println!("  output mp4s:  {} (cleaned at end)", tmp.display());
    if let Some(g) = probe_first_gpu() {
        println!(
            "  detected gpu: {} (vram {} MB free / {} MB total, driver {})",
            g.name, g.vram_free_mb, g.vram_total_mb, g.driver_version,
        );
        println!("  recommended worker count for this gpu: {}", workers_for_gpu(&g));
    }

    let mut sweeps: Vec<SweepResult> = Vec::new();
    for w in worker_counts {
        let jobs = make_jobs(jobs_per_sweep, &scene, &project_root, &tmp);
        let r = run_sweep(w, jobs);
        sweeps.push(r);
    }

    // Print table.
    println!();
    println!("================================================================");
    println!("M5 farm bench — hs-mvp scene, 720x1080×30fps×10s per job");
    println!("================================================================");
    println!(
        "{:>8}  {:>6}  {:>10}  {:>10}  {:>12}  {:>10}  {:>10}",
        "workers", "jobs", "wall(ms)", "fps_e2e", "videos/min", "p50(ms)", "p99(ms)",
    );
    for r in &sweeps {
        println!(
            "{:>8}  {:>6}  {:>10}  {:>10.1}  {:>12.1}  {:>10}  {:>10}",
            r.workers, r.jobs, r.wall_ms, r.fps_e2e, r.videos_per_min, r.median_job_wall_ms, r.p99_job_wall_ms,
        );
    }

    let json = serde_json::json!({
        "scene": "hs-mvp 720x1080 × 30fps × 10s",
        "jobs_per_sweep": jobs_per_sweep,
        "gpu": probe_first_gpu().map(|g| serde_json::json!({
            "name": g.name,
            "vram_total_mb": g.vram_total_mb,
            "driver_version": g.driver_version,
        })),
        "sweeps": sweeps,
    });
    std::fs::write(&out_path, serde_json::to_string_pretty(&json).unwrap()).expect("write json");
    println!("\nwrote {}", out_path.display());

    // Cleanup temp mp4s.
    for entry in std::fs::read_dir(&tmp).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(entry.path());
    }
}
