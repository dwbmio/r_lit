use serde::Deserialize;
use std::path::Path;

/// YAML configuration file structure.
/// Values here have the lowest priority: env vars and CLI args override them.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub postgres: Postgres,
    pub kafka: Kafka,
    /// Extra Kafka destinations for fan-out (YAML-only, each receives the same CDC events).
    pub kafka_destinations: Vec<Kafka>,
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

#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default)]
pub struct Kafka {
    /// Human-readable name for this destination (used in logs & metrics).
    pub name: Option<String>,
    pub brokers: Option<String>,
    pub topic_prefix: Option<String>,
    /// Optional table whitelist (e.g. `["public.orders", "public.users"]`).
    /// When set (non-empty), only events for these tables are forwarded to this target.
    /// When absent or empty, all tables in the publication are forwarded.
    pub tables: Option<Vec<String>>,
    /// plaintext | ssl | sasl_plaintext | sasl_ssl
    pub security_protocol: Option<String>,
    /// plain | scram-sha-256 | scram-sha-512
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

impl FileConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }

    /// Inject YAML values into env vars so clap can pick them up.
    /// Existing env vars are NOT overwritten (env > yaml).
    pub fn apply_to_env(&self) {
        fn set(key: &str, val: &str) {
            if std::env::var(key).is_err() {
                // Safety: called before tokio runtime starts; single-threaded at this point.
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
        if let Some(v) = &self.kafka.topic_prefix { set("KAFKA_TOPIC_PREFIX", v); }
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

/// Pre-parse `--config` / `CONFIG_PATH` before clap runs, so YAML
/// values become env vars that clap's `env = "..."` can resolve.
/// Returns `(config_path, FileConfig)` when a config file is present.
pub fn preload() -> Result<Option<(String, FileConfig)>, Box<dyn std::error::Error>> {
    let path = std::env::args()
        .zip(std::env::args().skip(1))
        .find(|(a, _)| a == "--config")
        .map(|(_, v)| v)
        .or_else(|| std::env::var("CONFIG_PATH").ok());

    if let Some(p) = path {
        let cfg = FileConfig::load(&p)?;
        cfg.apply_to_env();
        return Ok(Some((p, cfg)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_YAML: &str = r#"
postgres:
  host: db.example.com
  port: 5432
  database: mydb
  username: admin
  password: secret
  publication: my_pub
  ssh_host: bastion.example.com
  ssh_port: 2222
  ssh_username: tunnel_user
  ssh_password: tunnel_pass
  ssh_private_key_path: /keys/id_rsa
  ssh_private_key_passphrase: keypass

kafka:
  brokers: broker1:9092,broker2:9092
  topic_prefix: events
  security_protocol: sasl_ssl
  sasl_mechanism: scram-sha-256
  sasl_username: kafka_user
  sasl_password: kafka_pass
  ssl_ca_location: /certs/ca.pem
  ssl_certificate_location: /certs/client.pem
  ssl_key_location: /certs/client.key

pipeline:
  batch_max_fill_ms: 3000
  max_table_sync_workers: 8

otel:
  endpoint: http://collector:4317
"#;

    const MINIMAL_YAML: &str = r#"
postgres:
  host: localhost
kafka:
  brokers: localhost:9092
"#;

    #[test]
    fn parse_full_config() {
        let cfg: FileConfig = serde_yaml::from_str(FULL_YAML).unwrap();

        assert_eq!(cfg.postgres.host.as_deref(), Some("db.example.com"));
        assert_eq!(cfg.postgres.port, Some(5432));
        assert_eq!(cfg.postgres.database.as_deref(), Some("mydb"));
        assert_eq!(cfg.postgres.ssh_host.as_deref(), Some("bastion.example.com"));
        assert_eq!(cfg.postgres.ssh_port, Some(2222));
        assert_eq!(cfg.postgres.ssh_username.as_deref(), Some("tunnel_user"));
        assert_eq!(cfg.postgres.ssh_private_key_path.as_deref(), Some("/keys/id_rsa"));

        assert_eq!(cfg.kafka.brokers.as_deref(), Some("broker1:9092,broker2:9092"));
        assert_eq!(cfg.kafka.security_protocol.as_deref(), Some("sasl_ssl"));
        assert_eq!(cfg.kafka.sasl_mechanism.as_deref(), Some("scram-sha-256"));
        assert_eq!(cfg.kafka.sasl_username.as_deref(), Some("kafka_user"));
        assert_eq!(cfg.kafka.ssl_ca_location.as_deref(), Some("/certs/ca.pem"));

        assert_eq!(cfg.pipeline.batch_max_fill_ms, Some(3000));
        assert_eq!(cfg.pipeline.max_table_sync_workers, Some(8));
        assert_eq!(cfg.otel.endpoint.as_deref(), Some("http://collector:4317"));
    }

    #[test]
    fn parse_minimal_config_defaults_to_none() {
        let cfg: FileConfig = serde_yaml::from_str(MINIMAL_YAML).unwrap();

        assert_eq!(cfg.postgres.host.as_deref(), Some("localhost"));
        assert!(cfg.postgres.port.is_none());
        assert!(cfg.postgres.ssh_host.is_none());
        assert!(cfg.kafka.security_protocol.is_none());
        assert!(cfg.kafka.sasl_mechanism.is_none());
        assert!(cfg.pipeline.batch_max_fill_ms.is_none());
        assert!(cfg.otel.endpoint.is_none());
    }

    #[test]
    fn parse_empty_config() {
        let cfg: FileConfig = serde_yaml::from_str("").unwrap();
        assert!(cfg.postgres.host.is_none());
        assert!(cfg.kafka.brokers.is_none());
    }

    #[test]
    fn load_example_config_file() {
        let cfg = FileConfig::load("examples/config.example.yml")
            .expect("examples/config.example.yml should be parseable");
        assert!(cfg.postgres.host.is_some());
        assert!(cfg.kafka.brokers.is_some());
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        assert!(FileConfig::load("/tmp/nonexistent_pgpour_config.yml").is_err());
    }

    const MULTI_DEST_YAML: &str = r#"
kafka:
  brokers: primary:9092
  topic_prefix: cdc
  tables:
    - public.orders

kafka_destinations:
  - name: cluster-b
    brokers: backup:9092
    topic_prefix: cdc-backup
    security_protocol: sasl_ssl
    sasl_mechanism: scram-sha-512
    sasl_username: user_b
    sasl_password: pass_b
  - name: analytics
    brokers: analytics:9092
    topic_prefix: analytics
    tables:
      - public.events
      - public.logs
"#;

    #[test]
    fn parse_kafka_destinations_with_tables() {
        let cfg: FileConfig = serde_yaml::from_str(MULTI_DEST_YAML).unwrap();
        assert_eq!(cfg.kafka.brokers.as_deref(), Some("primary:9092"));
        assert_eq!(
            cfg.kafka.tables.as_deref(),
            Some(["public.orders".to_string()].as_slice())
        );
        assert_eq!(cfg.kafka_destinations.len(), 2);

        let b = &cfg.kafka_destinations[0];
        assert_eq!(b.name.as_deref(), Some("cluster-b"));
        assert_eq!(b.brokers.as_deref(), Some("backup:9092"));
        assert_eq!(b.topic_prefix.as_deref(), Some("cdc-backup"));
        assert_eq!(b.security_protocol.as_deref(), Some("sasl_ssl"));
        assert!(b.tables.is_none());

        let a = &cfg.kafka_destinations[1];
        assert_eq!(a.name.as_deref(), Some("analytics"));
        assert_eq!(a.brokers.as_deref(), Some("analytics:9092"));
        assert_eq!(a.tables.as_ref().unwrap().len(), 2);
        assert!(a.tables.as_ref().unwrap().contains(&"public.events".to_string()));
        assert!(a.tables.as_ref().unwrap().contains(&"public.logs".to_string()));
    }

    #[test]
    fn empty_kafka_destinations_defaults_to_empty_vec() {
        let cfg: FileConfig = serde_yaml::from_str(MINIMAL_YAML).unwrap();
        assert!(cfg.kafka_destinations.is_empty());
    }
}
