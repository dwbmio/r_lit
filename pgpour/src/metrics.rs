use opentelemetry::metrics::{Counter, Histogram};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use std::sync::Arc;

/// CDC pipeline metrics exported via OTLP.
#[derive(Clone)]
pub struct CdcMetrics {
    /// CDC events by operation type (insert/update/delete).
    pub events_total: Counter<u64>,
    /// Total events produced to Kafka (streaming phase).
    pub streaming_events: Counter<u64>,
    /// Number of streaming batches produced to Kafka.
    pub streaming_batches: Counter<u64>,
    /// Events per batch (for tuning batch config).
    pub produce_batch_size: Histogram<f64>,
    /// Per-message Kafka produce latency.
    pub produce_duration_ms: Histogram<f64>,
    /// Kafka produce failures.
    pub produce_errors: Counter<u64>,
    /// Total bytes produced to Kafka.
    pub produce_bytes: Counter<u64>,
    /// Rows skipped during initial sync (incremental-only mode).
    pub init_sync_rows: Counter<u64>,
    provider: Arc<SdkMeterProvider>,
}

impl CdcMetrics {
    pub fn init(endpoint: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()?;

        let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter).build();

        let provider = SdkMeterProvider::builder().with_reader(reader).build();
        opentelemetry::global::set_meter_provider(provider.clone());

        let meter = opentelemetry::global::meter("pgpour");

        Self::register_jemalloc_gauges(&meter);

        Ok(Self {
            events_total: meter
                .u64_counter("cdc.events")
                .with_description("CDC events by operation type")
                .build(),
            streaming_events: meter
                .u64_counter("cdc.streaming.events")
                .with_description("Total events produced to Kafka in streaming phase")
                .build(),
            streaming_batches: meter
                .u64_counter("cdc.streaming.batches")
                .with_description("Streaming event batches produced to Kafka")
                .build(),
            produce_batch_size: meter
                .f64_histogram("cdc.kafka.produce.batch_size")
                .with_description("Events per produce batch")
                .build(),
            produce_duration_ms: meter
                .f64_histogram("cdc.kafka.produce.duration")
                .with_description("Per-message Kafka produce latency")
                .with_unit("ms")
                .build(),
            produce_errors: meter
                .u64_counter("cdc.kafka.produce.errors")
                .with_description("Kafka produce failures")
                .build(),
            produce_bytes: meter
                .u64_counter("cdc.kafka.produce.bytes")
                .with_description("Total bytes produced to Kafka")
                .with_unit("By")
                .build(),
            init_sync_rows: meter
                .u64_counter("cdc.init_sync.rows")
                .with_description("Rows skipped during initial table sync (incremental-only)")
                .build(),
            provider: Arc::new(provider),
        })
    }

    /// Register jemalloc memory gauges as observable instruments.
    /// Callbacks are held by the meter provider — no need to store handles.
    fn register_jemalloc_gauges(meter: &opentelemetry::metrics::Meter) {
        meter
            .u64_observable_gauge("process.memory.jemalloc.allocated")
            .with_description("jemalloc: bytes allocated by the application")
            .with_unit("By")
            .with_callback(|gauge| {
                let _ = tikv_jemalloc_ctl::epoch::advance();
                if let Ok(v) = tikv_jemalloc_ctl::stats::allocated::read() {
                    gauge.observe(v as u64, &[]);
                }
            })
            .build();

        meter
            .u64_observable_gauge("process.memory.jemalloc.resident")
            .with_description("jemalloc: bytes in physically resident pages (RSS)")
            .with_unit("By")
            .with_callback(|gauge| {
                let _ = tikv_jemalloc_ctl::epoch::advance();
                if let Ok(v) = tikv_jemalloc_ctl::stats::resident::read() {
                    gauge.observe(v as u64, &[]);
                }
            })
            .build();

        meter
            .u64_observable_gauge("process.memory.jemalloc.active")
            .with_description("jemalloc: bytes in active extents (committed memory)")
            .with_unit("By")
            .with_callback(|gauge| {
                let _ = tikv_jemalloc_ctl::epoch::advance();
                if let Ok(v) = tikv_jemalloc_ctl::stats::active::read() {
                    gauge.observe(v as u64, &[]);
                }
            })
            .build();
    }

    pub fn shutdown(&self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::warn!(%e, "OTel meter provider shutdown error");
        }
    }
}
