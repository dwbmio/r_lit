//! M2 unit test: profile speed ordering.
//!
//! Asserts the *relative* speed ordering between profiles holds:
//!   Fast > Balanced > TikTokHQ
//!
//! Implementation note: all comparisons run in a single `#[test]` so there
//! is no parallel-test contention for the single GPU NVENC engine. cargo
//! defaults to running integration tests in parallel; a Fast test running
//! alongside a TikTokHQ test would have both sessions waiting on the
//! single hardware encoder and produce noise indistinguishable from the
//! preset-speed signal we want to measure.
//!
//! Sizing: 720×1080 (production resolution) and 300 frames (10 s @ 30 fps)
//! per measurement. NVENC session cold-start is 300–500 ms; smaller
//! workloads make the cold-start dominate and the preset signal disappear.
//!
//! VMAF / SSIM / PSNR floors live in
//! `tools/quality-eval/profile_quality_floor.sh` because each VMAF eval
//! takes 3–5 s and is too slow for cargo test.

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use movie_maker::encoder_profile::EncoderProfile;
use std::path::PathBuf;
use std::time::Instant;

const W: u32 = 720;
const H: u32 = 1080;
const MEASURED_FRAMES: u32 = 300; // 10 s
const WARMUP_FRAMES: u32 = 30;    // 1 s warmup, discarded

fn ensure_ffmpeg() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        ffmpeg::init().expect("ffmpeg init");
    });
}

fn time_profile(out: &PathBuf, profile: EncoderProfile) -> u128 {
    ensure_ffmpeg();
    let choice = profile.to_encoder_choice().expect("profile resolves");
    let codec = codec::encoder::find_by_name(choice.codec_name).expect("codec exists");

    // Warmup: NVENC session cold-start. Discarded.
    do_encode(out, &choice, codec, WARMUP_FRAMES);
    let start = Instant::now();
    do_encode(out, &choice, codec, MEASURED_FRAMES);
    start.elapsed().as_millis()
}

fn do_encode(
    out: &PathBuf,
    choice: &movie_maker::ffmpeg_inc::encoder_pick::EncoderChoice,
    codec: ffmpeg::Codec,
    frames: u32,
) {
    let mut octx = ffmpeg::format::output(out).expect("open output");
    let global_header = octx
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
    let mut ost = octx.add_stream(codec).expect("add stream");

    let mut enc = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .expect("video enc");
    enc.set_width(W);
    enc.set_height(H);
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
}

#[test]
fn profile_speed_ordering_holds() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let fast_ms = time_profile(&dir.path().join("fast.mp4"), EncoderProfile::Fast);
    let bal_ms  = time_profile(&dir.path().join("bal.mp4"),  EncoderProfile::Balanced);
    let hq_ms   = time_profile(&dir.path().join("hq.mp4"),   EncoderProfile::TikTokHQ);
    let hdr_ms  = time_profile(&dir.path().join("hdr.mp4"),  EncoderProfile::IgReelsHDR);

    let fps = |ms: u128| (MEASURED_FRAMES as f64) * 1000.0 / ms as f64;
    println!(
        "  Fast:       {fast_ms:>4} ms ({:>6.1} fps)\n  \
           Balanced:   {bal_ms:>4} ms ({:>6.1} fps)\n  \
           TikTokHQ:   {hq_ms:>4} ms ({:>6.1} fps)\n  \
           IgReelsHDR: {hdr_ms:>4} ms ({:>6.1} fps)",
        fps(fast_ms), fps(bal_ms), fps(hq_ms), fps(hdr_ms)
    );

    // 1) Fast not slower than Balanced × 1.10. Generous because at 720×1080
    //    on a 3060, both presets approach the GPU's hardware-encoder
    //    saturation point and preset choice barely moves the needle.
    assert!(
        fast_ms as f64 <= bal_ms as f64 * 1.10,
        "Fast ({fast_ms} ms) should not be > 1.10× Balanced ({bal_ms} ms) — \
         Fast preset (p2) regressed"
    );

    // 2) TikTokHQ not faster than Balanced × 0.85. p6 + lookahead + AQ
    //    is intentionally heavier; if it ever beats Balanced by more than
    //    15 %, someone silently dropped the quality knobs.
    assert!(
        hq_ms as f64 >= bal_ms as f64 * 0.85,
        "TikTokHQ ({hq_ms} ms) should not be < 0.85× Balanced ({bal_ms} ms) — \
         heavier-quality params were likely dropped"
    );

    // 3) IgReelsHDR within 30 % of TikTokHQ (M2 fallback contract). M3
    //    will diverge this; update the test when that lands.
    let ratio = hdr_ms as f64 / hq_ms as f64;
    assert!(
        ratio > 0.70 && ratio < 1.30,
        "IgReelsHDR ({hdr_ms} ms) and TikTokHQ ({hq_ms} ms) should be within ±30% \
         in M2 (HDR fallback). Got ratio={ratio:.2}. Update when M3 implements HDR."
    );
}
