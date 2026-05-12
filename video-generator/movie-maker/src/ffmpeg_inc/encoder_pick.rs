//! Encoder auto-selection (M1).
//!
//! Looks up encoders by name in priority order and returns the first one
//! that the linked ffmpeg actually has. Linux/NVENC is first-class on this
//! branch; the rest are best-effort fallbacks (videotoolbox = macOS,
//! qsv = Intel, vaapi = generic Linux, libx264 = software).
//!
//! For M1 we ship a single "Balanced" preference. M2 will introduce a richer
//! [`EncoderProfile`] enum (Fast/Balanced/TikTokHQ/IgReelsHDR) with VMAF-tuned
//! parameter sets per encoder.

use ffmpeg_next as ffmpeg;

use crate::error::MovieError;

/// Caller intent that influences both encoder pick and parameter set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderPreference {
    /// M1 default: best available H.264 hardware encoder, balanced quality.
    AutoBalanced,
    /// Force libx264 medium (used by baseline benches and CI on hosts
    /// without GPU acceleration).
    SoftwareX264,
}

/// Codec selection result. `opts` are passed to `Encoder::open_with` as a
/// `ffmpeg::Dictionary`. Pixel format is what the encoder consumes.
#[derive(Debug, Clone)]
pub struct EncoderChoice {
    pub codec_name: &'static str,
    pub pixel_format: ffmpeg::format::Pixel,
    pub opts: Vec<(&'static str, String)>,
    pub profile_label: &'static str,
}

/// Names searched, in priority order, when [`EncoderPreference::AutoBalanced`]
/// is requested. Public so the encoder_pick test can introspect it.
pub const HW_PRIORITY_H264: &[&str] = &[
    "h264_nvenc",        // NVIDIA — Linux first-class on this branch
    "h264_qsv",          // Intel Quick Sync (when iGPU is enabled)
    "h264_vaapi",        // generic Linux VAAPI fallback
    "h264_videotoolbox", // macOS (best-effort, unverified on this branch)
];

/// Picks the best H.264 encoder we can find linked into ffmpeg, given the
/// caller's preference. Returns Err if nothing acceptable is available.
pub fn pick_h264_encoder(pref: EncoderPreference) -> Result<EncoderChoice, MovieError> {
    if pref == EncoderPreference::SoftwareX264 {
        return libx264_choice();
    }

    for name in HW_PRIORITY_H264 {
        if ffmpeg::codec::encoder::find_by_name(name).is_some() {
            return Ok(opts_for(name));
        }
    }
    // No HW encoder available — fall back to libx264 if present.
    if ffmpeg::codec::encoder::find_by_name("libx264").is_some() {
        return libx264_choice();
    }
    Err(MovieError::CustomError(
        "no H.264 encoder available in linked ffmpeg".into(),
    ))
}

/// Returns the encoder option set for a given encoder name. Public for tests.
pub fn opts_for(name: &str) -> EncoderChoice {
    match name {
        "h264_nvenc" => EncoderChoice {
            codec_name: "h264_nvenc",
            pixel_format: ffmpeg::format::Pixel::YUV420P,
            // M1 balanced. M2 grid-search will refine these.
            opts: vec![
                ("preset", "p4".into()),
                ("tune", "hq".into()),
                ("rc", "vbr".into()),
                ("cq", "23".into()),
                ("b:v", "8M".into()),
                ("maxrate", "12M".into()),
                ("bufsize", "16M".into()),
                ("profile", "high".into()),
                ("bf", "3".into()),
            ],
            profile_label: "nvenc-balanced",
        },
        "h264_qsv" => EncoderChoice {
            codec_name: "h264_qsv",
            pixel_format: ffmpeg::format::Pixel::NV12,
            opts: vec![
                ("preset", "medium".into()),
                ("global_quality", "23".into()),
                ("b:v", "8M".into()),
            ],
            profile_label: "qsv-balanced",
        },
        "h264_vaapi" => EncoderChoice {
            codec_name: "h264_vaapi",
            pixel_format: ffmpeg::format::Pixel::NV12,
            opts: vec![
                ("rc_mode", "VBR".into()),
                ("b:v", "8M".into()),
                ("qp", "23".into()),
            ],
            profile_label: "vaapi-balanced",
        },
        "h264_videotoolbox" => EncoderChoice {
            codec_name: "h264_videotoolbox",
            pixel_format: ffmpeg::format::Pixel::YUV420P,
            opts: vec![("b:v", "8M".into()), ("profile", "high".into())],
            profile_label: "videotoolbox-balanced",
        },
        "libx264" => libx264_choice().expect("libx264 always present in this arm"),
        other => EncoderChoice {
            codec_name: Box::leak(other.to_string().into_boxed_str()),
            pixel_format: ffmpeg::format::Pixel::YUV420P,
            opts: vec![],
            profile_label: "unknown",
        },
    }
}

fn libx264_choice() -> Result<EncoderChoice, MovieError> {
    if ffmpeg::codec::encoder::find_by_name("libx264").is_none() {
        return Err(MovieError::CustomError(
            "libx264 not available in linked ffmpeg".into(),
        ));
    }
    Ok(EncoderChoice {
        codec_name: "libx264",
        pixel_format: ffmpeg::format::Pixel::YUV420P,
        opts: vec![
            ("preset", "medium".into()),
            ("crf", "23".into()),
        ],
        profile_label: "libx264-medium",
    })
}
