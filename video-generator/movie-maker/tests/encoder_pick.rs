//! M1 unit test for `ffmpeg_inc::encoder_pick`.
//!
//! These tests exercise the *selection policy* (priority order, fallbacks,
//! option sets) without depending on any specific hardware encoder being
//! available at test time. Two lanes:
//!   1. Pure policy assertions on the static priority table and `opts_for()`.
//!   2. Real `pick_h264_encoder()` call: must not panic, must return either
//!      a valid HW encoder or libx264.

use ffmpeg_next as ffmpeg;
use movie_maker::ffmpeg_inc;
use movie_maker::ffmpeg_inc::encoder_pick::{
    opts_for, pick_h264_encoder, EncoderPreference, HW_PRIORITY_H264,
};

/// One-time ffmpeg init. Tests can race; OnceLock keeps it cheap.
fn init() {
    static ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        ffmpeg_inc::init_env().expect("ffmpeg init must succeed");
    });
}

#[test]
fn priority_order_puts_nvenc_first() {
    assert_eq!(
        HW_PRIORITY_H264.first().copied(),
        Some("h264_nvenc"),
        "Linux/NVENC branch must list h264_nvenc first"
    );
    assert!(
        HW_PRIORITY_H264.contains(&"h264_videotoolbox"),
        "videotoolbox must remain in fallback chain for macOS users"
    );
    // No duplicates allowed — would silently shadow priority intent.
    let mut sorted: Vec<&str> = HW_PRIORITY_H264.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), HW_PRIORITY_H264.len(), "no duplicate codecs in priority table");
}

#[test]
fn opts_for_nvenc_uses_balanced_quality_knobs() {
    let c = opts_for("h264_nvenc");
    assert_eq!(c.codec_name, "h264_nvenc");
    assert_eq!(c.profile_label, "nvenc-balanced");
    let opts: std::collections::HashMap<_, _> = c.opts.into_iter().collect();
    assert_eq!(opts.get("preset").map(String::as_str), Some("p4"));
    assert_eq!(opts.get("rc").map(String::as_str), Some("vbr"));
    assert_eq!(opts.get("cq").map(String::as_str), Some("23"));
    assert_eq!(opts.get("profile").map(String::as_str), Some("high"));
    assert_eq!(opts.get("tune").map(String::as_str), Some("hq"));
    assert!(opts.contains_key("b:v"), "bitrate target required");
    assert!(opts.contains_key("maxrate"), "maxrate required for VBR");
}

#[test]
fn opts_for_libx264_uses_medium_crf() {
    let c = opts_for("libx264");
    assert_eq!(c.codec_name, "libx264");
    assert_eq!(c.profile_label, "libx264-medium");
    let opts: std::collections::HashMap<_, _> = c.opts.into_iter().collect();
    assert_eq!(opts.get("preset").map(String::as_str), Some("medium"));
    assert_eq!(opts.get("crf").map(String::as_str), Some("23"));
}

#[test]
fn opts_for_intel_qsv_uses_nv12() {
    let c = opts_for("h264_qsv");
    assert_eq!(c.pixel_format, ffmpeg::format::Pixel::NV12);
}

#[test]
fn opts_for_vaapi_uses_nv12() {
    let c = opts_for("h264_vaapi");
    assert_eq!(c.pixel_format, ffmpeg::format::Pixel::NV12);
}

#[test]
fn pick_software_x264_returns_libx264_when_compiled_in() {
    init();
    // libx264 is universally compiled into Ubuntu's ffmpeg package; if it
    // isn't, the test environment is broken and we want a loud failure.
    if ffmpeg::codec::encoder::find_by_name("libx264").is_none() {
        panic!("test prereq: libx264 must be linked into ffmpeg");
    }
    let pick = pick_h264_encoder(EncoderPreference::SoftwareX264).expect("software pick");
    assert_eq!(pick.codec_name, "libx264");
}

#[test]
fn pick_auto_balanced_returns_a_real_encoder() {
    init();
    let pick = pick_h264_encoder(EncoderPreference::AutoBalanced).expect("auto pick");
    // Whatever was returned must actually exist in linked ffmpeg.
    assert!(
        ffmpeg::codec::encoder::find_by_name(pick.codec_name).is_some(),
        "selected encoder '{}' must resolve via find_by_name",
        pick.codec_name
    );
    // Must come from our priority table or be the libx264 fallback.
    let allowed: Vec<&str> = HW_PRIORITY_H264.iter().copied().chain(["libx264"]).collect();
    assert!(
        allowed.contains(&pick.codec_name),
        "auto-pick returned {} which is not in {allowed:?}",
        pick.codec_name
    );
    // On the linux-nvenc-refactor branch with an installed NVIDIA driver we
    // expect h264_nvenc to win. This is environment-conditional so we only
    // assert it when the binary is actually present.
    if ffmpeg::codec::encoder::find_by_name("h264_nvenc").is_some() {
        assert_eq!(
            pick.codec_name, "h264_nvenc",
            "with NVENC available it must be the auto-balanced winner"
        );
    }
}
