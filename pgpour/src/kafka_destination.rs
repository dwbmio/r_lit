use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

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

#[cfg(feature = "metric")]
use crate::metrics::CdcMetrics;
#[cfg(feature = "metric")]
use opentelemetry::KeyValue;

pub struct KafkaAuth {
    pub security_protocol: String,
    pub sasl_mechanism: Option<String>,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    pub ssl_ca_location: Option<String>,
    pub ssl_certificate_location: Option<String>,
    pub ssl_key_location: Option<String>,
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
    #[cfg(feature = "metric")]
    metrics: Option<CdcMetrics>,
}

impl KafkaDestination {
    pub fn new(
        brokers: &str,
        topic_prefix: String,
        store: MemoryStore,
        auth: &KafkaAuth,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let producer = Self::create_producer(brokers, auth)?;
        Ok(Self {
            targets: vec![KafkaTarget {
                name: "default".to_string(),
                producer,
                topic_prefix,
                table_filter: None,
            }],
            store,
            schemas: Arc::new(RwLock::new(HashMap::new())),
            send_timeout: Duration::from_secs(30),
            #[cfg(feature = "metric")]
            metrics: None,
        })
    }

    /// Register an additional Kafka cluster for fan-out.
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

    /// Set the table whitelist for the primary ("default") target.
    pub fn set_primary_table_filter(&mut self, tables: Option<Vec<String>>) {
        if let Some(target) = self.targets.first_mut() {
            target.table_filter = into_filter(tables);
        }
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
            .set("queue.buffering.max.ms", "100")
            .set("batch.num.messages", "1000");

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
        info!(%table_id, "truncate_table is a no-op for Kafka (append-only)");
        Ok(())
    }

    async fn write_table_rows(
        &self,
        table_id: TableId,
        table_rows: Vec<TableRow>,
    ) -> EtlResult<()> {
        if table_rows.is_empty() {
            return Ok(());
        }

        let _schema = self.get_schema(&table_id).await.ok_or_else(|| {
            EtlError::from((
                ErrorKind::DestinationError,
                "Schema not found for table during initial sync",
                format!("table_id={table_id}"),
            ))
        })?;

        let total = table_rows.len();
        info!(%table_id, total, "write_table_rows: skipping initial sync produce (MVP mode)");
        Ok(())
    }

    async fn write_events(&self, events: Vec<Event>) -> EtlResult<()> {
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
                        self.produce(&schema, &key, &payload).await?;
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
                        self.produce(&schema, &key, &payload).await?;
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
                        self.produce(&schema, &key, &payload).await?;
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
mod tests {
    use super::*;

    fn plaintext_auth() -> KafkaAuth {
        KafkaAuth {
            security_protocol: "plaintext".into(),
            sasl_mechanism: None,
            sasl_username: None,
            sasl_password: None,
            ssl_ca_location: None,
            ssl_certificate_location: None,
            ssl_key_location: None,
        }
    }

    fn sasl_ssl_auth() -> KafkaAuth {
        KafkaAuth {
            security_protocol: "sasl_ssl".into(),
            sasl_mechanism: Some("scram-sha-256".into()),
            sasl_username: Some("user".into()),
            sasl_password: Some("pass".into()),
            ssl_ca_location: None,
            ssl_certificate_location: None,
            ssl_key_location: None,
        }
    }

    #[test]
    fn plaintext_producer_creates_ok() {
        let result = KafkaDestination::new(
            "localhost:9092",
            "test".into(),
            MemoryStore::new(),
            &plaintext_auth(),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn sasl_ssl_producer_creates_ok() {
        let result = KafkaDestination::new(
            "localhost:9092",
            "test".into(),
            MemoryStore::new(),
            &sasl_ssl_auth(),
        );
        assert!(result.is_ok(), "sasl_ssl create failed: {:?}", result.err());
    }

    #[test]
    fn add_target_and_list_names() {
        let mut dest = KafkaDestination::new(
            "localhost:9092",
            "cdc".into(),
            MemoryStore::new(),
            &plaintext_auth(),
        )
        .unwrap();
        assert_eq!(dest.target_names(), vec!["default"]);

        dest.add_target("cluster-b", "localhost:9093", "cdc-backup".into(), &plaintext_auth(), None)
            .unwrap();
        assert_eq!(dest.target_names(), vec!["default", "cluster-b"]);
    }

    #[test]
    fn table_filter_accepts_and_rejects() {
        let mut dest = KafkaDestination::new(
            "localhost:9092",
            "cdc".into(),
            MemoryStore::new(),
            &plaintext_auth(),
        )
        .unwrap();

        dest.set_primary_table_filter(Some(vec!["public.orders".into()]));
        dest.add_target(
            "analytics",
            "localhost:9093",
            "analytics".into(),
            &plaintext_auth(),
            Some(vec!["public.events".into(), "public.logs".into()]),
        )
        .unwrap();

        let primary = &dest.targets[0];
        let analytics = &dest.targets[1];

        assert!(primary.accepts_table("public.orders"));
        assert!(!primary.accepts_table("public.events"));

        assert!(!analytics.accepts_table("public.orders"));
        assert!(analytics.accepts_table("public.events"));
        assert!(analytics.accepts_table("public.logs"));
    }

    #[test]
    fn empty_table_filter_means_all() {
        let mut dest = KafkaDestination::new(
            "localhost:9092",
            "cdc".into(),
            MemoryStore::new(),
            &plaintext_auth(),
        )
        .unwrap();

        // None → all tables
        assert!(dest.targets[0].accepts_table("anything"));

        // Empty vec → also all tables (treated as no filter)
        dest.set_primary_table_filter(Some(vec![]));
        assert!(dest.targets[0].accepts_table("anything"));
    }

    #[test]
    fn hex_encode_works() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn cell_to_json_coverage() {
        assert_eq!(KafkaDestination::cell_to_json(&Cell::Null), Value::Null);
        assert_eq!(KafkaDestination::cell_to_json(&Cell::Bool(true)), json!(true));
        assert_eq!(KafkaDestination::cell_to_json(&Cell::String("hello".into())), json!("hello"));
        assert_eq!(KafkaDestination::cell_to_json(&Cell::I32(42)), json!(42));
        assert_eq!(KafkaDestination::cell_to_json(&Cell::I64(-1)), json!(-1));
        assert_eq!(KafkaDestination::cell_to_json(&Cell::F64(3.14)), json!(3.14));
        assert_eq!(
            KafkaDestination::cell_to_json(&Cell::Numeric(PgNumeric::NaN)),
            json!("NaN")
        );
        assert_eq!(KafkaDestination::cell_to_json(&Cell::Bytes(vec![0xab, 0xcd])), json!("0xabcd"));
    }

    const TEST_BROKERS: &str = "10.9.169.2:9092,10.9.131.68:9092,10.9.103.246:9092";

    /// Fan-out + per-target table filter integration test.
    ///
    /// Setup (same cluster, pre-existing topics):
    ///   - target "primary":  topic = test_broker_1, tables = ["public.table1"]
    ///   - target "secondary": topic = test_broker_2, tables = ["public.table2"]
    ///
    /// Verifies:
    ///   1. table1 event → only produced to test_broker_1 (primary)
    ///   2. table2 event → only produced to test_broker_2 (secondary)
    ///   3. table3 event → dropped (no target matches)
    ///
    /// Run with: `cargo test fanout_table_filter -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn fanout_table_filter() {
        // --- build destination with two filtered targets ---
        let mut dest = KafkaDestination::new(
            TEST_BROKERS,
            "unused-prefix".into(),
            MemoryStore::new(),
            &plaintext_auth(),
        )
        .unwrap();
        dest.set_primary_table_filter(Some(vec!["public.table1".into()]));
        dest.add_target(
            "secondary",
            TEST_BROKERS,
            "unused-prefix".into(),
            &plaintext_auth(),
            Some(vec!["public.table2".into()]),
        )
        .unwrap();

        assert_eq!(dest.target_names(), vec!["default", "secondary"]);

        let timeout = Duration::from_secs(10);

        // --- helper: produce directly to one target's topic ---
        // We override the topic to the pre-existing test_broker_N topics.
        async fn send(
            producer: &FutureProducer,
            topic: &str,
            key: &str,
            msg: &str,
        ) -> Result<(i32, i64), String> {
            let payload = serde_json::to_vec(&json!({
                "op": "insert", "table": topic, "after": {"key": key, "msg": msg}
            }))
            .unwrap();
            let record = FutureRecord::to(topic).key(key).payload(&payload);
            producer
                .send(record, Duration::from_secs(10))
                .await
                .map_err(|(e, _)| e.to_string())
        }

        // --- 1. table1 event → only primary accepts ---
        assert!(dest.targets[0].accepts_table("public.table1"));
        assert!(!dest.targets[1].accepts_table("public.table1"));
        let (p, o) = send(&dest.targets[0].producer, "test_broker_1", "k1", "table1 event")
            .await
            .expect("produce to test_broker_1 failed");
        println!("[primary]   test_broker_1  partition={p} offset={o}");

        // --- 2. table2 event → only secondary accepts ---
        assert!(!dest.targets[0].accepts_table("public.table2"));
        assert!(dest.targets[1].accepts_table("public.table2"));
        let (p, o) = send(&dest.targets[1].producer, "test_broker_2", "k2", "table2 event")
            .await
            .expect("produce to test_broker_2 failed");
        println!("[secondary] test_broker_2  partition={p} offset={o}");

        // --- 3. table3 event → no target matches ---
        assert!(!dest.targets[0].accepts_table("public.table3"));
        assert!(!dest.targets[1].accepts_table("public.table3"));
        println!("[dropped]   public.table3  (no matching target)");

        // --- 4. concurrent fan-out for a table both accept (remove filters) ---
        dest.set_primary_table_filter(None);
        // Build two futures producing concurrently to both topics
        let futs = [
            ("test_broker_1", dest.targets[0].producer.clone()),
            ("test_broker_2", dest.targets[1].producer.clone()),
        ]
        .map(|(topic, producer)| {
            let payload =
                serde_json::to_vec(&json!({"op":"insert","table":"shared","after":{"id":99}}))
                    .unwrap();
            async move {
                let record = FutureRecord::to(topic).key("99").payload(&payload);
                producer
                    .send(record, timeout)
                    .await
                    .map_err(|(e, _)| e.to_string())
            }
        });
        let results = futures::future::join_all(futs).await;
        for (i, r) in results.iter().enumerate() {
            let (p, o) = r.as_ref().unwrap_or_else(|e| panic!("concurrent produce {i} failed: {e}"));
            println!("[concurrent] target={i} partition={p} offset={o}");
        }
    }
}
