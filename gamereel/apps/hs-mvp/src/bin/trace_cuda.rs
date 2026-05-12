//! Same workload as `trace.rs` but routes through the M3 CUDA hwframes
//! pipeline (`create_scene_stream_cuda`) instead of the default M2
//! sws_scale path. Output is comparable line-for-line so we can quantify
//! how much M3 actually buys on the real demo.
//!
//! Per-frame instrumentation here covers:
//!   * scene.on_render        (CPU compose, identical to trace.rs)
//!   * cuda_convert.upload+kernel  (RGBA host→GPU + RGBA→NV12 kernel)
//!   * cuda_convert.copy_to_pool   (cuMemcpy2DAsync_v2 to ffmpeg pool)
//!   * cuda_convert.synchronize    (the M3 per-frame stall)
//!   * encoder.send_frame
//!   * drain packets

use ffmpeg_next as ffmpeg;
use ffmpeg::codec;
use ffmpeg_sys_next as ffsys;
use gamereel_core::cuda_pipeline::CudaConverter;
use gamereel_core::ffmpeg_inc::hwctx::CudaHwContext;
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::stage;
use gamereel_core::{ffmpeg_inc as gci, RuntimeCtx};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "../nodes/mod.rs"]
mod nodes;
#[path = "../report/mod.rs"]
mod report;

#[derive(Default)]
struct PhaseLog {
    label: String,
    elapsed: Duration,
}

struct FrameTimings {
    on_render: Duration,
    cuda_convert: Duration,
    cuda_copy: Duration,
    cuda_sync: Duration,
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
        .join("tests/hs-proj/output_trace_cuda.mp4");

    let total_start = Instant::now();
    let mut phases: Vec<PhaseLog> = Vec::new();

    let t = Instant::now();
    gci::init_env().expect("ffmpeg init");
    phases.push(PhaseLog { label: "init.ffmpeg".into(), elapsed: t.elapsed() });

    let t = Instant::now();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio");
    phases.push(PhaseLog { label: "init.tokio".into(), elapsed: t.elapsed() });

    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    rtx.set_source_path(Path::new(project_root).to_path_buf());

    let frame_timings: Vec<FrameTimings> = rt.block_on(async {
        let t = Instant::now();
        let scene_meta = stage::import_scene(
            Path::new(project_root).join("tests/hs-proj/scene.meta"),
        )
        .await
        .expect("import");
        phases.push(PhaseLog { label: "scene.import_meta".into(), elapsed: t.elapsed() });

        let t = Instant::now();
        let report = report::Report::new();
        report.gen_report_dynamic_images(&mut rtx);
        phases.push(PhaseLog { label: "report.gen_dynamic_images".into(), elapsed: t.elapsed() });

        let t = Instant::now();
        let mut stage_mgr = StageMgr::new(scene_meta);
        report.gen_nodes(&mut rtx, &mut stage_mgr.scenes_meta.meta_scene_list[0]);
        phases.push(PhaseLog { label: "report.gen_nodes".into(), elapsed: t.elapsed() });

        let t = Instant::now();
        stage_mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
        phases.push(PhaseLog { label: "scene.preload".into(), elapsed: t.elapsed() });

        let scene = stage_mgr.scenes.values_mut().next().expect("scene");

        let t = Instant::now();
        scene.on_init(&rtx);
        phases.push(PhaseLog { label: "scene.on_init".into(), elapsed: t.elapsed() });

        // ---- CUDA pipeline init (M3 specific) ----
        let t = Instant::now();
        let mut converter = CudaConverter::new(720, 1080).expect("CudaConverter");
        let hwctx = CudaHwContext::new(720, 1080, 4).expect("CudaHwContext");
        phases.push(PhaseLog { label: "cuda.init (NVRTC + hwframes pool)".into(), elapsed: t.elapsed() });

        // ---- encoder open with hw_frames_ctx ----
        let t = Instant::now();
        let mut octx = ffmpeg::format::output(&out_mp4).expect("output");
        let global_header = octx.format().flags()
            .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let codec_h264 = codec::encoder::find_by_name("h264_nvenc").expect("nvenc");
        let mut ost = octx.add_stream(codec_h264).expect("stream");
        let mut enc = codec::context::Context::new_with_codec(codec_h264)
            .encoder().video().expect("video enc");
        enc.set_width(720); enc.set_height(1080);
        enc.set_format(ffmpeg::format::Pixel::CUDA);
        enc.set_frame_rate(Some((30, 1)));
        enc.set_time_base(ffmpeg::Rational(1, 30));
        if global_header { enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER); }
        unsafe {
            let cc_ptr = enc.as_mut_ptr() as *mut ffsys::AVCodecContext;
            (*cc_ptr).hw_frames_ctx = hwctx.frames_ref();
        }
        let mut opts = ffmpeg::Dictionary::new();
        for (k, v) in &[("preset","p4"), ("tune","hq"), ("rc","vbr"), ("cq","23"),
                        ("b:v","8M"), ("maxrate","12M"), ("bufsize","16M"),
                        ("profile","high"), ("bf","3")] {
            opts.set(k, v);
        }
        let mut cc = enc.open_with(opts).expect("open enc");
        ost.set_parameters(&cc);
        ost.set_time_base(ffmpeg::Rational(1, 30));
        octx.write_header().expect("header");
        let stream_tb = octx.stream(0).expect("stream").time_base();
        phases.push(PhaseLog { label: "encoder.open (CUDA path)".into(), elapsed: t.elapsed() });

        let mut timings: Vec<FrameTimings> = Vec::with_capacity(300);

        for f in 0..300u32 {
            let t1 = Instant::now();
            let img = scene.on_render(&mut rtx, f as f32 / 30.0).expect("render");
            let on_render = t1.elapsed();

            let t2 = Instant::now();
            let rgba = img.to_rgba8();
            converter.convert(rgba.as_raw()).expect("convert");
            let cuda_convert = t2.elapsed();

            let t3 = Instant::now();
            let frame_ptr = unsafe { hwctx.allocate_frame().expect("alloc") };
            unsafe {
                let dy = (*frame_ptr).data[0] as u64;
                let duv = (*frame_ptr).data[1] as u64;
                let dyp = (*frame_ptr).linesize[0] as usize;
                let dup = (*frame_ptr).linesize[1] as usize;
                converter.copy_to_device_2d(dy, dyp, duv, dup).expect("copy 2d");
            }
            let cuda_copy = t3.elapsed();

            let t4 = Instant::now();
            converter.synchronize().expect("sync");
            let cuda_sync = t4.elapsed();

            let t5 = Instant::now();
            unsafe {
                (*frame_ptr).pts = f as i64;
                let cc_ptr = cc.as_mut_ptr() as *mut ffsys::AVCodecContext;
                let send_rc = ffsys::avcodec_send_frame(cc_ptr, frame_ptr);
                if send_rc < 0 {
                    let mut fp = frame_ptr;
                    ffsys::av_frame_free(&mut fp);
                    panic!("avcodec_send_frame rc={send_rc}");
                }
                let mut fp = frame_ptr;
                ffsys::av_frame_free(&mut fp);
            }
            let send_frame = t5.elapsed();

            let t6 = Instant::now();
            let mut pkt = ffmpeg::Packet::empty();
            while cc.receive_packet(&mut pkt).is_ok() {
                pkt.set_stream(0);
                pkt.rescale_ts(ffmpeg::Rational(1, 30), stream_tb);
                pkt.write_interleaved(&mut octx).expect("write");
            }
            let drain = t6.elapsed();

            timings.push(FrameTimings { on_render, cuda_convert, cuda_copy, cuda_sync, send_frame, drain });
        }

        let t = Instant::now();
        cc.send_eof().expect("eof");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(0);
            pkt.rescale_ts(ffmpeg::Rational(1, 30), stream_tb);
            pkt.write_interleaved(&mut octx).expect("flush");
        }
        octx.write_trailer().expect("trailer");
        phases.push(PhaseLog { label: "encoder.flush".into(), elapsed: t.elapsed() });

        timings
    });

    let total = total_start.elapsed();

    println!();
    println!("=========================================================================");
    println!("hs-mvp instrumented run (M3 CUDA path) — phase timeline:");
    println!("=========================================================================");
    let mut cumulative_ms = 0.0;
    for p in &phases {
        let now_ms = cumulative_ms;
        cumulative_ms += p.elapsed.as_secs_f64() * 1000.0;
        println!("  [t={:>5.0} ms] {:<35}{:>10.2} ms",
            now_ms, p.label, p.elapsed.as_secs_f64() * 1000.0);
    }
    let frame_loop_total: Duration = frame_timings.iter()
        .map(|f| f.on_render + f.cuda_convert + f.cuda_copy + f.cuda_sync + f.send_frame + f.drain)
        .sum();
    println!("  [t={:>5.0} ms] {:<35}{:>10.2} ms   (300 frames)",
        cumulative_ms, "encoder.loop (CUDA)", frame_loop_total.as_secs_f64() * 1000.0);
    println!();
    println!("  total wall                                     {:>10.2} ms ({:.1} fps e2e)",
        total.as_secs_f64() * 1000.0, 300.0 / total.as_secs_f64());
    println!();

    let on_render: Vec<u128> = frame_timings.iter().map(|f| f.on_render.as_micros()).collect();
    let cuda_conv: Vec<u128> = frame_timings.iter().map(|f| f.cuda_convert.as_micros()).collect();
    let cuda_cp:   Vec<u128> = frame_timings.iter().map(|f| f.cuda_copy.as_micros()).collect();
    let cuda_sy:   Vec<u128> = frame_timings.iter().map(|f| f.cuda_sync.as_micros()).collect();
    let send:      Vec<u128> = frame_timings.iter().map(|f| f.send_frame.as_micros()).collect();
    let drain:     Vec<u128> = frame_timings.iter().map(|f| f.drain.as_micros()).collect();

    let sum = |v: &Vec<u128>| -> u128 { v.iter().sum() };
    let med = |v: &Vec<u128>| -> u128 { median(v.clone()) };
    let total_us: u128 = sum(&on_render) + sum(&cuda_conv) + sum(&cuda_cp) + sum(&cuda_sy) + sum(&send) + sum(&drain);
    let pct = |us: u128| -> f64 { 100.0 * us as f64 / total_us as f64 };
    let line = |label: &str, v: &Vec<u128>| {
        println!("  {:<32}  median {:>6.2} ms  total {:>7.1} ms  ({:>5.1}%)",
            label, med(v) as f64 / 1000.0, sum(v) as f64 / 1000.0, pct(sum(v)));
    };

    println!("=========================================================================");
    println!("per-frame breakdown (M3 CUDA path, 300 frames):");
    println!("=========================================================================");
    line("scene.on_render (CPU compose)", &on_render);
    line("cuda.convert (upload+kernel)",  &cuda_conv);
    line("cuda.copy_to_pool (cuMemcpy2D)",&cuda_cp);
    line("cuda.synchronize (per-frame)",  &cuda_sy);
    line("encoder.send_frame",            &send);
    line("drain packets",                 &drain);
    println!("  {:<32}  total {:>7.1} ms  ({:>5.1} ms/frame avg, ceiling {:.1} fps)",
        "─── sum", total_us as f64 / 1000.0, total_us as f64 / 1000.0 / 300.0,
        300.0 / (total_us as f64 / 1_000_000.0));

    let json = serde_json::json!({
        "path": "M3 CUDA hwframes",
        "total_wall_ms": total.as_millis(),
        "fps_e2e": 300.0 / total.as_secs_f64(),
        "per_frame_total_ms": {
            "on_render": sum(&on_render) as f64 / 1000.0,
            "cuda_convert": sum(&cuda_conv) as f64 / 1000.0,
            "cuda_copy": sum(&cuda_cp) as f64 / 1000.0,
            "cuda_sync": sum(&cuda_sy) as f64 / 1000.0,
            "send_frame": sum(&send) as f64 / 1000.0,
            "drain": sum(&drain) as f64 / 1000.0,
        },
    });
    std::fs::write("/tmp/hs-mvp-trace-cuda.json", serde_json::to_string_pretty(&json).unwrap()).expect("write json");
    println!();
    println!("trace JSON: /tmp/hs-mvp-trace-cuda.json");
}
