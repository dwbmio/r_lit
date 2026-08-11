//! FLAC codec — encode + decode.
//!
//! Encode: `flacenc` 0.5 (pure Rust, lossless).
//! Decode: `symphonia` via the trait default (it has FLAC support).

use super::{Codec, EncodeCfg, Pcm};
use anyhow::{anyhow, Result};
use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::config::Encoder;
use flacenc::error::Verify;
use flacenc::source::MemSource;

pub struct FlacCodec;

impl Codec for FlacCodec {
    fn ext(&self) -> &'static str { "flac" }
    fn name(&self) -> &'static str { "flac (Free Lossless Audio Codec, pure Rust via flacenc)" }

    fn encode(&self, pcm: &Pcm, _cfg: &EncodeCfg) -> Result<Vec<u8>> {
        const BLOCK: usize = 4096;

        if pcm.channels == 0 || pcm.channels > 2 {
            return Err(anyhow!("FLAC supports 1 or 2 channels, got {}", pcm.channels));
        }
        let sr = pcm.sample_rate;
        if !matches!(sr, 8000 | 11025 | 16000 | 22050 | 24000 | 32000 | 44100 | 48000 | 88200 | 96000 | 176400 | 192000) {
            return Err(anyhow!("FLAC unsupported sample rate {sr} (must be 8k/11k/16k/22k/24k/32k/44.1k/48k/88.2k/96k/176.4k/192k)"));
        }

        // Deinterleave + pad each channel to multiple of BLOCK.
        let frames = pcm.samples_per_channel();
        let total = frames.div_ceil(BLOCK) * BLOCK;
        // FLAC's MemSource wants interleaved i32 samples (LRLRLR…) — not planar.
        let mut interleaved: Vec<i32> = Vec::with_capacity(total * pcm.channels as usize);
        for f in 0..total {
            for c in 0..pcm.channels as usize {
                let s = if f < frames {
                    pcm.samples[f * pcm.channels as usize + c] as i32
                } else {
                    0
                };
                interleaved.push(s);
            }
        }

        let source = MemSource::from_samples(
            &interleaved,
            pcm.channels as usize,
            16,
            sr as usize,
        );
        let verified = Encoder::default()
            .into_verified()
            .map_err(|e| anyhow!("flacenc verify: {e:?}"))?;
        let stream = flacenc::encode_with_fixed_block_size(&verified, source, BLOCK)
            .map_err(|e| anyhow!("flacenc encode: {e:?}"))?;
        // Write the FLAC stream bits into a ByteSink to get raw bytes.
        let mut sink = ByteSink::new();
        stream.write(&mut sink)
            .map_err(|e| anyhow!("flacenc bitwrite: {e:?}"))?;
        Ok(sink.into_inner())
    }

    // decode is provided by the trait default → symphonia
}