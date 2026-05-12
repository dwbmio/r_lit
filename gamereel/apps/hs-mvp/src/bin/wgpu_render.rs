//! M4 end-to-end render: wgpu compositor + cudarc kernel + ffmpeg
//! CUDA hwframes + h264_nvenc.
//!
//! Mirrors `trace_cuda.rs` but replaces `Scene::on_render` (CPU
//! image_effect) with `compose_scene_frame` (wgpu). Outputs a phase
//! timeline + per-frame breakdown so we can compare against the
//! CPU-compose path apples-to-apples.

use ffmpeg_next as ffmpeg;
use ffmpeg::codec;
use ffmpeg_sys_next as ffsys;
use gamereel_compositor::{compose_scene_frame, upload_scene_textures, WgpuCompositor};
use gamereel_core::cuda_pipeline::CudaConverter;
use gamereel_core::ffmpeg_inc::hwctx::CudaHwContext;
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::stage::model::meta_scene::MetaSceneList;
use gamereel_core::{ffmpeg_inc as gci, RuntimeCtx};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "../nodes/mod.rs"]
mod nodes;
#[path = "../report/mod.rs"]
mod report;

struct PhaseLog { label: String, elapsed: Duration }

struct FrameTimings {
    wgpu_compose: Duration,
    cuda_convert: Duration,  // RGBA upload + kernel
    cuda_copy: Duration,
    cuda_sync: Duration,
    send_frame: Duration,
}
fn median<T: Ord + Copy>(mut v: Vec<T>) -> T { v.sort(); v[v.len()/2] }

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let project_root = env!("CARGO_MANIFEST_DIR");
    let out_mp4 = Path::new(project_root).join("tests/hs-proj/output_wgpu.mp4");

    let total_start = Instant::now();
    let mut phases: Vec<PhaseLog> = Vec::new();

    let t = Instant::now();
    gci::init_env().expect("ffmpeg init");
    phases.push(PhaseLog { label: "init.ffmpeg".into(), elapsed: t.elapsed() });

    // Load scene.
    let t = Instant::now();
    let bytes = std::fs::read(Path::new(project_root).join("tests/hs-proj/scene.meta")).expect("read");
    let scene_meta: MetaSceneList = serde_json::from_slice(&bytes).expect("parse");
    phases.push(PhaseLog { label: "scene.load".into(), elapsed: t.elapsed() });

    let t = Instant::now();
    let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
    rtx.set_source_path(PathBuf::from(project_root));
    let report = report::Report::new();
    report.gen_report_dynamic_images(&mut rtx);
    let mut mgr = StageMgr::new(scene_meta);
    report.gen_nodes(&mut rtx, &mut mgr.scenes_meta.meta_scene_list[0]);
    mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
    let scene = mgr.scenes.values_mut().next().expect("scene");
    scene.on_init(&rtx);
    phases.push(PhaseLog { label: "scene.preload+init".into(), elapsed: t.elapsed() });

    // Init wgpu compositor.
    let t = Instant::now();
    let mut compositor = WgpuCompositor::new(720, 1080).expect("WgpuCompositor::new");
    upload_scene_textures(&mut compositor, scene, &rtx).expect("upload");
    phases.push(PhaseLog { label: "wgpu.init+upload".into(), elapsed: t.elapsed() });

    // Init CUDA pipeline.
    let t = Instant::now();
    let mut converter = CudaConverter::new(720, 1080).expect("CudaConverter");
    let hwctx = CudaHwContext::new(720, 1080, 4).expect("CudaHwContext");
    phases.push(PhaseLog { label: "cuda.init".into(), elapsed: t.elapsed() });

    // Open ffmpeg encoder.
    let t = Instant::now();
    let mut octx = ffmpeg::format::output(&out_mp4).expect("output");
    let global_header = octx.format().flags().contains(ffmpeg::format::Flags::GLOBAL_HEADER);
    let codec_h264 = codec::encoder::find_by_name("h264_nvenc").expect("nvenc");
    let mut ost = octx.add_stream(codec_h264).expect("stream");
    let mut enc = codec::context::Context::new_with_codec(codec_h264).encoder().video().expect("video enc");
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
    phases.push(PhaseLog { label: "encoder.open".into(), elapsed: t.elapsed() });

    // ---- Render loop ----
    let mut timings: Vec<FrameTimings> = Vec::with_capacity(300);
    for f in 0..300u32 {
        let t1 = Instant::now();
        let rgba = compose_scene_frame(&mut compositor, scene, &mut rtx, f as f32 / 30.0)
            .expect("wgpu compose");
        let wgpu_compose = t1.elapsed();

        let t2 = Instant::now();
        converter.convert(&rgba).expect("convert");
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
                panic!("send_frame rc={send_rc}");
            }
            let mut fp = frame_ptr;
            ffsys::av_frame_free(&mut fp);
        }
        let send_frame = t5.elapsed();

        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.set_stream(0);
            pkt.rescale_ts(ffmpeg::Rational(1, 30), stream_tb);
            pkt.write_interleaved(&mut octx).expect("write");
        }

        timings.push(FrameTimings { wgpu_compose, cuda_convert, cuda_copy, cuda_sync, send_frame });
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

    let total = total_start.elapsed();

    // ---- Print phase timeline ----
    println!();
    println!("=========================================================================");
    println!("hs-mvp wgpu+CUDA pipeline:");
    println!("=========================================================================");
    let mut cum = 0.0;
    for p in &phases {
        let now = cum;
        cum += p.elapsed.as_secs_f64() * 1000.0;
        println!("  [t={:>5.0} ms] {:<35}{:>10.2} ms",
                 now, p.label, p.elapsed.as_secs_f64() * 1000.0);
    }
    let loop_total: Duration = timings.iter()
        .map(|f| f.wgpu_compose + f.cuda_convert + f.cuda_copy + f.cuda_sync + f.send_frame)
        .sum();
    println!("  [t={:>5.0} ms] {:<35}{:>10.2} ms (300 frames)",
             cum, "encoder.loop (wgpu+CUDA)", loop_total.as_secs_f64() * 1000.0);
    println!();
    println!("  total wall                                     {:>10.2} ms ({:.1} fps e2e)",
             total.as_secs_f64() * 1000.0, 300.0 / total.as_secs_f64());

    // ---- Per-frame breakdown ----
    let to_us = |d: Duration| d.as_micros();
    let wc: Vec<u128> = timings.iter().map(|f| to_us(f.wgpu_compose)).collect();
    let cc_us: Vec<u128> = timings.iter().map(|f| to_us(f.cuda_convert)).collect();
    let cp: Vec<u128> = timings.iter().map(|f| to_us(f.cuda_copy)).collect();
    let cs: Vec<u128> = timings.iter().map(|f| to_us(f.cuda_sync)).collect();
    let sf: Vec<u128> = timings.iter().map(|f| to_us(f.send_frame)).collect();
    let total_us: u128 = wc.iter().sum::<u128>() + cc_us.iter().sum::<u128>() + cp.iter().sum::<u128>() + cs.iter().sum::<u128>() + sf.iter().sum::<u128>();
    let pct = |us: u128| 100.0 * us as f64 / total_us as f64;
    let line = |label: &str, v: &Vec<u128>| {
        println!("  {:<32}  median {:>6.2} ms  total {:>7.1} ms  ({:>5.1}%)",
                 label, median(v.clone()) as f64 / 1000.0,
                 v.iter().sum::<u128>() as f64 / 1000.0, pct(v.iter().sum::<u128>()));
    };
    println!();
    println!("=========================================================================");
    println!("per-frame breakdown (M4 wgpu+CUDA, 300 frames):");
    println!("=========================================================================");
    line("wgpu.compose (GPU compose+RB)", &wc);
    line("cuda.convert (RGBA→NV12)",     &cc_us);
    line("cuda.copy_to_pool",            &cp);
    line("cuda.synchronize",             &cs);
    line("encoder.send_frame",           &sf);
    println!("  {:<32}  total {:>7.1} ms  ({:>5.1} ms/frame avg, ceiling {:.1} fps)",
             "─── sum", total_us as f64 / 1000.0,
             total_us as f64 / 1000.0 / 300.0,
             300.0 / (total_us as f64 / 1_000_000.0));

    // JSON for diff vs trace_cuda.json
    let json = serde_json::json!({
        "path": "M4 wgpu compose + CUDA hwframes + h264_nvenc",
        "total_wall_ms": total.as_millis(),
        "fps_e2e": 300.0 / total.as_secs_f64(),
        "per_frame_total_ms": {
            "wgpu_compose": wc.iter().sum::<u128>() as f64 / 1000.0,
            "cuda_convert": cc_us.iter().sum::<u128>() as f64 / 1000.0,
            "cuda_copy": cp.iter().sum::<u128>() as f64 / 1000.0,
            "cuda_sync": cs.iter().sum::<u128>() as f64 / 1000.0,
            "send_frame": sf.iter().sum::<u128>() as f64 / 1000.0,
        },
    });
    std::fs::write("/tmp/hs-mvp-trace-wgpu.json", serde_json::to_string_pretty(&json).unwrap()).expect("write");
    println!("\ntrace JSON: /tmp/hs-mvp-trace-wgpu.json");
}
