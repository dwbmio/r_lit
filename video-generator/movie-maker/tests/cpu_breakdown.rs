//! Diagnostic: where does the CPU time go in a movie-maker render?
//!
//! Not a regression test — this prints a phase-by-phase timing
//! breakdown. Run with `cargo test --release -p movie-maker --test
//! cpu_breakdown -- --nocapture --ignored cpu_breakdown_perf_main`.
//!
//! The `#[ignore]` keeps it out of the default test sweep; this test
//! exists to answer the recurring "why is CPU still hot when I'm using
//! NVENC?" question with hard numbers.

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use movie_maker::encoder_profile::EncoderProfile;
use movie_maker::ffmpeg_inc::stage_mgr::StageMgr;
use movie_maker::stage;
use movie_maker::RuntimeCtx;
use std::path::PathBuf;
use std::time::Instant;

#[test]
#[ignore]
fn cpu_breakdown_perf_main() {
    let project_root = env!("CARGO_MANIFEST_DIR");
    let scene_meta_path = PathBuf::from(project_root).join("tests/perf_main/scene.meta");

    ffmpeg::init().expect("ffmpeg init");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");

    rt.block_on(async {
        let scene_list = stage::import_scene(scene_meta_path).await.expect("import");

        let mut rtx = RuntimeCtx::new(720, 1080, 10, 30);
        rtx.init(Some(PathBuf::from(project_root))).expect("init");

        let mut mgr = StageMgr::new(scene_list);
        mgr.meta_scene_preload(&mut rtx, 0).expect("preload");
        let scene = mgr
            .scenes
            .values_mut()
            .next()
            .expect("at least one scene");
        scene.on_init(&rtx);

        // Phase 1: pure composition (Scene::on_render in a loop, no encode)
        let mut total_compose = std::time::Duration::ZERO;
        let mut sample_image: Option<image::DynamicImage> = None;
        for f in 0..300u32 {
            let t = f as f32 / 30.0;
            let start = Instant::now();
            let img = scene.on_render(&mut rtx, t).expect("render");
            total_compose += start.elapsed();
            if f == 0 {
                sample_image = Some(img.clone());
            }
        }
        let compose_ms = total_compose.as_secs_f64() * 1000.0;

        // Phase 2: pure RGBA→YUV (sws_scale) on the same composed frames
        let img = sample_image.expect("got a sample frame");
        let (w, h) = (img.width(), img.height());
        let mut scaler = ffmpeg::software::scaling::Context::get(
            ffmpeg::format::Pixel::RGBA, w, h,
            ffmpeg::format::Pixel::YUV420P, 720, 1080,
            ffmpeg::software::scaling::Flags::BILINEAR,
        ).expect("scaler");
        let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, w, h);
        let mut yuv = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::YUV420P, 720, 1080);
        let raw = img.to_rgba8();
        let raw_bytes = raw.as_raw();
        let row_bytes = (w as usize) * 4;
        let linesize = rgba.stride(0);
        let dst = rgba.data_mut(0);
        if linesize == row_bytes {
            dst[..raw_bytes.len()].copy_from_slice(raw_bytes);
        } else {
            for y in 0..h as usize {
                dst[y * linesize..y * linesize + row_bytes]
                    .copy_from_slice(&raw_bytes[y * row_bytes..(y + 1) * row_bytes]);
            }
        }

        let start = Instant::now();
        for _ in 0..300 {
            scaler.run(&rgba, &mut yuv).expect("scale");
        }
        let scale_ms = start.elapsed().as_secs_f64() * 1000.0;

        // Phase 3: pure NVENC encoding on the YUV frame
        let choice = EncoderProfile::Balanced.to_encoder_choice().expect("choice");
        let codec = codec::encoder::find_by_name(choice.codec_name).expect("codec");
        let dir = tempfile::tempdir().expect("tmp");
        let out_path = dir.path().join("encode_only.mp4");

        let mut octx = ffmpeg::format::output(&out_path).expect("octx");
        let global_header = octx
            .format()
            .flags()
            .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
        let mut ost = octx.add_stream(codec).expect("stream");
        let mut enc = codec::context::Context::new_with_codec(codec)
            .encoder()
            .video()
            .expect("enc");
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

        // Warm up NVENC: send 30 frames, discard timing
        for f in 0..30 {
            yuv.set_pts(Some(f as i64));
            cc.send_frame(&yuv).expect("send");
            let mut pkt = ffmpeg::Packet::empty();
            while cc.receive_packet(&mut pkt).is_ok() {
                pkt.write_interleaved(&mut octx).expect("write");
            }
        }

        let start = Instant::now();
        for f in 30..330 {
            yuv.set_pts(Some(f as i64));
            cc.send_frame(&yuv).expect("send");
            let mut pkt = ffmpeg::Packet::empty();
            while cc.receive_packet(&mut pkt).is_ok() {
                pkt.write_interleaved(&mut octx).expect("write");
            }
        }
        let encode_ms = start.elapsed().as_secs_f64() * 1000.0;
        cc.send_eof().expect("eof");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.write_interleaved(&mut octx).expect("flush");
        }
        octx.write_trailer().expect("trailer");

        let total = compose_ms + scale_ms + encode_ms;
        println!();
        println!("CPU breakdown for 300-frame 720x1080 movie-maker render:");
        println!("  Compose (Scene::on_render, CPU pixel blends): {compose_ms:>7.1} ms  ({:>5.1}%)", 100.0 * compose_ms / total);
        println!("  RGBA→YUV (sws_scale on CPU):                  {scale_ms:>7.1} ms  ({:>5.1}%)", 100.0 * scale_ms / total);
        println!("  NVENC encode (GPU, CPU only does send/recv):  {encode_ms:>7.1} ms  ({:>5.1}%)", 100.0 * encode_ms / total);
        println!("  Total isolated:                               {total:>7.1} ms");
        println!();
        println!("Note: phases sum to *more* than e2e wall time because the real");
        println!("hot loop overlaps composition for frame N+1 with NVENC submission");
        println!("for frame N. The breakdown shows where each phase's *cost* lives,");
        println!("not its critical-path time.");
    });
}
