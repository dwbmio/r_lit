//! OGG/Vorbis codec — encode NOT IMPLEMENTED (v1).
//!
//! Pure-Rust Vorbis encoder availability: no production-quality crate at the time
//! of writing. v2 can add symphonia-backed decode + a future encoder.

use super::{Codec, EncodeCfg, Pcm};
use anyhow::{anyhow, Result};

pub struct OggCodec;

impl Codec for OggCodec {
    fn ext(&self) -> &'static str { "ogg" }
    fn name(&self) -> &'static str { "ogg/vorbis (encode TODO; pure-Rust encoder unavailable)" }

    fn encode(&self, _pcm: &Pcm, _cfg: &EncodeCfg) -> Result<Vec<u8>> {
        Err(anyhow!(
            "OGG/Vorbis encoding not implemented in v1 (no pure-Rust encoder available). \
             Use MP3 or WAV output, or post-process via external tooling."
        ))
    }
}