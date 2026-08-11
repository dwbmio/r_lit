//! audiogen — Pure-Rust audio pipeline for game assets.
//!
//! Architecture:
//!   - [`codec`] — unified `Codec` trait; one impl per format (mp3/wav/ogg/...).
//!   - [`synth`] — procedural SFX generators (zero network, deterministic).
//!   - [`jobs`] — JSON job descriptors per game.
//!   - [`ssh`]   — pure-Rust SSH client (russh) → VPS redis-cli queue.
//!   - Pipeline: [`pipeline::submit`] (LPUSH, async) / [`pipeline::collect`] (BRPOP).
//!
//! v1 limits (see README):
//!   - No resampling / no normalization / no MP3 re-encoding.
//!   - AI results land as-is (MiniMax returns MP3 at requested bitrate/sample_rate).
//!   - OGG encoding = NotImplemented (pure-Rust Vorbis encoder unavailable).

pub mod codec;
pub mod jobs;
pub mod synth;
pub mod ssh;
pub mod pipeline;
pub mod r2;
pub mod hfrog;
pub mod redis_q;

pub use codec::{Codec, Pcm, EncodeCfg, Mp3Codec, WavCodec, OggCodec, codec_for_ext, decode_any};
pub use jobs::{JobsFile, JobSpec, JobKind};
pub use r2::{R2Client, Uploaded, default_key};
pub use hfrog::{HfrogClient, DEFAULT_HFROG_BASE, runtime_for_game, validate_runtime};
pub use redis_q::{RedisQueue, DEFAULT_QUEUE, DEFAULT_RESULT};