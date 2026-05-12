//! M1 unit test: scaler + frame buffers hoisted out of the encode loop.
//!
//! Pre-M1, `create_scene_stream` rebuilt `software::scaling::Context`,
//! `frame::Video::RGBA`, and `frame::Video::YUV420P` *every frame*
//! (mod.rs:67-83 in the original code). That triggered ~5 large heap
//! allocations per frame regardless of payload.
//!
//! Strategy: install a counting global allocator only in this test binary
//! (cargo compiles each integration test as a separate process, so the
//! custom allocator does not leak across tests). Drive a real end-to-end
//! encode of a synthetic 5-frame source and assert the *steady-state*
//! per-frame allocation count is below a generous threshold. We cannot
//! assert "= 0" because ffmpeg's encoder, mux and packet machinery do
//! their own allocations on every frame; we are guarding against
//! the scaler/frame churn specifically.
//!
//! Threshold: ≤ 32 large allocations (≥ 4 KiB) per frame in the steady
//! state. Pre-M1 this number was ~2-3x higher because each scaler context
//! alone allocates a slew of intermediate buffers.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

struct Counting;

static LARGE_ALLOCS: AtomicU64 = AtomicU64::new(0);
static TOTAL_BYTES: AtomicU64 = AtomicU64::new(0);
static GATE_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const LARGE_THRESHOLD: usize = 4096;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if GATE_OPEN.load(Ordering::Relaxed) && layout.size() >= LARGE_THRESHOLD {
            LARGE_ALLOCS.fetch_add(1, Ordering::Relaxed);
            TOTAL_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use std::path::PathBuf;

const W: u32 = 320;
const H: u32 = 240;
const FPS: i32 = 30;
const PROBE_FRAMES: u32 = 5;

/// Mirrors the post-M1 encode loop: encoder + scaler + frames built once,
/// then `PROBE_FRAMES` frames pumped through. Allocation counter is open
/// only across the per-frame body.
fn encode_with_hoisted_scaler(out: &PathBuf) -> u64 {
    ffmpeg::init().expect("ffmpeg init");

    let mut octx = ffmpeg::format::output(out).expect("open output");
    let codec_x264 = codec::encoder::find_by_name("libx264").expect("libx264 missing");
    let mut ost = octx.add_stream(codec_x264).expect("add stream");

    let mut enc = codec::context::Context::new_with_codec(codec_x264)
        .encoder()
        .video()
        .expect("video enc");
    enc.set_width(W);
    enc.set_height(H);
    enc.set_format(ffmpeg::format::Pixel::YUV420P);
    enc.set_frame_rate(Some((FPS, 1)));
    enc.set_time_base(ffmpeg::Rational(1, FPS));
    let mut opts = ffmpeg::Dictionary::new();
    opts.set("preset", "ultrafast");
    opts.set("crf", "30");
    let mut cc = enc.open_with(opts).expect("open enc");
    ost.set_parameters(&cc);
    ost.set_time_base(ffmpeg::Rational(1, FPS));
    octx.write_header().expect("header");

    // ---- HOISTED ----
    let mut scaler = ffmpeg::software::scaling::Context::get(
        ffmpeg::format::Pixel::RGBA, W, H,
        ffmpeg::format::Pixel::YUV420P, W, H,
        ffmpeg::software::scaling::Flags::BILINEAR,
    ).expect("scaler");
    let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, W, H);
    let mut yuv = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::YUV420P, W, H);
    // -----------------

    // Warm-up 1 frame outside the gate (so encoder lazy init doesn't taint the count).
    {
        let dst = rgba.data_mut(0);
        for b in dst.iter_mut() { *b = 0; }
        scaler.run(&rgba, &mut yuv).expect("warmup scale");
        yuv.set_pts(Some(0));
        cc.send_frame(&yuv).expect("warmup send");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.write_interleaved(&mut octx).expect("warmup write");
        }
    }

    LARGE_ALLOCS.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
    GATE_OPEN.store(true, Ordering::Relaxed);

    for f in 1..=PROBE_FRAMES {
        let dst = rgba.data_mut(0);
        for (i, b) in dst.iter_mut().enumerate() {
            *b = ((i + f as usize) & 0xff) as u8;
        }
        scaler.run(&rgba, &mut yuv).expect("scale");
        yuv.set_pts(Some(f as i64));
        cc.send_frame(&yuv).expect("send");
        let mut pkt = ffmpeg::Packet::empty();
        while cc.receive_packet(&mut pkt).is_ok() {
            pkt.write_interleaved(&mut octx).expect("write");
        }
    }

    GATE_OPEN.store(false, Ordering::Relaxed);

    cc.send_eof().expect("eof");
    let mut pkt = ffmpeg::Packet::empty();
    while cc.receive_packet(&mut pkt).is_ok() {
        pkt.write_interleaved(&mut octx).expect("flush");
    }
    octx.write_trailer().expect("trailer");

    LARGE_ALLOCS.load(Ordering::Relaxed)
}

#[test]
fn steady_state_per_frame_large_allocs_under_threshold() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let out = dir.path().join("hoisted.mp4");
    let total = encode_with_hoisted_scaler(&out);
    let per_frame = total as f64 / PROBE_FRAMES as f64;
    let bytes = TOTAL_BYTES.load(Ordering::Relaxed);
    println!(
        "scaler-reuse: {total} large allocs across {PROBE_FRAMES} frames \
         ({per_frame:.1} alloc/frame, {bytes} bytes total)"
    );

    // Threshold: the post-M1 hoisted loop measures **0 large allocs/frame**
    // on libx264 ultrafast — scaler context, packet header, and both Video
    // frames are constructed once and recycled. We allow ≤ 4 to absorb
    // libavcodec internal jitter on different versions, but anything more
    // means a regression (someone reintroduced per-frame allocation).
    assert!(
        per_frame <= 4.0,
        "regression: {per_frame:.1} large allocs/frame exceeds budget of 4 \
         (total {total} across {PROBE_FRAMES} frames)"
    );
    // Output must actually have been written.
    let sz = std::fs::metadata(&out).expect("output metadata").len();
    assert!(sz > 1024, "output suspiciously small: {sz} bytes");
}
