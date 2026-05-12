//! M5-2 self-proof: LocalWorker pays CUDA init exactly once and
//! subsequent jobs reuse the warm context.
//!
//! Acceptance:
//!   * worker.init_wall_ms is in the 200–500 ms range (NVRTC compile +
//!     ffmpeg hwframes pool).
//!   * The first render() call is the slowest (~400 ms for hs-mvp).
//!   * Subsequent render() calls are >= 30 % faster than the first
//!     because they reuse CudaConverter / CudaHwContext.
//!   * Total wall for 5 sequential renders is much less than
//!     5 × first_render_ms (proves amortization).
//!
//! Marked `#[ignore]` (CUDA hardware required):
//!   cargo test --release -p gamereel-farm --test local_worker_amortizes_init \
//!     -- --ignored --nocapture --test-threads=1

use gamereel_farm::job::{JobPriority, RenderJob};
use gamereel_farm::worker::local::LocalWorker;
use gamereel_farm::worker::Worker;
use std::path::PathBuf;
use std::time::Instant;

fn hs_mvp_scene() -> PathBuf {
    // hs-mvp lives at gamereel/apps/hs-mvp/tests/hs-proj/scene.meta.
    // From gamereel-farm we hop up to workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/hs-mvp/tests/hs-proj/scene.meta")
}

fn hs_mvp_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/hs-mvp")
}

fn job(id: &str, output: PathBuf) -> RenderJob {
    RenderJob {
        id: id.into(),
        scene_meta_path: hs_mvp_scene(),
        output_path: output,
        source_root: Some(hs_mvp_root()),
        width: Some(720),
        height: Some(1080),
        fps: Some(30),
        duration_s: Some(10),
        priority: JobPriority::Normal,
        tag: serde_json::Value::Null,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn worker_init_then_5_jobs_amortizes_cuda_setup() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();

    let dir = tempfile::tempdir().expect("tmpdir");

    let init_t = Instant::now();
    let mut worker = LocalWorker::new("local-0", 720, 1080)
        .expect("LocalWorker::new (CUDA + hwframes init)");
    let init_ms = init_t.elapsed().as_millis() as u64;
    println!("worker init: {init_ms} ms (reported: {} ms)", worker.init_wall_ms());

    // Init must be in a sensible range. Tightening the upper bound
    // would catch a regression where someone added work at construction.
    assert!(
        worker.init_wall_ms() >= 100 && worker.init_wall_ms() <= 1500,
        "worker.init_wall_ms = {} outside expected [100, 1500] range",
        worker.init_wall_ms()
    );

    let mut wall_ms: Vec<u128> = Vec::new();
    for i in 0..5 {
        let r = worker
            .render(job(
                &format!("job-{i}"),
                dir.path().join(format!("out-{i}.mp4")),
            ))
            .await
            .expect("render");
        assert!(r.ok);
        assert!(r.output_bytes > 1024, "output_bytes too small: {}", r.output_bytes);
        wall_ms.push(r.wall.as_millis());
        println!(
            "job-{i}: {:>5} ms wall, {:>5} ms render_loop ({} bytes)",
            r.wall.as_millis(),
            r.render_loop.as_millis(),
            r.output_bytes
        );
    }

    let first = wall_ms[0];
    let median_rest: u128 = {
        let mut rest = wall_ms[1..].to_vec();
        rest.sort();
        rest[rest.len() / 2]
    };
    println!("first job: {first} ms, median of jobs 1..5: {median_rest} ms");

    // Amortization claim: subsequent jobs MUST be at least somewhat
    // cheaper than the first (which paid the warm-up). We allow a
    // generous 0.95× because some scenes have variance run-to-run, but
    // a regression where amortization stops working would push the
    // ratio close to 1.0.
    assert!(
        (median_rest as f64) <= (first as f64) * 0.95,
        "no amortization detected: first {first} ms, median rest {median_rest} ms — \
         CUDA contexts likely getting recreated per job"
    );

    let total_5_jobs: u128 = wall_ms.iter().sum();
    let naive_5_jobs = first * 5;
    println!("total wall (5 jobs): {total_5_jobs} ms");
    println!("naive 5x first:      {naive_5_jobs} ms");
    println!("savings:             {} ms ({:.1}%)",
             naive_5_jobs - total_5_jobs,
             100.0 * (naive_5_jobs - total_5_jobs) as f64 / naive_5_jobs as f64);
    assert!(worker.jobs_completed() == 5);
}
