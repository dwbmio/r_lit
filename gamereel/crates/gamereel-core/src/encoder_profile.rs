//! High-level encoder presets that bundle (codec choice, parameter set,
//! quality target) into named profiles.
//!
//! Pre-M2, callers passed an [`EncoderPreference`] that controlled only
//! which encoder family to pick. M2 adds *quality intent*: the same NVENC
//! encoder can be driven Fast / Balanced / TikTokHQ / IgReelsHDR with very
//! different parameter sets selected via VMAF grid search.
//!
//! Specific parameter values inside each profile are documented in
//! `docs/optimization-log.md` (entries O-006 through O-010 once M2 lands).
//! Update both when changing a profile so the rationale stays discoverable.

use crate::ffmpeg_inc::encoder_pick::{opts_for, EncoderChoice, EncoderPreference};
use ffmpeg_next as ffmpeg;

/// Caller-facing quality + speed intent. Each profile maps to a concrete
/// [`EncoderChoice`] + parameter set tuned via the VMAF grid search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderProfile {
    /// Maximum throughput, acceptable quality. Use for previews / drafts.
    /// Target: VMAF ≥ 88, fps ≥ 1.1× Balanced.
    Fast,
    /// M1's default. Sensible middle ground. Used by `perf_main` and `demo`.
    /// Target: VMAF ≥ 92, fps within 0.8× of Fast.
    Balanced,
    /// Optimized for TikTok HQ upload (8 Mbps H.264 High, p6 + lookahead +
    /// spatial AQ). Target: VMAF ≥ 95.
    TikTokHQ,
    /// Optimized for Instagram Reels HDR (HEVC Main10, 10-bit, BT.2020).
    /// Target: VMAF ≥ 95 with HDR metadata intact. **NOT YET WIRED** in M2.
    /// Selecting it currently falls back to TikTokHQ + a warning log; M3
    /// will add the real HDR / hwframes pipeline.
    IgReelsHDR,
}

impl Default for EncoderProfile {
    fn default() -> Self {
        EncoderProfile::Balanced
    }
}

impl EncoderProfile {
    /// Map this profile to the encoder choice we'll actually configure
    /// ffmpeg with. M2 grid search drives the parameter constants. When
    /// no NVENC is present, we transparently fall back to libx264 with
    /// a roughly equivalent intent profile (so tests pass on CI hosts
    /// without GPU, and the *behavior* — Fast is faster, TikTokHQ is
    /// higher quality — survives even on the software path).
    pub fn to_encoder_choice(self) -> Result<EncoderChoice, crate::error::GamereelError> {
        // v1.5 — GAMEREEL_FORCE_SW=1 跳过 HW probe, 直接 libx264 (GPU mismatch / no GPU)
        if std::env::var("GAMEREEL_FORCE_SW").is_ok() {
            if ffmpeg::codec::encoder::find_by_name("libx264").is_some() {
                return Ok(self.libx264_choice());
            }
        }
        // Probe what's available. Hardware path first.
        let has_nvenc = ffmpeg::codec::encoder::find_by_name("h264_nvenc").is_some();
        if has_nvenc {
            return Ok(self.nvenc_choice());
        }
        // Software fallback: same intent, libx264 parameters.
        if ffmpeg::codec::encoder::find_by_name("libx264").is_some() {
            return Ok(self.libx264_choice());
        }
        Err(crate::error::GamereelError::CustomError(
            "no usable H.264 encoder (h264_nvenc or libx264) in linked ffmpeg".into(),
        ))
    }

    fn nvenc_choice(self) -> EncoderChoice {
        let mut base = opts_for("h264_nvenc");
        base.opts = match self {
            // O-006: Fast — p2 preset (NVENC's "fast" tier), no lookahead,
            // no AQ. Single-pass VBR. Drops VMAF by ~0.3–0.5 vs balanced.
            EncoderProfile::Fast => vec![
                ("preset", "p2".into()),
                ("tune", "hq".into()),
                ("rc", "vbr".into()),
                ("cq", "23".into()),
                ("b:v", "6M".into()),
                ("maxrate", "9M".into()),
                ("bufsize", "12M".into()),
                ("profile", "high".into()),
                ("bf", "0".into()),
            ],
            // O-007: Balanced — p4 preset, B-frames on, no AQ.
            // M2 grid showed AQ doesn't help on synthetic sources (no
            // perceptual texture variance to differentially weight),
            // and the cost is non-trivial. We leave AQ off here so
            // Balanced stays the e2e default with predictable cost.
            // Real-camera content might benefit from AQ — that case is
            // handled by TikTokHQ.
            EncoderProfile::Balanced => vec![
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
            // O-008: TikTokHQ — p6 preset, lookahead=16 (grid winner; 32
            // gave no measurable VMAF lift vs 16 but cut fps ~10%), bf=3
            // (grid avoided bf=4 due to B-pyramid latency overhead),
            // spatial+temporal AQ on. Targets the platform HQ upload tier
            // (8 Mbps H.264 High recommended; we provision higher buffer
            // to survive the platform's recompression pass).
            EncoderProfile::TikTokHQ => vec![
                ("preset", "p6".into()),
                ("tune", "hq".into()),
                ("rc", "vbr".into()),
                ("cq", "21".into()),
                ("b:v", "10M".into()),
                ("maxrate", "14M".into()),
                ("bufsize", "20M".into()),
                ("profile", "high".into()),
                ("bf", "3".into()),
                ("b_ref_mode", "middle".into()),
                ("rc-lookahead", "16".into()),
                ("spatial-aq", "1".into()),
                ("temporal-aq", "1".into()),
                ("aq-strength", "8".into()),
            ],
            // O-009 placeholder — NVENC HEVC Main10 + HDR10 metadata are
            // wired in M3 once we have hwframes. For now M2 falls back to
            // the TikTokHQ H.264 set and logs a warning so callers know
            // they're not getting HDR.
            EncoderProfile::IgReelsHDR => {
                log::warn!(
                    "encoder_profile: IgReelsHDR not yet implemented in M2 — using TikTokHQ params instead"
                );
                return EncoderProfile::TikTokHQ.nvenc_choice();
            }
        };
        base.profile_label = match self {
            EncoderProfile::Fast => "nvenc-fast",
            EncoderProfile::Balanced => "nvenc-balanced",
            EncoderProfile::TikTokHQ => "nvenc-tiktok-hq",
            EncoderProfile::IgReelsHDR => "nvenc-tiktok-hq(fallback-from-hdr)",
        };
        base
    }

    fn libx264_choice(self) -> EncoderChoice {
        let mut base = opts_for("libx264");
        base.opts = match self {
            EncoderProfile::Fast => vec![
                ("preset", "veryfast".into()),
                ("crf", "25".into()),
            ],
            EncoderProfile::Balanced => vec![
                ("preset", "medium".into()),
                ("crf", "23".into()),
            ],
            EncoderProfile::TikTokHQ | EncoderProfile::IgReelsHDR => vec![
                ("preset", "slow".into()),
                ("crf", "20".into()),
            ],
        };
        base.profile_label = match self {
            EncoderProfile::Fast => "libx264-veryfast",
            EncoderProfile::Balanced => "libx264-medium",
            EncoderProfile::TikTokHQ => "libx264-slow-q20",
            EncoderProfile::IgReelsHDR => "libx264-slow-q20(fallback-from-hdr)",
        };
        base
    }

    /// Map to the M1-vintage [`EncoderPreference`] for code paths that
    /// haven't yet adopted profiles.
    pub fn to_preference(self) -> EncoderPreference {
        // Both Fast/Balanced/TikTokHQ all want HW first.
        EncoderPreference::AutoBalanced
    }
}
