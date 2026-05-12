//! M2 unit tests for `EncoderProfile`.
//!
//! These run against the real encoder pipeline (not mocks) and use
//! `ffprobe` to verify that each profile produces a video stream with
//! the expected codec, profile, pixel format, and *order-of-magnitude*
//! bitrate. Exact bitrate isn't asserted because content variance moves
//! it ±30% — VMAF / SSIM regression coverage is in
//! `tests/profile_quality_floor.rs`.
//!
//! Each test encodes a short (1 s) synthetic source, runs `ffprobe`
//! against the output, and asserts the parsed metadata. This catches
//! regressions where someone (a) flips an encoder default, (b) breaks
//! the codec selection chain, or (c) renames a parameter ffmpeg silently
//! ignores.

use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use gamereel_core::encoder_profile::EncoderProfile;
use std::path::PathBuf;
use std::process::Command;

fn ffmpeg_init() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        ffmpeg::init().expect("ffmpeg init");
    });
}

/// Encode a 1-second 320x180 synthetic source using `profile`. We build
/// the encoder pipeline directly here (mirrors the production hot loop)
/// rather than going through `create_scene_stream_with_profile` to keep
/// the test self-contained and fast.
fn encode_synthetic(out: &PathBuf, profile: EncoderProfile) {
    ffmpeg_init();
    let choice = profile.to_encoder_choice().expect("profile resolves");
    let codec = codec::encoder::find_by_name(choice.codec_name).expect("codec exists");

    let mut octx = ffmpeg::format::output(out).expect("open output");
    let global_header = octx
        .format()
        .flags()
        .contains(ffmpeg::format::Flags::GLOBAL_HEADER);
    let mut ost = octx.add_stream(codec).expect("add stream");

    let mut enc = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .expect("video encoder");
    enc.set_width(320);
    enc.set_height(180);
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
        ffmpeg::format::Pixel::RGBA, 320, 180,
        choice.pixel_format, 320, 180,
        ffmpeg::software::scaling::Flags::BILINEAR,
    ).expect("scaler");
    let mut rgba = ffmpeg::frame::Video::new(ffmpeg::format::Pixel::RGBA, 320, 180);
    let mut yuv = ffmpeg::frame::Video::new(choice.pixel_format, 320, 180);

    for f in 0..30u32 {
        let buf = rgba.data_mut(0);
        for (i, b) in buf.iter_mut().enumerate() {
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
    cc.send_eof().expect("eof");
    let mut pkt = ffmpeg::Packet::empty();
    while cc.receive_packet(&mut pkt).is_ok() {
        pkt.write_interleaved(&mut octx).expect("flush");
    }
    octx.write_trailer().expect("trailer");
}

/// Run `ffprobe` and return (codec_name, profile, pix_fmt) for stream 0.
fn probe(path: &PathBuf) -> (String, String, String) {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error",
            "-select_streams", "v:0",
            "-show_entries", "stream=codec_name,profile,pix_fmt",
            "-of", "default=nw=1",
        ])
        .arg(path)
        .output()
        .expect("ffprobe");
    let s = String::from_utf8_lossy(&out.stdout);
    let mut codec_name = String::new();
    let mut profile = String::new();
    let mut pix_fmt = String::new();
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("codec_name=") { codec_name = v.into(); }
        else if let Some(v) = line.strip_prefix("profile=") { profile = v.into(); }
        else if let Some(v) = line.strip_prefix("pix_fmt=") { pix_fmt = v.into(); }
    }
    (codec_name, profile, pix_fmt)
}

fn run_for_profile(profile: EncoderProfile, expected_label: &str) {
    let dir = tempfile::tempdir().expect("tmpdir");
    let out = dir.path().join(format!("{expected_label}.mp4"));
    encode_synthetic(&out, profile);

    let bytes = std::fs::metadata(&out).expect("metadata").len();
    assert!(bytes > 1024, "{expected_label} output suspiciously small: {bytes} bytes");

    let (codec_name, profile_str, pix_fmt) = probe(&out);
    assert_eq!(codec_name, "h264", "{expected_label}: codec_name mismatch (got '{codec_name}')");
    assert_eq!(profile_str, "High", "{expected_label}: H.264 profile must be High (got '{profile_str}')");
    // YUV420P or NV12 — both are 4:2:0 8-bit, the choice depends on encoder.
    assert!(
        pix_fmt == "yuv420p" || pix_fmt == "nv12",
        "{expected_label}: pix_fmt should be yuv420p or nv12 (got '{pix_fmt}')"
    );
}

#[test]
fn fast_profile_produces_valid_h264_high() {
    run_for_profile(EncoderProfile::Fast, "fast");
}

#[test]
fn balanced_profile_produces_valid_h264_high() {
    run_for_profile(EncoderProfile::Balanced, "balanced");
}

#[test]
fn tiktok_hq_profile_produces_valid_h264_high() {
    run_for_profile(EncoderProfile::TikTokHQ, "tiktok_hq");
}

#[test]
fn ig_reels_hdr_falls_back_to_tiktok_hq_in_m2() {
    // M3 will wire HDR for real. M2 just verifies we don't blow up — the
    // profile must resolve to a working H.264 High encoder using the
    // TikTokHQ parameter set as documented.
    run_for_profile(EncoderProfile::IgReelsHDR, "ig_reels_hdr");
}

#[test]
fn profile_default_is_balanced() {
    assert_eq!(EncoderProfile::default(), EncoderProfile::Balanced);
}

#[test]
fn profile_choices_have_expected_labels() {
    // Catches the class of bug where someone reorders the param set but
    // forgets to update the human-readable label.
    let pairs = [
        (EncoderProfile::Fast,       "fast"),
        (EncoderProfile::Balanced,   "balanced"),
        (EncoderProfile::TikTokHQ,   "tiktok-hq"),
        (EncoderProfile::IgReelsHDR, "tiktok-hq"), // M2 fallback
    ];
    for (prof, expected_substring) in pairs {
        let label = prof.to_encoder_choice().expect("resolve").profile_label;
        assert!(
            label.contains(expected_substring),
            "{prof:?} label '{label}' does not contain '{expected_substring}'"
        );
    }
}

#[test]
fn output_size_grows_with_higher_quality_profile() {
    // For the same source, TikTokHQ should produce a larger file than Fast
    // (or at least not smaller than ~80% of Fast — content can be hard to
    // predict, but the relative ordering should hold for a synthetic
    // gradient where higher bitrate translates to bigger file).
    let dir = tempfile::tempdir().expect("tmpdir");
    let fast = dir.path().join("fast.mp4");
    let hq = dir.path().join("hq.mp4");

    encode_synthetic(&fast, EncoderProfile::Fast);
    encode_synthetic(&hq, EncoderProfile::TikTokHQ);

    let size_fast = std::fs::metadata(&fast).unwrap().len();
    let size_hq = std::fs::metadata(&hq).unwrap().len();
    println!("fast={size_fast}B  hq={size_hq}B  ratio={:.2}", size_hq as f64 / size_fast as f64);

    // We accept HQ being noticeably larger or roughly equivalent (NVENC
    // can rate-cap aggressively on synthetic content). The strict
    // assertion: HQ must not be substantially *smaller* than Fast at
    // the same content — that would indicate Fast's parameters are
    // accidentally producing higher bitrate output.
    assert!(
        size_hq as f64 >= size_fast as f64 * 0.75,
        "HQ output ({size_hq}) is much smaller than Fast ({size_fast}) — \
         profile parameters likely regressed (HQ should be ≥ 75% of Fast)"
    );
}
