//! `RenderJob` and `RenderResult` — the data on the wire between the
//! CLI / orchestrator and the worker pool. Designed to be both:
//!   * cheap to clone (Arc<scene_meta_path: PathBuf>),
//!   * JSON-serializable (so a future RemoteWorker can ship them
//!     across a gRPC boundary unmodified — the Cloud-ready interface),
//!   * stable: every field is `#[serde(default)]` for forward-compat.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Higher = more important. Workers pick the highest-priority pending
/// job from the queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPriority {
    Low,
    Normal,
    High,
    Critical,
}
impl Default for JobPriority {
    fn default() -> Self { JobPriority::Normal }
}

/// One render request. JSON shape (one job per line in jobs.jsonl):
/// ```json
/// {
///   "id": "battle-2026-05-12-001",
///   "scene_meta_path": "tests/hs-proj/scene.meta",
///   "output_path": "/tmp/out/battle-001.mp4",
///   "priority": "normal"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderJob {
    /// Caller-chosen identifier; shows up in logs and progress reports.
    pub id: String,

    /// Path to the scene.meta file driving this render. Resolved
    /// relative to `RuntimeCtx::source_path` (caller convention).
    pub scene_meta_path: PathBuf,

    /// Where the worker should write the resulting MP4.
    pub output_path: PathBuf,

    /// Resolves texture paths inside scene.meta. If `None`, defaults to
    /// `scene_meta_path.parent().parent()` — the convention used by the
    /// hs-mvp scene where scene.meta sits at `<root>/tests/<scene>/scene.meta`
    /// and asset paths inside read `tests/<scene>/...`.
    /// Set explicitly to avoid surprises.
    #[serde(default)]
    pub source_root: Option<PathBuf>,

    /// Width / height / fps / duration override. None = use scene defaults
    /// (currently 720x1080 @ 30 fps × 10 s baked into hs-mvp).
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub duration_s: Option<u64>,

    #[serde(default)]
    pub priority: JobPriority,

    /// Free-form metadata the caller wants to pass through to the
    /// RenderResult — useful for correlating with upstream systems.
    #[serde(default)]
    pub tag: serde_json::Value,
}

/// What the worker reports when a job finishes (or fails).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderResult {
    /// Echoed from `RenderJob.id`.
    pub job_id: String,

    /// Worker that handled this job (e.g. "local-3").
    pub worker_id: String,

    pub ok: bool,

    /// Set when `ok = true`. Final on-disk size of the output mp4.
    #[serde(default)]
    pub output_bytes: u64,

    /// Total wall time for this job, including queue wait if reported by
    /// the queue actor wrapper.
    #[serde(with = "duration_ms")]
    pub wall: Duration,

    /// Pure render-loop time (encoder.loop) — useful for diagnosing
    /// whether queue backpressure is biting.
    #[serde(with = "duration_ms")]
    pub render_loop: Duration,

    /// On `ok = false`, plain-text reason. None on success.
    #[serde(default)]
    pub error: Option<String>,

    /// Echoed from RenderJob.tag.
    #[serde(default)]
    pub tag: serde_json::Value,
}

mod duration_ms {
    use serde::{Deserializer, Serializer};
    use std::time::Duration;
    pub fn serialize<S: Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(d.as_millis() as u64)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
        use serde::Deserialize;
        let ms = u64::deserialize(d)?;
        Ok(Duration::from_millis(ms))
    }
}
