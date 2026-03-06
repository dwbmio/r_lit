use std::collections::HashMap;
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

/// Kafka CDC destination — converts PG replication events to JSON messages
/// and produces them to Kafka topics named `{prefix}.{schema}.{table}`.
#[derive(Clone)]
pub struct KafkaDestination {
    producer: FutureProducer,
    topic_prefix: String,
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
        let mut cfg = ClientConfig::new();
        cfg.set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "30000")
            .set("queue.buffering.max.messages", "100000")
            .set("queue.buffering.max.ms", "100")
            .set("batch.num.messages", "1000");

        let protocol = auth.security_protocol.to_uppercase().replace('-', "_");
        cfg.set("security.protocol", &protocol);

        let needs_sasl = protocol.contains("SASL");
        if needs_sasl {
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

        let needs_ssl = protocol.contains("SSL");
        if needs_ssl {
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

        let producer: FutureProducer = cfg.create()?;

        Ok(Self {
            producer,
            topic_prefix,
            store,
            schemas: Arc::new(RwLock::new(HashMap::new())),
            send_timeout: Duration::from_secs(30),
            #[cfg(feature = "metric")]
            metrics: None,
        })
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

    fn topic_for_table(&self, schema: &TableSchema) -> String {
        format!(
            "{}.{}.{}",
            self.topic_prefix, schema.name.schema, schema.name.name
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

    async fn produce(&self, topic: &str, key: &str, payload: &[u8]) -> EtlResult<()> {
        let record = FutureRecord::to(topic).key(key).payload(payload);
        debug!(topic, key, payload_len = payload.len(), "producing message");

        #[cfg(feature = "metric")]
        let t0 = std::time::Instant::now();

        match self.producer.send(record, self.send_timeout).await {
            Ok((partition, offset)) => {
                debug!(topic, partition, offset, "produce OK");
                #[cfg(feature = "metric")]
                if let Some(m) = &self.metrics {
                    m.produce_duration_ms
                        .record(t0.elapsed().as_secs_f64() * 1000.0, &[]);
                    m.produce_bytes.add(payload.len() as u64, &[]);
                }
                Ok(())
            }
            Err((err, _)) => {
                error!(topic, %err, "produce FAILED");
                #[cfg(feature = "metric")]
                if let Some(m) = &self.metrics {
                    m.produce_errors.add(1, &[]);
                }
                Err(EtlError::from((
                    ErrorKind::DestinationError,
                    "Kafka produce failed",
                    err.to_string(),
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
                        let topic = self.topic_for_table(&schema);
                        let key = Self::extract_key(&evt.table_row, &schema);
                        let envelope = json!({
                            "op": "insert",
                            "table": format!("{}", schema.name),
                            "after": Self::row_to_json(&evt.table_row, &schema),
                        });
                        let payload =
                            serde_json::to_vec(&envelope).map_err(EtlError::from)?;
                        self.produce(&topic, &key, &payload).await?;
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
                        let topic = self.topic_for_table(&schema);
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
                        self.produce(&topic, &key, &payload).await?;
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
                        let topic = self.topic_for_table(&schema);
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
                        self.produce(&topic, &key, &payload).await?;
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
}
