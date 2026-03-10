use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use etl::destination::Destination;
use etl::error::{ErrorKind, EtlError, EtlResult};
use etl::store::both::memory::MemoryStore;
use etl::store::schema::SchemaStore;
use etl::types::{Cell, Event, PgNumeric, TableId, TableRow, TableSchema};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::KafkaAuth;

#[cfg(feature = "metric")]
use crate::metrics::CdcMetrics;
#[cfg(feature = "metric")]
use opentelemetry::KeyValue;

/// Tracks pipeline phase transitions visible from the Destination side.
///
/// init phase:  truncate_table / write_table_rows called per table
/// streaming:   write_events called continuously
#[derive(Clone)]
struct PhaseTracker {
    tables_init_started: Arc<RwLock<HashSet<TableId>>>,
    tables_init_completed: Arc<RwLock<HashSet<TableId>>>,
    init_rows_total: Arc<AtomicU64>,
    streaming_entered: Arc<AtomicBool>,
    streaming_batches: Arc<AtomicU64>,
    streaming_events: Arc<AtomicU64>,
    streaming_start_time: Arc<RwLock<Option<Instant>>>,
}

impl PhaseTracker {
    fn new() -> Self {
        Self {
            tables_init_started: Arc::new(RwLock::new(HashSet::new())),
            tables_init_completed: Arc::new(RwLock::new(HashSet::new())),
            init_rows_total: Arc::new(AtomicU64::new(0)),
            streaming_entered: Arc::new(AtomicBool::new(false)),
            streaming_batches: Arc::new(AtomicU64::new(0)),
            streaming_events: Arc::new(AtomicU64::new(0)),
            streaming_start_time: Arc::new(RwLock::new(None)),
        }
    }
}

#[derive(Clone)]
struct KafkaTarget {
    name: String,
    producer: FutureProducer,
    topic_prefix: String,
    /// `None` = forward all tables; `Some(set)` = only matching tables.
    table_filter: Option<HashSet<String>>,
}

impl KafkaTarget {
    fn accepts_table(&self, table_full_name: &str) -> bool {
        self.table_filter
            .as_ref()
            .map_or(true, |f| f.contains(table_full_name))
    }
}

/// Kafka CDC destination — converts PG replication events to JSON messages
/// and produces them to Kafka topics named `{prefix}.{schema}.{table}`.
/// Supports fan-out to multiple Kafka clusters via [`add_target`].
#[derive(Clone)]
pub struct KafkaDestination {
    targets: Vec<KafkaTarget>,
    store: MemoryStore,
    /// Fallback schema cache populated from RelationEvent
    schemas: Arc<RwLock<HashMap<TableId, Arc<TableSchema>>>>,
    send_timeout: Duration,
    phase: PhaseTracker,
    #[cfg(feature = "metric")]
    metrics: Option<CdcMetrics>,
}

impl KafkaDestination {
    /// Create an empty destination with no targets.
    /// Use [`add_target`] to register one or more independent Kafka targets.
    pub fn new(store: MemoryStore) -> Self {
        Self {
            targets: Vec::new(),
            store,
            schemas: Arc::new(RwLock::new(HashMap::new())),
            send_timeout: Duration::from_secs(30),
            phase: PhaseTracker::new(),
            #[cfg(feature = "metric")]
            metrics: None,
        }
    }

    /// Register a Kafka target. All targets are independent peers.
    /// `table_filter`: optional whitelist of `"schema.table"` names.
    /// `None` or empty vec → forward all tables.
    pub fn add_target(
        &mut self,
        name: &str,
        brokers: &str,
        topic_prefix: String,
        auth: &KafkaAuth,
        table_filter: Option<Vec<String>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let producer = Self::create_producer(brokers, auth)?;
        self.targets.push(KafkaTarget {
            name: name.to_string(),
            producer,
            topic_prefix,
            table_filter: into_filter(table_filter),
        });
        Ok(())
    }

    pub fn target_names(&self) -> Vec<&str> {
        self.targets.iter().map(|t| t.name.as_str()).collect()
    }

    fn create_producer(
        brokers: &str,
        auth: &KafkaAuth,
    ) -> Result<FutureProducer, Box<dyn std::error::Error>> {
        let mut cfg = ClientConfig::new();
        cfg.set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "30000")
            .set("queue.buffering.max.messages", "100000")
            .set("queue.buffering.max.ms", "5")
            .set("batch.num.messages", "10000")
            .set("compression.type", "lz4");

        let protocol = auth.security_protocol.to_uppercase().replace('-', "_");
        cfg.set("security.protocol", &protocol);

        if protocol.contains("SASL") {
            let mechanism = auth
                .sasl_mechanism
                .as_deref()
                .unwrap_or("PLAIN")
                .to_uppercase();
            cfg.set("sasl.mechanism", &mechanism);
            if let Some(u) = &auth.sasl_username {
                cfg.set("sasl.username", u);
            }
            if let Some(p) = &auth.sasl_password {
                cfg.set("sasl.password", p);
            }
        }

        if protocol.contains("SSL") {
            if let Some(ca) = &auth.ssl_ca_location {
                cfg.set("ssl.ca.location", ca);
            }
            if let Some(cert) = &auth.ssl_certificate_location {
                cfg.set("ssl.certificate.location", cert);
            }
            if let Some(key) = &auth.ssl_key_location {
                cfg.set("ssl.key.location", key);
            }
        }

        Ok(cfg.create()?)
    }

    #[cfg(feature = "metric")]
    pub fn with_metrics(mut self, metrics: CdcMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }

    async fn get_schema(&self, table_id: &TableId) -> Option<Arc<TableSchema>> {
        if let Ok(Some(schema)) = self.store.get_table_schema(table_id).await {
            return Some(schema);
        }
        self.schemas.read().await.get(table_id).cloned()
    }

    fn topic_for_target(target: &KafkaTarget, schema: &TableSchema) -> String {
        format!(
            "{}.{}.{}",
            target.topic_prefix, schema.name.schema, schema.name.name
        )
    }

    fn row_to_json(row: &TableRow, schema: &TableSchema) -> Value {
        let mut map = serde_json::Map::new();
        for (cell, col) in row.values().iter().zip(schema.column_schemas.iter()) {
            map.insert(col.name.clone(), Self::cell_to_json(cell));
        }
        Value::Object(map)
    }

    fn cell_to_json(cell: &Cell) -> Value {
        match cell {
            Cell::Null => Value::Null,
            Cell::Bool(v) => json!(v),
            Cell::String(v) => json!(v),
            Cell::I16(v) => json!(v),
            Cell::I32(v) => json!(v),
            Cell::U32(v) => json!(v),
            Cell::I64(v) => json!(v),
            Cell::F32(v) => json!(v),
            Cell::F64(v) => json!(v),
            Cell::Numeric(n) => match n {
                PgNumeric::NaN => json!("NaN"),
                PgNumeric::PositiveInfinity => json!("Infinity"),
                PgNumeric::NegativeInfinity => json!("-Infinity"),
                _ => json!(format!("{n:?}")),
            },
            Cell::Date(v) => json!(v.to_string()),
            Cell::Time(v) => json!(v.to_string()),
            Cell::Timestamp(v) => json!(v.to_string()),
            Cell::TimestampTz(v) => json!(v.to_rfc3339()),
            Cell::Uuid(v) => json!(v.to_string()),
            Cell::Json(v) => v.clone(),
            Cell::Bytes(v) => json!(format!("0x{}", hex_encode(v))),
            Cell::Array(_) => json!("<array>"),
        }
    }

    /// Build a message key from primary key columns (for partition affinity).
    fn extract_key(row: &TableRow, schema: &TableSchema) -> String {
        let pk_values: Vec<Value> = schema
            .column_schemas
            .iter()
            .zip(row.values().iter())
            .filter(|(col, _)| col.primary)
            .map(|(_, cell)| Self::cell_to_json(cell))
            .collect();

        if pk_values.len() == 1 {
            pk_values[0].to_string()
        } else {
            serde_json::to_string(&pk_values).unwrap_or_default()
        }
    }

    /// Produce the payload to every target whose table filter matches.
    /// Targets that don't subscribe to this table are silently skipped.
    async fn produce(
        &self,
        schema: &TableSchema,
        key: &str,
        payload: &[u8],
    ) -> EtlResult<()> {
        let table_full_name = format!("{}.{}", schema.name.schema, schema.name.name);
        let targets: Vec<_> = self
            .targets
            .iter()
            .filter(|t| t.accepts_table(&table_full_name))
            .collect();

        match targets.len() {
            0 => Ok(()),
            1 => self.produce_one(targets[0], schema, key, payload).await,
            _ => {
                let futs = targets
                    .iter()
                    .map(|t| self.produce_one(t, schema, key, payload));
                futures::future::try_join_all(futs).await?;
                Ok(())
            }
        }
    }

    async fn produce_one(
        &self,
        target: &KafkaTarget,
        schema: &TableSchema,
        key: &str,
        payload: &[u8],
    ) -> EtlResult<()> {
        let topic = Self::topic_for_target(target, schema);
        let record = FutureRecord::to(&topic).key(key).payload(payload);
        debug!(
            target = %target.name, topic, key,
            payload_len = payload.len(), "producing message"
        );

        #[cfg(feature = "metric")]
        let t0 = std::time::Instant::now();

        match target.producer.send(record, self.send_timeout).await {
            Ok((partition, offset)) => {
                debug!(
                    target = %target.name, topic, partition, offset,
                    "produce OK"
                );
                #[cfg(feature = "metric")]
                if let Some(m) = &self.metrics {
                    let attrs = [KeyValue::new("target", target.name.clone())];
                    m.produce_duration_ms
                        .record(t0.elapsed().as_secs_f64() * 1000.0, &attrs);
                    m.produce_bytes.add(payload.len() as u64, &attrs);
                }
                Ok(())
            }
            Err((err, _)) => {
                error!(
                    target = %target.name, topic, %err,
                    "produce FAILED"
                );
                #[cfg(feature = "metric")]
                if let Some(m) = &self.metrics {
                    m.produce_errors.add(
                        1,
                        &[KeyValue::new("target", target.name.clone())],
                    );
                }
                Err(EtlError::from((
                    ErrorKind::DestinationError,
                    "Kafka produce failed",
                    format!("[{}] {}", target.name, err),
                )))
            }
        }
    }
}

impl Destination for KafkaDestination {
    fn name() -> &'static str {
        "kafka"
    }

    async fn truncate_table(&self, table_id: TableId) -> EtlResult<()> {
        let mut started = self.phase.tables_init_started.write().await;
        started.insert(table_id);
        let count = started.len();
        info!(
            %table_id,
            phase = "init_sync",
            tables_started = count,
            "table init sync started (truncate is no-op for Kafka)"
        );
        Ok(())
    }

    async fn write_table_rows(
        &self,
        table_id: TableId,
        table_rows: Vec<TableRow>,
    ) -> EtlResult<()> {
        let batch_rows = table_rows.len();

        if batch_rows > 0 {
            let total = self.phase.init_rows_total.fetch_add(batch_rows as u64, Ordering::Relaxed) + batch_rows as u64;
            info!(
                %table_id,
                phase = "init_sync",
                batch_rows,
                total_rows_skipped = total,
                "skipped (incremental-only mode, no snapshot produce)"
            );

            #[cfg(feature = "metric")]
            if let Some(m) = &self.metrics {
                m.init_sync_rows.add(batch_rows as u64, &[]);
            }
        }

        let mut completed = self.phase.tables_init_completed.write().await;
        completed.insert(table_id);
        let started = self.phase.tables_init_started.read().await;
        let done = completed.len();
        let total_tables = started.len();

        info!(
            %table_id,
            phase = "init_sync",
            tables_completed = done,
            tables_total = total_tables,
            "table init sync completed ({done}/{total_tables})"
        );

        Ok(())
    }

    async fn write_events(&self, events: Vec<Event>) -> EtlResult<()> {
        // Log once when entering the streaming phase
        if !self.phase.streaming_entered.swap(true, Ordering::Relaxed) {
            let init_tables = self.phase.tables_init_completed.read().await.len();
            let init_rows = self.phase.init_rows_total.load(Ordering::Relaxed);
            *self.phase.streaming_start_time.write().await = Some(Instant::now());
            info!(
                phase = "streaming",
                init_tables_synced = init_tables,
                init_rows_total = init_rows,
                kafka_targets = ?self.target_names(),
                "entered streaming phase — now processing WAL events in real-time"
            );
        }

        struct Msg {
            schema: Arc<TableSchema>,
            key: String,
            payload: Vec<u8>,
        }

        // Phase 1: schema lookup + JSON serialization (CPU-bound, sequential)
        let mut messages: Vec<Msg> = Vec::with_capacity(events.len());

        for event in events {
            match event {
                Event::Relation(rel) => {
                    let tid = rel.table_schema.id;
                    let name = format!("{}", rel.table_schema.name);
                    self.schemas
                        .write()
                        .await
                        .insert(tid, Arc::new(rel.table_schema));
                    info!(%tid, name, "cached table schema");
                }

                Event::Insert(evt) => {
                    if let Some(schema) = self.get_schema(&evt.table_id).await {
                        let key = Self::extract_key(&evt.table_row, &schema);
                        let envelope = json!({
                            "op": "insert",
                            "table": format!("{}", schema.name),
                            "after": Self::row_to_json(&evt.table_row, &schema),
                        });
                        let payload =
                            serde_json::to_vec(&envelope).map_err(EtlError::from)?;
                        messages.push(Msg { schema, key, payload });
                        #[cfg(feature = "metric")]
                        if let Some(m) = &self.metrics {
                            m.events_total.add(1, &[KeyValue::new("op", "insert")]);
                        }
                    } else {
                        warn!(table_id = %evt.table_id, "insert event skipped: schema unknown");
                    }
                }

                Event::Update(evt) => {
                    if let Some(schema) = self.get_schema(&evt.table_id).await {
                        let key = Self::extract_key(&evt.table_row, &schema);
                        let before = evt
                            .old_table_row
                            .as_ref()
                            .map(|(_, r)| Self::row_to_json(r, &schema));
                        let envelope = json!({
                            "op": "update",
                            "table": format!("{}", schema.name),
                            "before": before,
                            "after": Self::row_to_json(&evt.table_row, &schema),
                        });
                        let payload =
                            serde_json::to_vec(&envelope).map_err(EtlError::from)?;
                        messages.push(Msg { schema, key, payload });
                        #[cfg(feature = "metric")]
                        if let Some(m) = &self.metrics {
                            m.events_total.add(1, &[KeyValue::new("op", "update")]);
                        }
                    } else {
                        warn!(table_id = %evt.table_id, "update event skipped: schema unknown");
                    }
                }

                Event::Delete(evt) => {
                    if let Some(schema) = self.get_schema(&evt.table_id).await {
                        let before = evt
                            .old_table_row
                            .as_ref()
                            .map(|(_, r)| Self::row_to_json(r, &schema));
                        let key = evt
                            .old_table_row
                            .as_ref()
                            .map(|(_, r)| Self::extract_key(r, &schema))
                            .unwrap_or_default();
                        let envelope = json!({
                            "op": "delete",
                            "table": format!("{}", schema.name),
                            "before": before,
                        });
                        let payload =
                            serde_json::to_vec(&envelope).map_err(EtlError::from)?;
                        messages.push(Msg { schema, key, payload });
                        #[cfg(feature = "metric")]
                        if let Some(m) = &self.metrics {
                            m.events_total.add(1, &[KeyValue::new("op", "delete")]);
                        }
                    } else {
                        warn!(table_id = %evt.table_id, "delete event skipped: schema unknown");
                    }
                }

                Event::Begin(_) | Event::Commit(_) | Event::Truncate(_) | Event::Unsupported => {}
            }
        }

        // Phase 2: produce all messages concurrently (I/O-bound)
        // librdkafka batches the underlying network requests automatically.
        if !messages.is_empty() {
            let count = messages.len();
            debug!(count, "batch producing");
            let futs: Vec<_> = messages
                .iter()
                .map(|m| self.produce(&m.schema, &m.key, &m.payload))
                .collect();
            futures::future::try_join_all(futs).await?;

            let batch_num = self.phase.streaming_batches.fetch_add(1, Ordering::Relaxed) + 1;
            let total_events = self.phase.streaming_events.fetch_add(count as u64, Ordering::Relaxed) + count as u64;

            #[cfg(feature = "metric")]
            if let Some(m) = &self.metrics {
                m.streaming_events.add(count as u64, &[]);
                m.streaming_batches.add(1, &[]);
                m.produce_batch_size.record(count as f64, &[]);
            }

            // Periodic summary every 100 batches
            if batch_num % 100 == 0 {
                let elapsed = self.phase.streaming_start_time.read().await
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(0);
                let eps = if elapsed > 0 { total_events / elapsed } else { 0 };
                info!(
                    phase = "streaming",
                    batches = batch_num,
                    events_total = total_events,
                    elapsed_secs = elapsed,
                    events_per_sec = eps,
                    "streaming progress"
                );
            }
        }

        Ok(())
    }
}

/// Convert an optional table list into a `HashSet` filter.
/// Empty or `None` → `None` (no filtering, forward all tables).
fn into_filter(tables: Option<Vec<String>>) -> Option<HashSet<String>> {
    tables
        .filter(|v| !v.is_empty())
        .map(|v| v.into_iter().collect())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests;
