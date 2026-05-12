//! `LocalDiskSink` — write the rendered MP4 bytes to a local path.
//! Used by demos and as a staging sink before something more
//! interesting (object storage, push channels) takes over.

use crate::{DeliveryReceipt, OutputSink, SinkError};
use async_trait::async_trait;
use gamereel_farm::RenderResult;
use std::path::PathBuf;

pub struct LocalDiskSink {
    /// Output directory. Filename is derived from `job_id`.
    pub root: PathBuf,
}

impl LocalDiskSink {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl OutputSink for LocalDiskSink {
    fn name(&self) -> &'static str { "local_disk" }

    async fn deliver(
        &self,
        result: &RenderResult,
        mp4_bytes: &[u8],
    ) -> Result<DeliveryReceipt, SinkError> {
        tokio::fs::create_dir_all(&self.root)
            .await
            .map_err(|e| SinkError::Io(format!("mkdir {}: {e}", self.root.display())))?;
        let safe_name = sanitize(&result.job_id);
        let path = self.root.join(format!("{safe_name}.mp4"));
        tokio::fs::write(&path, mp4_bytes)
            .await
            .map_err(|e| SinkError::Io(format!("write {}: {e}", path.display())))?;
        Ok(DeliveryReceipt {
            job_id: result.job_id.clone(),
            sink: "local_disk",
            location: path.to_string_lossy().to_string(),
            bytes: mp4_bytes.len() as u64,
            extra: serde_json::Value::Null,
        })
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}
