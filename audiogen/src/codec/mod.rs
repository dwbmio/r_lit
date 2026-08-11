//! Unified audio codec interface.
//!
//! v1: encode only. Decode is declared in the trait but returns `NotImplemented`
//! everywhere. v1 pipeline (AI results land as MP3 → save; procedural → MP3) does
//! not require re-encoding. Adding decode = add symphonia + implement.
//!
//! Adding a new format = one impl file, no other changes required.

pub mod decode;
pub mod mp3;
pub mod wav;
pub mod ogg;
pub mod flac;

pub use mp3::Mp3Codec;
pub use wav::WavCodec;
pub use ogg::OggCodec;
pub use flac::FlacCodec;

use anyhow::Result;
use std::fmt;

/// In-memory PCM audio buffer (signed 16-bit, interleaved).
#[derive(Debug, Clone)]
pub struct Pcm {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Pcm {
    pub fn new(samples: Vec<i16>, sample_rate: u32, channels: u16) -> Self {
        Self { samples, sample_rate, channels }
    }

    pub fn empty(sample_rate: u32, channels: u16) -> Self {
        Self { samples: Vec::new(), sample_rate, channels }
    }

    pub fn duration_secs(&self) -> f64 {
        let frames = self.samples.len() as f64 / self.channels as f64;
        if self.sample_rate == 0 { 0.0 } else { frames / self.sample_rate as f64 }
    }

    pub fn samples_per_channel(&self) -> usize {
        self.samples.len() / self.channels as usize
    }
}

/// Encode configuration. Bitrate is meaningful only for lossy codecs.
#[derive(Debug, Clone)]
pub struct EncodeCfg {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u32, // bits per second; 0 = codec default
}

impl EncodeCfg {
    pub fn mp3(sample_rate: u32, channels: u16, bitrate: u32) -> Self {
        Self { sample_rate, channels, bitrate }
    }
    pub fn wav(sample_rate: u32, channels: u16) -> Self {
        Self { sample_rate, channels, bitrate: 0 }
    }
}

/// Unified codec trait — every audio format plugs in here.
///
/// v1: `decode` returns `Err`. v2: add symphonia-backed decoder.
pub trait Codec: Send + Sync {
    /// File extension WITHOUT leading dot (e.g. "mp3", "wav", "ogg").
    fn ext(&self) -> &'static str;

    /// Human-readable codec name (for logs).
    fn name(&self) -> &'static str { self.ext() }

    /// Encode PCM → format bytes.
    fn encode(&self, pcm: &Pcm, cfg: &EncodeCfg) -> Result<Vec<u8>>;

    /// Decode format bytes → PCM. v1 returns `NotImplemented`.
    fn decode(&self, _bytes: &[u8]) -> Result<Pcm> {
        Err(anyhow::anyhow!(
            "decode not implemented in v1 for {} (v2 will add symphonia-backed decoder)",
            self.ext()
        ))
    }
}

/// Dispatch a codec by file extension.
pub fn codec_for_ext(ext: &str) -> Result<Box<dyn Codec>> {
    let lower = ext.to_ascii_lowercase();
    Ok(match lower.as_str() {
        "mp3" => Box::new(Mp3Codec),
        "wav" => Box::new(WavCodec),
        "ogg" => Box::new(OggCodec),
        "flac" => Box::new(FlacCodec),
        "aac" | "m4a" => Box::new(AacCodecStub),
        other => return Err(anyhow::anyhow!("unsupported audio format: .{other}")),
    })
}

/// AAC/M4A codec — decode supported (symphonia), encode NOT IMPLEMENTED in v1.
/// No production-quality pure-Rust AAC encoder exists. iOS/Android/WeChat/Douyin all
/// accept MP3 natively; for AAC encoding, downstream tools (ffmpeg / Apple
/// CoreAudio / Android MediaCodec) are recommended.
pub struct AacCodecStub;
impl Codec for AacCodecStub {
    fn ext(&self) -> &'static str { "aac" }
    fn name(&self) -> &'static str { "aac/m4a (decode-only in v1; use mp3 or external ffmpeg for encode)" }
    fn encode(&self, _pcm: &Pcm, _cfg: &EncodeCfg) -> Result<Vec<u8>> {
        Err(anyhow::anyhow!(
            "AAC encode not implemented in v1 (no production pure-Rust encoder). \
             Use mp3 output (universal) or ffmpeg for AAC."
        ))
    }
}

/// Convenience: decode by extension (auto-dispatch to right codec).
pub use decode::decode_any;

#[derive(Debug)]
pub struct CodecError(pub String);

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CodecError {}