//! Instrumented hs-mvp run — captures a phase-level + per-frame timeline
//! of the actual demo workload (not perf_main synthetic).
//!
//! Output format:
//!   [t=  0 ms]  init
//!     init.ffmpeg                            5 ms
//!     init.tokio                             1 ms
//!     scene.import_meta                     12 ms
//!     report.gen_dynamic_images             87 ms
//!       └ load_loc_image x1                  4 ms
//!       └ gen_bans_image x3                 24 ms
//!       └ gen_block_image x9                52 ms
//!     report.gen_nodes                       2 ms
//!     scene.preload                         15 ms
//!     scene.on_init                          8 ms
//!     [encoder loop starts, 300 frames]
//!     ...
//!     encoder.flush                         12 ms
//!   total                                 1043 ms
//!
//!   per-frame breakdown (median across 300 frames):
//!     scene.on_render                        2.40 ms  (cpu compositing)
//!     copy_rgba_to_frame                     0.20 ms
//!     scaler.run (sws RGBA→YUV)              1.05 ms  (CPU SIMD)
//!     encoder.send_frame                     0.05 ms
//!     drain_packets                          0.02 ms
//!     ─────────────────────────────────────
//!     total per-frame                        3.72 ms  ≈ 269 fps ceiling
//!
//! Also writes JSON to /tmp/hs-mvp-trace.json for diffing across runs.

use ffmpeg_next as ffmpeg;
use ffmpeg::codec;
use gamereel_core::ffmpeg_inc::encoder_pick::{pick_h264_encoder, EncoderPreference};
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::stage;
use gamereel_core::{ffmpeg_inc as gci, RuntimeCtx};
use image::GenericImageView;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// We can't import the demo's report/nodes mods from a `bin/` target
// because Cargo bins don't see other bins' source. Re-include them via
// `#[path]` so this trace binary uses exactly the same workload as
// the production hs-mvp main.rs. The crate root is apps/hs-mvp/src/.
#[path = "../nodes/mod.rs"]
mod nodes;
#[path = "../report/mod.rs"]
mod report;

#[derive(Default)]
struct PhaseLog {
    label: String,
    elapsed: Duration,
    children: Vec<PhaseLog>,
}

impl PhaseLog {
    fn print(&self, depth: usize) {
        let indent = if depth == 0 { "  " } else { "    " };
        let prefix = if depth > 1 { "└ " } else { "" };
        println!(
            "{indent}{:width$}{prefix}{:<35}{:>10.2} ms",
            "",
            self.label,
            self.elapsed.as_secs_f64() * 1000.0,
            width = depth.saturating_sub(1) * 2,
        );
        for c in &self.children {
            c.print(depth + 1);
        }
    }
}

struct FrameTimings {
    on_render: Duration,
    copy_rgba: Duration,
    scaler_run: Duration,
    send_frame: Duration,
    drain: Duration,
}

fn median<T: Ord + Copy>(mut v: Vec<T>) -> T {
    v.sort();
    v[v.len() / 2]
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let project_root = env!("CARGO_MANIFEST_DIR");
    let out_mp4: PathBuf = Path::new(project_root)
        .join("tests/hs-proj/output_trace.mp4");

    let total_start = Instant::now();
    let mut phases: Vec<PhaseLog> = Vec::new();

    // -------- init.ffmpeg --------
    let t = Instant::now();
    gci::init_env().expect("ffmpeg init");
    phases.push(PhaseLog { label: "init.ffmpeg".into(), elapsed: t.elapsed(), children: vec![] });

    // -------- init.tokio --------
    let t = Instant::now();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio");
    phases.push(PhaseLog { label: "init.tokio".into(), elapsed: t.elapsed(), children: vec![] });

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    rtx.set_source_path(Path::new(project_root).to_path_buf());

    let frame_timings: Vec<FrameTimings> = rt.block_on(async {
        // -------- scene.import_meta --------
        let t = Instant::now();
        let scene_meta = stage::import_scene(
            Path::new(project_root).join("tests/hs-proj/scene.meta"),
        )
        .await
        .expect("import");
        phases.push(PhaseLog { label: "scene.import_meta".into(), elapsed: t.elapsed(), children: vec![] });

        // -------- report.gen_dynamic_images --------
        let t = Instant::now();
        let report = report::Report::new();
        report.gen_report_dynamic_images(&mut rtx);
        phases.push(PhaseLog { label: "report.gen_dynamic_images".into(), elapsed: t.elapsed(), children: vec![] });

        // -------- report.gen_nodes --------
        let t = Instant::now();
        let mut stage_mgr = StageMgr::new(scene_meta);
        report.gen_nodes(&mut rtx, &mut stage_mgr.scenes_meta.meta_scene_list[0]);
        phases.push(PhaseLog { label: "report.gen_nodes".into(), elapsed: t.elapsed(), children: vec![] });

        // -------- scene.preload --------
        let t = Instant::now();
        stage_mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
        phases.push(PhaseLog { label: "scene.preload".into(), elapsed: t.elapsed(), children: vec![] });

        let scene = stage_mgr
            .scenes
            .values_mut()
            .next()
            .expect("at least one scene");

        // -------- scene.on_init --------
        let t = Instant::now();
        scene.on_init(&rtx);
        phases.push(PhaseLog { label: "scene.on_init".into(), elapsed: t.elapsed(), children: vec![] });

        // -------- encoder loop (300 frames, fully timed) --------
        let mut octx = ffmpeg::format::output(&out_mp4).expect("output");
        let global_header = octx
            .format()
            .flags()
            .contains(ffmpeg::format::Flags::GLOBAL_HEADER);

        let choice = pick_h264_encoder(EncoderPreference::AutoBalanced).expect("encoder");
        let codec_h264 = codec::encoder::find_by_name(choice.codec_name).expect("find codec");
        let mut ost = octx.add_stream(codec_h264).expect("add stream");
        let mut enc = codec::context::Context::new_with_codec(codec_h264)
            .encoder()
            .video()
            .expect("video encoder");
        enc.set_width(720);
        enc.set_height(1080);
        enc.set_format(choice.pixel_format);
        enc.set_frame_rate(Some((30, 1)));
        enc.set_time_base(ffmpeg::Rational(1, 30));
        if global_header {
            enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
        }
        let mut opts = ffmpeg::Dictionary::new();
        for (k, v) in &choice.opts {
            opts.set(k, v);
        }
        let mut cc = enc.open_with(opts).expect("open enc");
        ost.set_parameters(&cc);
        ost.set_time_base(ffmpeg::Rational(1, 30));
        octx.write_header().expect("header");
        let stream_tb = octx.stream(0).expect("stream").time_base();

        // probe first frame to size the scaler.
        let probe = scene.on_render(&mut rtx, 0.0).expect("render");
        let (sw, sh) = probe.dimensions();

        let mut scaler = ffmpeg::software::scaling::Context::get(
            ffmpeg::format::Pixel::RGBA, sw, sh,
            choice.pixel_format, 720, 1080,
            ffmpeg::software::scaling::Flags::BILINEAR,
        ).expect("scaler");
        let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, sw, sh);
        let mut yuv = ffmpeg::frame::Video::new(choice.pixel_format, 720, 1080);

        let mut timings: Vec<FrameTimings> = Vec::with_capacity(300);

        for f in 0..300u32 {
            // --- on_render ---
            let t1 = Instant::now();
            let img = if f == 0 {
                probe.clone()
            } else {
                scene.on_render(&mut rtx, f as f32 / 30.0).expect("render")
            };
            let on_render = t1.elapsed();

            // --- copy_rgba ---
            let t2 = Instant::now();
            let raw = img.to_rgba8();
            let raw_bytes = raw.as_raw();
            let stride = rgba.stride(0);
            let row_bytes = (sw as usize) * 4;
            let dst = rgba.data_mut(0);
            if stride == row_bytes {
                dst[..raw_bytes.len()].copy_from_slice(raw_bytes);
            } else {
                for y in 0..sh as usize {
                    dst[y * stride..y * stride + row_bytes]
                        .copy_from_slice(&raw_bytes[y * row_bytes..(y + 1) * row_bytes]);
                }
            }
            let copy_rgba = t2.elapsed();

            // --- scaler.run ---
            let t3 = Instant::now();
            scaler.run(&rgba, &mut yuv).expect("scale");
            let scaler_run = t3.elapsed();

            // --- encoder.send_frame ---
            let t4 = Instant::now();
            yuv.set_pts(Some(f as i64));
            cc.send_frame(&yuv).expect("send");
            let send_frame = t4.elapsed();

            // --- drain packets ---
            let t5 = Instant::now();
            let mut pkt = ffmpeg::Packet::empty();
            while cc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(0);
                pkt.rescale_ts(ffmpeg::Rational(1, 30), stream_tb);
                pkt.write_interleaved(&mut octx).expect("write");
            }
            let drain = t5.elapsed();

            timings.push(FrameTimings {
                on_render, copy_rgba, scaler_run, send_frame, drain,
            });
        }

        // -------- encoder.flush --------
        let t = Instant::now();
        cc.send_eof().expect("eof");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(0);
            pkt.rescale_ts(ffmpeg::Rational(1, 30), stream_tb);
            pkt.write_interleaved(&mut octx).expect("flush");
        }
        octx.write_trailer().expect("trailer");
        phases.push(PhaseLog { label: "encoder.flush".into(), elapsed: t.elapsed(), children: vec![] });

        timings
    });

    let total = total_start.elapsed();

    // ---- Render the timeline ----
    println!();
    println!("=========================================================================");
    println!("hs-mvp instrumented run — phase timeline (sequential, no overlap):");
    println!("=========================================================================");
    let mut cumulative_ms = 0.0;
    for p in &phases {
        let now_ms = cumulative_ms;
        cumulative_ms += p.elapsed.as_secs_f64() * 1000.0;
        println!(
            "  [t={:>5.0} ms] {:<35}{:>10.2} ms",
            now_ms,
            p.label,
            p.elapsed.as_secs_f64() * 1000.0,
        );
    }
    let frame_loop_total: Duration = frame_timings
        .iter()
        .map(|f| f.on_render + f.copy_rgba + f.scaler_run + f.send_frame + f.drain)
        .sum();
    println!(
        "  [t={:>5.0} ms] {:<35}{:>10.2} ms   (300 frames, sum of phases)",
        cumulative_ms,
        "encoder.loop (300 frames)",
        frame_loop_total.as_secs_f64() * 1000.0,
    );
    println!();
    println!("  total wall                                     {:>10.2} ms ({:.1} fps e2e)",
             total.as_secs_f64() * 1000.0,
             300.0 / total.as_secs_f64());
    println!();

    // ---- Per-frame breakdown ----
    let on_render_ms: Vec<u128> = frame_timings.iter().map(|f| f.on_render.as_micros()).collect();
    let copy_ms: Vec<u128> = frame_timings.iter().map(|f| f.copy_rgba.as_micros()).collect();
    let scaler_ms: Vec<u128> = frame_timings.iter().map(|f| f.scaler_run.as_micros()).collect();
    let send_ms: Vec<u128> = frame_timings.iter().map(|f| f.send_frame.as_micros()).collect();
    let drain_ms: Vec<u128> = frame_timings.iter().map(|f| f.drain.as_micros()).collect();

    let sum_us = |v: &Vec<u128>| -> u128 { v.iter().sum() };
    let med_us = |v: &Vec<u128>| -> u128 { median(v.clone()) };

    println!("=========================================================================");
    println!("per-frame breakdown (median + total across 300 frames):");
    println!("=========================================================================");
    let total_per_frame_us = sum_us(&on_render_ms) + sum_us(&copy_ms) + sum_us(&scaler_ms)
        + sum_us(&send_ms) + sum_us(&drain_ms);
    let pct = |us: u128| -> f64 { 100.0 * us as f64 / total_per_frame_us as f64 };
    let line = |label: &str, us: &Vec<u128>| {
        println!(
            "  {:<32}  median {:>6.2} ms  total {:>7.1} ms  ({:>5.1}%)",
            label,
            med_us(us) as f64 / 1000.0,
            sum_us(us) as f64 / 1000.0,
            pct(sum_us(us)),
        );
    };
    line("scene.on_render (CPU compose)", &on_render_ms);
    line("copy_rgba_to_frame", &copy_ms);
    line("scaler.run (sws RGBA→YUV)", &scaler_ms);
    line("encoder.send_frame", &send_ms);
    line("drain packets", &drain_ms);
    println!(
        "  {:<32}  total {:>7.1} ms  ({:>5.1} ms/frame avg, ceiling {:.1} fps)",
        "─── sum",
        total_per_frame_us as f64 / 1000.0,
        total_per_frame_us as f64 / 1000.0 / 300.0,
        300.0 / (total_per_frame_us as f64 / 1_000_000.0),
    );

    // ---- JSON for diff ----
    let json = serde_json::json!({
        "total_wall_ms": total.as_millis(),
        "fps_e2e": 300.0 / total.as_secs_f64(),
        "phases": phases.iter().map(|p| serde_json::json!({
            "label": p.label,
            "elapsed_ms": p.elapsed.as_secs_f64() * 1000.0,
        })).collect::<Vec<_>>(),
        "per_frame_median_us": {
            "on_render": med_us(&on_render_ms),
            "copy_rgba": med_us(&copy_ms),
            "scaler_run": med_us(&scaler_ms),
            "send_frame": med_us(&send_ms),
            "drain": med_us(&drain_ms),
        },
        "per_frame_total_ms": {
            "on_render": sum_us(&on_render_ms) as f64 / 1000.0,
            "copy_rgba": sum_us(&copy_ms) as f64 / 1000.0,
            "scaler_run": sum_us(&scaler_ms) as f64 / 1000.0,
            "send_frame": sum_us(&send_ms) as f64 / 1000.0,
            "drain": sum_us(&drain_ms) as f64 / 1000.0,
        },
    });
    let json_path = "/tmp/hs-mvp-trace.json";
    std::fs::write(json_path, serde_json::to_string_pretty(&json).unwrap()).expect("write json");
    println!();
    println!("trace JSON: {json_path}");
}
