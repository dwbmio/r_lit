//! M1 criterion bench: encoder-only throughput on the post-M1 hot loop.
//!
//! Mirrors `encode_baseline.rs` but lets the user select which encoder is
//! exercised — so we can see the standalone speed of NVENC vs libx264 on
//! the same RGBA→YUV pipeline. Composition cost (CPU `image_effect` blends)
//! is *not* in scope here; that's M4. This bench only proves that the
//! encoder + scaler subsystem hits the M1 5x acceptance bar.
//!
//! Acceptance from the roadmap: ≥ 760 fps for 720x1080 (5x M0 baseline of 152).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use gamereel_core::ffmpeg_inc::encoder_pick::{opts_for, EncoderChoice};
use std::path::PathBuf;

const W: u32 = 720;
const H: u32 = 1080;
const FPS: i32 = 30;
// 10 s per iteration. NVENC session cold-start is ~300–500 ms; shorter clips
// would have startup time dominate the measurement, undercounting steady-state
// throughput by 5–10x.
const FRAMES_PER_ITER: u32 = 300;

fn ensure_ffmpeg() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        ffmpeg::init().expect("ffmpeg init");
    });
}

fn encode_chunk(out_path: &PathBuf, choice: &EncoderChoice, frames: u32) -> u64 {
    let mut octx = ffmpeg::format::output(out_path).expect("open output");
    let global_header = octx
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);

    let codec_h264 = codec::encoder::find_by_name(choice.codec_name)
        .unwrap_or_else(|| panic!("encoder '{}' not present in linked ffmpeg", choice.codec_name));
    let mut ost = octx.add_stream(codec_h264).expect("add stream");

    let mut enc = codec::context::Context::new_with_codec(codec_h264)
        .encoder()
        .video()
        .expect("video encoder");
    enc.set_width(W);
    enc.set_height(H);
    enc.set_format(choice.pixel_format);
    enc.set_frame_rate(Some((FPS, 1)));
    enc.set_time_base(ffmpeg::Rational(1, FPS));
    if global_header {
        enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
    }
    let mut opts = ffmpeg::Dictionary::new();
    for (k, v) in &choice.opts {
        opts.set(k, v);
    }
    let mut cc = enc.open_with(opts).expect("open enc");
    ost.set_parameters(&cc);
    ost.set_time_base(ffmpeg::Rational(1, FPS));
    octx.write_header().expect("header");

    // Hoisted scaler + frames (post-M1 hot-loop layout).
    let mut scaler = ffmpeg::software::scaling::Context::get(
        ffmpeg::format::Pixel::RGBA, W, H,
        choice.pixel_format, W, H,
        ffmpeg::software::scaling::Flags::BILINEAR,
    ).expect("scaler");
    let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, W, H);
    let mut yuv = ffmpeg::frame::Video::new(choice.pixel_format, W, H);

    for f in 0..frames {
        let buf = rgba.data_mut(0);
        for y in 0..H as usize {
            let row = (f as usize + y) as u8;
            let base = y * (W * 4) as usize;
            for x in 0..W as usize {
                let i = base + x * 4;
                buf[i]     = row;
                buf[i + 1] = (x as u8).wrapping_add(row);
                buf[i + 2] = ((x ^ y) as u8).wrapping_add(row);
                buf[i + 3] = 255;
            }
        }
        scaler.run(&rgba, &mut yuv).expect("scale");
        yuv.set_pts(Some(f as i64));
        cc.send_frame(&yuv).expect("send");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.write_interleaved(&mut octx).expect("write");
        }
    }
    cc.send_eof().expect("eof");
    let mut pkt = ffmpeg::Packet::empty();
    while cc.receive_packet(&mut pkt).is_ok() {
        pkt.write_interleaved(&mut octx).expect("flush");
    }
    octx.write_trailer().expect("trailer");
    std::fs::metadata(out_path).map(|m| m.len()).unwrap_or(0)
}

fn bench(c: &mut Criterion) {
    ensure_ffmpeg();
    let dir = tempfile::tempdir().expect("tmpdir");
    let mut idx: u64 = 0;

    let mut group = c.benchmark_group("encode_720x1080_10s");
    group.throughput(Throughput::Elements(FRAMES_PER_ITER as u64));
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(30));

    // Order: HW first (NVENC), then x264 reference. Skip silently if the
    // encoder is not linked into ffmpeg — keeps the bench portable.
    let candidates: &[&str] = &["h264_nvenc", "libx264"];
    for name in candidates {
        if ffmpeg::codec::encoder::find_by_name(name).is_none() {
            eprintln!("encoder '{name}' not available, skipping");
            continue;
        }
        let choice = opts_for(name);
        group.bench_with_input(BenchmarkId::from_parameter(name), &choice, |b, c| {
            b.iter(|| {
                idx += 1;
                let path = dir.path().join(format!("{name}_{idx}.mp4"));
                let bytes = encode_chunk(&path, c, FRAMES_PER_ITER);
                assert!(bytes > 1024, "{name} produced suspiciously small file: {bytes}");
                let _ = std::fs::remove_file(&path);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
