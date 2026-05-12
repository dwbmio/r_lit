//! `CompositeSink` — fan out a single render to multiple sinks
//! concurrently. Returns one DeliveryReceipt per inner sink; partial
//! failure is reported per-sink so the caller can retry only the
//! failed ones.

use crate::{DeliveryReceipt, OutputSink, SinkError};
use async_trait::async_trait;
use gamereel_farm::RenderResult;
use std::sync::Arc;

/// Per-sink delivery outcome.
#[derive(Debug)]
pub struct CompositeOutcome {
    pub sink: &'static str,
    pub result: Result<DeliveryReceipt, SinkError>,
}

pub struct CompositeSink {
    sinks: Vec<Arc<dyn OutputSink>>,
}

impl CompositeSink {
    pub fn new(sinks: Vec<Arc<dyn OutputSink>>) -> Self {
        Self { sinks }
    }

    /// Fan-out delivery; return per-sink outcomes (always Vec<CompositeOutcome>,
    /// not collapsed to a single Result so callers see partial success).
    pub async fn deliver_all(
        &self,
        result: &RenderResult,
        mp4_bytes: &[u8],
    ) -> Vec<CompositeOutcome> {
        let mut outcomes = Vec::with_capacity(self.sinks.len());
        // Run sequentially for simplicity in v0; switch to
        // tokio::join! / FuturesUnordered when the network sinks land.
        for sink in &self.sinks {
            let r = sink.deliver(result, mp4_bytes).await;
            outcomes.push(CompositeOutcome { sink: sink.name(), result: r });
        }
        outcomes
    }
}

/// CompositeSink also implements OutputSink — returns the FIRST
/// success, errors only if every inner sink fails. Useful when the
/// caller wants "deliver to whichever sink works" semantics rather
/// than fan-out.
#[async_trait]
impl OutputSink for CompositeSink {
    fn name(&self) -> &'static str { "composite" }

    async fn deliver(
        &self,
        result: &RenderResult,
        mp4_bytes: &[u8],
    ) -> Result<DeliveryReceipt, SinkError> {
        let mut last_err: Option<SinkError> = None;
        for sink in &self.sinks {
            match sink.deliver(result, mp4_bytes).await {
                Ok(r) => return Ok(r),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| SinkError::Config("composite has no sinks".into())))
    }
}
