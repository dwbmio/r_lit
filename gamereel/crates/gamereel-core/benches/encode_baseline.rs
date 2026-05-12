//! M0 criterion bench: pure ffmpeg-next libx264 medium encoding throughput.
//!
//! Establishes a Rust-side baseline that mirrors gamereel-core's main loop:
//! synthetic RGBA buffer per frame → sws_scale → libx264 medium encode → mux.
//! This is the "x264 baseline" referenced by M1's 5x acceptance criterion.
//!
//! Methodology:
//!   - 720x1080 @ 30fps, 1 second per criterion sample (30 frames)
//!   - Solid-color animated RGBA source (cheap to generate, deterministic)
//!   - libx264 preset=medium, CRF 23
//!   - Encoder + scaler are reconstructed each iteration (matches current
//!     gamereel-core behavior — M1 will hoist them and re-bench)

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use std::path::PathBuf;

const W: u32 = 720;
const H: u32 = 1080;
const FPS: i32 = 30;
const FRAMES_PER_ITER: u32 = 30; // 1 second of video per criterion iter

fn ensure_ffmpeg() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        ffmpeg::init().expect("ffmpeg init");
    });
}

/// Encode `frames_per_iter` frames of a synthetic RGBA pattern with libx264
/// medium and write to a tempfile. Returns the bytes written so the bench
/// can verify non-trivial output.
fn encode_one_chunk(out_path: &PathBuf, frames_per_iter: u32) -> u64 {
    let mut octx = ffmpeg::format::output(out_path).expect("open output");
    let global_header = octx
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);

    let codec_x264 = codec::encoder::find_by_name("libx264").expect("libx264 missing");
    let mut ost = octx.add_stream(codec_x264).expect("add stream");

    let mut enc = codec::context::Context::new_with_codec(codec_x264)
        .encoder()
        .video()
        .expect("video encoder");
    enc.set_width(W);
    enc.set_height(H);
    enc.set_format(ffmpeg::format::Pixel::YUV420P);
    enc.set_frame_rate(Some((FPS, 1)));
    enc.set_time_base(ffmpeg::Rational(1, FPS));
    enc.set_bit_rate(6_000_000);
    if global_header {
        enc.set_flags(ffmpeg::codec::Flags::GLOBAL_HEADER);
    }
    let mut opts = ffmpeg::Dictionary::new();
    opts.set("preset", "medium");
    opts.set("crf", "23");
    let mut cc = enc.open_with(opts).expect("encoder open");

    ost.set_parameters(&cc);
    ost.set_time_base(ffmpeg::Rational(1, FPS));
    octx.write_header().expect("header");

    // Reusable scaler + frames matches the *current* (pre-M1) hot loop — but we
    // construct the scaler ONCE here even for the baseline so the bench measures
    // pure encoding cost, not allocator churn. The naive every-frame-rebuild path
    // is benchmarked separately in M1's `benches/m1_alloc_count.rs`.
    let mut scaler = ffmpeg::software::scaling::Context::get(
        ffmpeg::format::Pixel::RGBA, W, H,
        ffmpeg::format::Pixel::YUV420P, W, H,
        ffmpeg::software::scaling::Flags::BILINEAR,
    ).expect("scaler");
    let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, W, H);
    let mut yuv = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::YUV420P, W, H);

    for f in 0..frames_per_iter {
        // Cheap deterministic pattern: vertical gradient that scrolls per-frame.
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
        cc.send_frame(&yuv).expect("send frame");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.write_interleaved(&mut octx).expect("write pkt");
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

fn bench_x264_medium(c: &mut Criterion) {
    ensure_ffmpeg();
    let dir = tempfile::tempdir().expect("tmpdir");
    let mut idx: u64 = 0;

    let mut group = c.benchmark_group("encode_x264_medium_720x1080");
    group.throughput(Throughput::Elements(FRAMES_PER_ITER as u64));
    group.sample_size(10);
    group.bench_function("1s_30frames", |b| {
        b.iter(|| {
            idx += 1;
            let path = dir.path().join(format!("chunk_{idx}.mp4"));
            let bytes = encode_one_chunk(&path, FRAMES_PER_ITER);
            assert!(bytes > 1024, "encoded output suspiciously small: {bytes}");
            // Don't keep accumulating files on disk.
            let _ = std::fs::remove_file(&path);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_x264_medium);
criterion_main!(benches);
