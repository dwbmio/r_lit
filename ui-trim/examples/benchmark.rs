use serde::Serialize;
use std::time::Instant;
use ui_trim::{default_options, make_synthetic_ui_asset, trim_image};

#[derive(Serialize)]
struct BenchCase {
    width: u32,
    height: u32,
    iterations: u32,
    avg_total_ms: f64,
    avg_algorithm_ms: f64,
    avg_throughput_mp_s: f64,
    last_removed_pixels: u64,
    last_alpha_ratio: f32,
}

fn main() -> ui_trim::Result<()> {
    let mut cases = Vec::new();
    for (w, h, iterations) in [(512, 512, 30), (1024, 1024, 15), (2048, 2048, 5)] {
        cases.push(run_case(w, h, iterations)?);
    }
    println!("{}", serde_json::to_string_pretty(&cases).expect("json"));
    Ok(())
}

fn run_case(w: u32, h: u32, iterations: u32) -> ui_trim::Result<BenchCase> {
    let options = default_options();
    let mut total_ms = 0.0;
    let mut algorithm_ms = 0.0;
    let mut throughput = 0.0;
    let mut last_removed_pixels = 0;
    let mut last_alpha_ratio = 0.0;

    for _ in 0..iterations {
        let img = make_synthetic_ui_asset(w, h);
        let start = Instant::now();
        let (_out, report) = trim_image(img, &options)?;
        total_ms += start.elapsed().as_secs_f64() * 1000.0;
        algorithm_ms += report.timings_ms.total;
        throughput += report.throughput_mp_s;
        last_removed_pixels = report.removed_pixels;
        last_alpha_ratio = report.alpha_ratio;
    }

    Ok(BenchCase {
        width: w,
        height: h,
        iterations,
        avg_total_ms: total_ms / f64::from(iterations),
        avg_algorithm_ms: algorithm_ms / f64::from(iterations),
        avg_throughput_mp_s: throughput / f64::from(iterations),
        last_removed_pixels,
        last_alpha_ratio,
    })
}
