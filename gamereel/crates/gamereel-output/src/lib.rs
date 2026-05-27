//! gamereel-output — where rendered MP4s go.
//!
//! Sinks decouple "we have a finished video" from "deliver it
//! somewhere". The trait is async because every realistic sink does
//! network IO. Built-in implementations:
//!
//!   * [`LocalDiskSink`] — write to a local path. Useful for the demo
//!     and for staging before a more interesting sink takes over.
//!   * [`ObjectStorageSink`] — upload to any S3-compatible endpoint
//!     (AWS S3 / Aliyun OSS / GCP-via-S3 / MinIO / Cloudflare R2),
//!     configured entirely via env vars so the same binary runs in
//!     any cloud without code changes.
//!
//! Future sinks (TikTok / IG / WeChat push) implement the same trait
//! and get composed via [`CompositeSink`] for "do all of these in
//! parallel" semantics.
//!
//! Wire shape: `OutputSink::deliver(receipt_meta, mp4_bytes)` returns
//! a [`DeliveryReceipt`] carrying the durable URL / key / push id.
//! `gamereel-farm`'s `RenderResult` is augmented per call so the
//! caller can correlate deliveries back to job ids.

pub mod local_disk;
pub mod object_storage;
pub mod composite;

use async_trait::async_trait;
use gamereel_farm::RenderResult;
use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug)]
pub enum SinkError {
    #[error("sink misconfigured: {0}")]
    Config(String),
    #[error("io: {0}")]
    Io(String),
    #[error("transport: {0}")]
    Transport(String),
}

/// What a sink hands back. JSON-serializable so a downstream notifier
/// (out of scope here) can ship it across a queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReceipt {
    pub job_id: String,
    pub sink: &'static str,
    /// Durable URL / object key / push id — depends on the sink.
    pub location: String,
    pub bytes: u64,
    /// Per-sink free-form metadata for downstream consumers.
    #[serde(default)]
    pub extra: serde_json::Value,
}

#[async_trait]
pub trait OutputSink: Send + Sync {
    fn name(&self) -> &'static str;

    /// Deliver the rendered video. `result` carries the job_id and
    /// timing metadata; `mp4_bytes` is the encoded MP4 payload.
    async fn deliver(
        &self,
        result: &RenderResult,
        mp4_bytes: &[u8],
    ) -> Result<DeliveryReceipt, SinkError>;
}

pub use composite::CompositeSink;
pub use local_disk::LocalDiskSink;
pub use object_storage::{ObjectStorageConfig, ObjectStorageSink};
