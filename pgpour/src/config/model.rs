use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

// ── Data types ──────────────────────────────────────────────────

/// YAML configuration file structure.
/// Priority: CLI args > env vars > YAML > defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub postgres: Postgres,
    pub kafka: KafkaConfig,
    pub pipeline: Pipeline,
    pub otel: Otel,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Postgres {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub publication: Option<String>,
    pub ssh_host: Option<String>,
    pub ssh_port: Option<u16>,
    pub ssh_username: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_private_key_path: Option<String>,
    pub ssh_private_key_passphrase: Option<String>,
}

/// Kafka section: shared connection defaults + flat list of routing targets.
///
/// `brokers` and auth fields serve as defaults that every target inherits
/// unless overridden per-target.  Each target in `targets` is an
/// independent peer — there is no "primary" or parent-child hierarchy.
#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default)]
pub struct KafkaConfig {
    pub brokers: Option<String>,
    /// plaintext | ssl | sasl_plaintext | sasl_ssl
    pub security_protocol: Option<String>,
    pub sasl_mechanism: Option<String>,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    pub ssl_ca_location: Option<String>,
    pub ssl_certificate_location: Option<String>,
    pub ssl_key_location: Option<String>,
    /// Independent routing targets. Each must have `name` and `topic_prefix`.
    pub targets: Vec<KafkaTarget>,
}

/// A single, independent Kafka routing target.
///
/// Connection fields (`brokers`, `security_protocol`, …) are optional;
/// when absent they fall back to the parent [`KafkaConfig`] defaults.
/// Routing fields (`topic_prefix`, `tables`) are always per-target.
#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default)]
pub struct KafkaTarget {
    pub name: Option<String>,
    pub topic_prefix: Option<String>,
    /// Optional table whitelist (e.g. `["public.orders"]`).
    /// `None` / empty → forward all tables in the publication.
    pub tables: Option<Vec<String>>,
    pub brokers: Option<String>,
    pub security_protocol: Option<String>,
    pub sasl_mechanism: Option<String>,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    pub ssl_ca_location: Option<String>,
    pub ssl_certificate_location: Option<String>,
    pub ssl_key_location: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Pipeline {
    pub batch_max_fill_ms: Option<u64>,
    pub max_table_sync_workers: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Otel {
    pub endpoint: Option<String>,
}

/// Kafka producer authentication configuration (resolved, no Option).
pub struct KafkaAuth {
    pub security_protocol: String,
    pub sasl_mechanism: Option<String>,
    pub sasl_username: Option<String>,
    pub sasl_password: Option<String>,
    pub ssl_ca_location: Option<String>,
    pub ssl_certificate_location: Option<String>,
    pub ssl_key_location: Option<String>,
}

// ── FileConfig ──────────────────────────────────────────────────

impl FileConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }

    /// Inject YAML values into env vars so clap `env = "..."` can resolve them.
    /// Existing env vars are NOT overwritten (env > yaml).
    pub fn apply_to_env(&self) {
        fn set(key: &str, val: &str) {
            if std::env::var(key).is_err() {
                unsafe { std::env::set_var(key, val) };
            }
        }

        if let Some(v) = &self.postgres.host { set("PG_HOST", v); }
        if let Some(v) = self.postgres.port { set("PG_PORT", &v.to_string()); }
        if let Some(v) = &self.postgres.database { set("PG_DATABASE", v); }
        if let Some(v) = &self.postgres.username { set("PG_USERNAME", v); }
        if let Some(v) = &self.postgres.password { set("PG_PASSWORD", v); }
        if let Some(v) = &self.postgres.publication { set("PG_PUBLICATION", v); }
        if let Some(v) = &self.postgres.ssh_host { set("PG_SSH_HOST", v); }
        if let Some(v) = self.postgres.ssh_port { set("PG_SSH_PORT", &v.to_string()); }
        if let Some(v) = &self.postgres.ssh_username { set("PG_SSH_USERNAME", v); }
        if let Some(v) = &self.postgres.ssh_password { set("PG_SSH_PASSWORD", v); }
        if let Some(v) = &self.postgres.ssh_private_key_path { set("PG_SSH_PRIVATE_KEY_PATH", v); }
        if let Some(v) = &self.postgres.ssh_private_key_passphrase { set("PG_SSH_PRIVATE_KEY_PASSPHRASE", v); }
        if let Some(v) = &self.kafka.brokers { set("KAFKA_BROKERS", v); }
        if let Some(v) = &self.kafka.security_protocol { set("KAFKA_SECURITY_PROTOCOL", v); }
        if let Some(v) = &self.kafka.sasl_mechanism { set("KAFKA_SASL_MECHANISM", v); }
        if let Some(v) = &self.kafka.sasl_username { set("KAFKA_SASL_USERNAME", v); }
        if let Some(v) = &self.kafka.sasl_password { set("KAFKA_SASL_PASSWORD", v); }
        if let Some(v) = &self.kafka.ssl_ca_location { set("KAFKA_SSL_CA_LOCATION", v); }
        if let Some(v) = &self.kafka.ssl_certificate_location { set("KAFKA_SSL_CERTIFICATE_LOCATION", v); }
        if let Some(v) = &self.kafka.ssl_key_location { set("KAFKA_SSL_KEY_LOCATION", v); }
        if let Some(v) = self.pipeline.batch_max_fill_ms { set("BATCH_MAX_FILL_MS", &v.to_string()); }
        if let Some(v) = self.pipeline.max_table_sync_workers { set("MAX_TABLE_SYNC_WORKERS", &v.to_string()); }
        if let Some(v) = &self.otel.endpoint { set("OTEL_EXPORTER_OTLP_ENDPOINT", v); }
    }
}

// ── KafkaTarget ─────────────────────────────────────────────────

impl KafkaTarget {
    /// Fill missing connection fields from the shared [`KafkaConfig`] defaults.
    /// Routing fields (`name`, `topic_prefix`, `tables`) are never touched.
    pub fn with_defaults(&self, defaults: &KafkaConfig) -> KafkaTarget {
        KafkaTarget {
            name: self.name.clone(),
            topic_prefix: self.topic_prefix.clone(),
            tables: self.tables.clone(),
            brokers: self.brokers.clone().or_else(|| defaults.brokers.clone()),
            security_protocol: self
                .security_protocol
                .clone()
                .or_else(|| defaults.security_protocol.clone()),
            sasl_mechanism: self
                .sasl_mechanism
                .clone()
                .or_else(|| defaults.sasl_mechanism.clone()),
            sasl_username: self
                .sasl_username
                .clone()
                .or_else(|| defaults.sasl_username.clone()),
            sasl_password: self
                .sasl_password
                .clone()
                .or_else(|| defaults.sasl_password.clone()),
            ssl_ca_location: self
                .ssl_ca_location
                .clone()
                .or_else(|| defaults.ssl_ca_location.clone()),
            ssl_certificate_location: self
                .ssl_certificate_location
                .clone()
                .or_else(|| defaults.ssl_certificate_location.clone()),
            ssl_key_location: self
                .ssl_key_location
                .clone()
                .or_else(|| defaults.ssl_key_location.clone()),
        }
    }

    /// Extract authentication configuration.
    /// Defaults to `plaintext` when `security_protocol` is not set.
    pub fn to_auth(&self) -> KafkaAuth {
        KafkaAuth {
            security_protocol: self
                .security_protocol
                .clone()
                .unwrap_or_else(|| "plaintext".into()),
            sasl_mechanism: self.sasl_mechanism.clone(),
            sasl_username: self.sasl_username.clone(),
            sasl_password: self.sasl_password.clone(),
            ssl_ca_location: self.ssl_ca_location.clone(),
            ssl_certificate_location: self.ssl_certificate_location.clone(),
            ssl_key_location: self.ssl_key_location.clone(),
        }
    }
}

// ── Validation ──────────────────────────────────────────────────

/// Validate a [`KafkaConfig`] with all its targets.
///
/// Checks:
/// - At least one target exists
/// - Each target has a non-empty `name` and `topic_prefix`
/// - All `name` values are unique
/// - No two targets share the same `(brokers, topic_prefix)` pair
/// - Table references are qualified (`schema.table`)
pub fn validate_kafka_targets(kafka: &KafkaConfig) -> Result<(), String> {
    let mut errors = Vec::new();

    if kafka.targets.is_empty() {
        errors.push("kafka.targets: at least one target must be configured".to_string());
        return Err(errors.join("\n"));
    }

    let mut seen_names: HashSet<String> = HashSet::new();
    let mut seen_pairs: HashSet<(String, String)> = HashSet::new();

    for (i, target) in kafka.targets.iter().enumerate() {
        let idx = format!("kafka.targets[{i}]");
        let label = target
            .name
            .as_ref()
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| idx.clone());

        match &target.name {
            None => errors.push(format!("{idx}: 'name' is required")),
            Some(n) if n.is_empty() => {
                errors.push(format!("{idx}: 'name' must not be empty"))
            }
            Some(n) => {
                if !seen_names.insert(n.clone()) {
                    errors.push(format!("{label}: duplicate target name"));
                }
            }
        }

        match &target.topic_prefix {
            None => errors.push(format!("{label}: 'topic_prefix' is required")),
            Some(p) if p.is_empty() => {
                errors.push(format!("{label}: 'topic_prefix' must not be empty"))
            }
            _ => {}
        }

        let effective_brokers = target
            .brokers
            .as_deref()
            .or(kafka.brokers.as_deref())
            .unwrap_or("");
        if effective_brokers.is_empty() {
            errors.push(format!(
                "{label}: 'brokers' is required (set it per-target or in kafka.brokers as shared default)"
            ));
        }
        if let Some(prefix) = &target.topic_prefix {
            let key = (normalize_brokers(effective_brokers), prefix.clone());
            if !seen_pairs.insert(key) {
                errors.push(format!(
                    "{label}: duplicate (brokers, topic_prefix) — \
                     another target already writes to '{prefix}' on the same brokers"
                ));
            }
        }

        validate_table_list(&format!("{label}.tables"), &target.tables, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

// ── Helpers ─────────────────────────────────────────────────────

/// Validate that every entry in the table list is a qualified `schema.table` reference.
fn validate_table_list(field: &str, tables: &Option<Vec<String>>, errors: &mut Vec<String>) {
    if let Some(tables) = tables {
        for t in tables {
            if !is_qualified_table_name(t) {
                errors.push(format!(
                    "{field}: invalid table reference '{t}' \
                     (expected 'schema.table', e.g. 'public.orders')"
                ));
            }
        }
    }
}

/// `schema.table` — exactly one dot, both parts non-empty.
fn is_qualified_table_name(name: &str) -> bool {
    match name.split_once('.') {
        Some((schema, table)) => !schema.is_empty() && !table.is_empty() && !table.contains('.'),
        None => false,
    }
}

/// Sort + dedupe broker addresses for reliable equality comparison.
fn normalize_brokers(brokers: &str) -> String {
    let mut addrs: Vec<&str> = brokers.split(',').map(str::trim).collect();
    addrs.sort();
    addrs.dedup();
    addrs.join(",")
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
