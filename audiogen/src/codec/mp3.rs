//! MP3 codec — encode via `shine-rs`.
//! Decode is a v2 TODO (add symphonia with mp3 feature).

use super::{Codec, EncodeCfg, Pcm};
use anyhow::{anyhow, Result};
use shine_rs::{Mp3Encoder, Mp3EncoderConfig, StereoMode};

pub struct Mp3Codec;

impl Codec for Mp3Codec {
    fn ext(&self) -> &'static str { "mp3" }
    fn name(&self) -> &'static str { "mp3 (MPEG-1 Layer III, shine-rs encoder)" }

    fn encode(&self, pcm: &Pcm, cfg: &EncodeCfg) -> Result<Vec<u8>> {
        if cfg.channels == 0 || cfg.channels > 2 {
            return Err(anyhow!("MP3 supports 1 or 2 channels, got {}", cfg.channels));
        }
        // shine-rs only supports discrete sample rates: pick nearest or error.
        let sr = nearest_supported_sr(cfg.sample_rate)
            .ok_or_else(|| anyhow!("MP3 unsupported sample_rate {} (must be one of {:?})",
                cfg.sample_rate, shine_rs::SUPPORTED_SAMPLE_RATES))?;
        let kbps = (cfg.bitrate / 1000).max(8); // minimum 8 kbps
        let kbps = nearest_supported_bitrate(kbps)
            .ok_or_else(|| anyhow!("MP3 unsupported bitrate {}k (must be one of {:?})",
                kbps, shine_rs::SUPPORTED_BITRATES))?;
        let stereo_mode = if cfg.channels == 2 { StereoMode::Stereo } else { StereoMode::Mono };

        let config = Mp3EncoderConfig::new()
            .sample_rate(sr)
            .bitrate(kbps)
            .channels(cfg.channels as u8)
            .stereo_mode(stereo_mode);
        let mut enc = Mp3Encoder::new(config).map_err(|e| anyhow!("shine-rs init: {e:?}"))?;

        // Feed all PCM samples; the encoder buffers internally.
        let frames = enc.encode_interleaved(&pcm.samples)
            .map_err(|e| anyhow!("shine-rs encode: {e:?}"))?;
        let mut out = Vec::with_capacity(frames.iter().map(|f| f.len()).sum::<usize>() + 256);
        for f in frames {
            out.extend_from_slice(&f);
        }
        let tail = enc.finish().map_err(|e| anyhow!("shine-rs finish: {e:?}"))?;
        out.extend_from_slice(&tail);
        Ok(out)
    }
}

fn nearest_supported_sr(sr: u32) -> Option<u32> {
    shine_rs::SUPPORTED_SAMPLE_RATES.iter()
        .min_by_key(|&&s| (s as i64 - sr as i64).abs())
        .copied()
}

fn nearest_supported_bitrate(kbps: u32) -> Option<u32> {
    shine_rs::SUPPORTED_BITRATES.iter()
        .min_by_key(|&&b| (b as i64 - kbps as i64).abs())
        .copied()
}