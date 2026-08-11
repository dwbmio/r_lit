//! Jobs file schema — one JSON per game (e.g. `jobs/classroom.json`).
//!
//! Shape:
//! ```json
//! {
//!   "game": "classroom",
//!   "style": "古风仙侠 RPG; 竹笛/古筝/箫 + 弦乐; 悠扬空灵; 适配小学课堂教育语境",
//!   "voice_id": "male-qn-jingying",
//!   "output_dir": "origin_res/classroom/audio",
//!   "install_dir": "classroom/classroom/audio",
//!   "jobs": [
//!     { "name": "bgm_lobby", "kind": "music",
//!       "prompt": "...", "sample_rate": 32000, "bitrate": 96000, "channels": 2 },
//!     { "name": "voice_hint", "kind": "voice",
//!       "text": "再想想看", "voice_id": "female-shaonv", "sample_rate": 24000, "bitrate": 32000, "channels": 1 },
//!     { "name": "sfx_click", "kind": "procedural",
//!       "synth": "woodblock", "duration_secs": 0.05, "sample_rate": 22050 }
//!   ]
//! }
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// AI music generation (MiniMax music_generation API).
    Music,
    /// AI voice / TTS (MiniMax t2a_v2 API).
    Voice,
    /// Procedurally synthesized locally via [`crate::synth`].
    Procedural,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobSpec {
    pub name: String,
    pub kind: JobKind,
    // ── music ──
    #[serde(default)]
    pub prompt: Option<String>,
    // ── voice ──
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub voice_id: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    // ── procedural ──
    #[serde(default)]
    pub synth: Option<String>,        // e.g. "woodblock", "ding", "thud"
    #[serde(default)]
    pub duration_secs: Option<f32>,
    // ── common ──
    pub sample_rate: u32,
    pub channels: u16,
    #[serde(default)]
    pub bitrate: u32,                 // 0 = codec default
    /// Output file format (extension). Defaults to "mp3".
    #[serde(default = "default_format")]
    pub format: String,
}

fn default_format() -> String { "mp3".into() }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JobsFile {
    pub game: String,
    /// Default style baseline prepended to every music job's prompt.
    #[serde(default)]
    pub style: Option<String>,
    /// Default voice_id for voice jobs (overridable per-job).
    #[serde(default)]
    pub voice_id: Option<String>,
    /// Where to land generated files. Usually `origin_res/<game>/audio`.
    pub output_dir: String,
    /// Where to copy for Godot import. Usually `<proj>/<game>/audio`.
    #[serde(default)]
    pub install_dir: Option<String>,
    pub jobs: Vec<JobSpec>,
}

impl JobsFile {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("read jobs file: {}", path.display()))?;
        let jf: JobsFile = serde_json::from_str(&raw)
            .with_context(|| format!("parse jobs JSON: {}", path.display()))?;
        Ok(jf)
    }
}