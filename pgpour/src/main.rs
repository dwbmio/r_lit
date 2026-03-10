#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use clap::Parser;
use pgpour::config;
use pgpour::{PgConfig, PgPour, PgPourConfig, SshConfig};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Debug, Parser)]
#[command(
    name = "pgpour",
    version,
    about = "pgpour — Postgres CDC → Kafka pipeline (powered by supabase/etl)"
)]
struct Args {
    /// Path to YAML config file (values overridden by env vars and CLI args)
    #[arg(long, env = "CONFIG_PATH")]
    config: Option<String>,

    /// Postgres host
    #[arg(long, env = "PG_HOST")]
    pg_host: String,

    /// Postgres port
    #[arg(long, env = "PG_PORT", default_value = "5432")]
    pg_port: u16,

    /// Postgres database name
    #[arg(long, env = "PG_DATABASE")]
    pg_database: String,

    /// Postgres username (must have REPLICATION privilege)
    #[arg(long, env = "PG_USERNAME")]
    pg_username: String,

    /// Postgres password
    #[arg(long, env = "PG_PASSWORD")]
    pg_password: Option<String>,

    /// Postgres publication name (must exist: CREATE PUBLICATION ... FOR TABLE ...)
    #[arg(long, env = "PG_PUBLICATION", default_value = "cdc_publication")]
    publication: String,

    /// SSH tunnel host (bastion / jump server). Enables SSH tunnel when set.
    #[arg(long, env = "PG_SSH_HOST")]
    pg_ssh_host: Option<String>,

    /// SSH tunnel port
    #[arg(long, env = "PG_SSH_PORT", default_value = "22")]
    pg_ssh_port: u16,

    /// SSH tunnel username
    #[arg(long, env = "PG_SSH_USERNAME")]
    pg_ssh_username: Option<String>,

    /// SSH tunnel password
    #[arg(long, env = "PG_SSH_PASSWORD")]
    pg_ssh_password: Option<String>,

    /// Path to SSH private key file
    #[arg(long, env = "PG_SSH_PRIVATE_KEY_PATH")]
    pg_ssh_private_key_path: Option<String>,

    /// Passphrase for the SSH private key
    #[arg(long, env = "PG_SSH_PRIVATE_KEY_PASSPHRASE")]
    pg_ssh_private_key_passphrase: Option<String>,

    /// Kafka bootstrap servers
    #[arg(long, env = "KAFKA_BROKERS", default_value = "localhost:9092")]
    kafka_brokers: String,

    /// Kafka topic prefix (topics: {prefix}.{schema}.{table})
    #[arg(long, env = "KAFKA_TOPIC_PREFIX", default_value = "cdc")]
    kafka_topic_prefix: String,

    /// Kafka security protocol: plaintext, ssl, sasl_plaintext, sasl_ssl
    #[arg(long, env = "KAFKA_SECURITY_PROTOCOL", default_value = "plaintext")]
    kafka_security_protocol: String,

    /// SASL mechanism: plain, scram-sha-256, scram-sha-512
    #[arg(long, env = "KAFKA_SASL_MECHANISM")]
    kafka_sasl_mechanism: Option<String>,

    /// SASL username
    #[arg(long, env = "KAFKA_SASL_USERNAME")]
    kafka_sasl_username: Option<String>,

    /// SASL password
    #[arg(long, env = "KAFKA_SASL_PASSWORD")]
    kafka_sasl_password: Option<String>,

    /// Path to CA certificate for SSL
    #[arg(long, env = "KAFKA_SSL_CA_LOCATION")]
    kafka_ssl_ca_location: Option<String>,

    /// Path to client certificate for mutual TLS
    #[arg(long, env = "KAFKA_SSL_CERTIFICATE_LOCATION")]
    kafka_ssl_certificate_location: Option<String>,

    /// Path to client private key for mutual TLS
    #[arg(long, env = "KAFKA_SSL_KEY_LOCATION")]
    kafka_ssl_key_location: Option<String>,

    /// Batch max fill duration (ms)
    #[arg(long, env = "BATCH_MAX_FILL_MS", default_value = "5000")]
    batch_max_fill_ms: u64,

    /// Max parallel table sync workers
    #[arg(long, env = "MAX_TABLE_SYNC_WORKERS", default_value = "4")]
    max_table_sync_workers: u16,

    /// OTLP gRPC endpoint for metrics export (e.g. http://localhost:4317).
    /// Only effective when compiled with --features metric.
    #[cfg(feature = "metric")]
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = run().await {
        error!("{e}");
        std::process::exit(1);
    }
    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let preloaded = config::preload()?;

    if std::env::var("RUST_LOG").is_err() {
        unsafe { std::env::set_var("RUST_LOG", "info") };
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pgpour=info,etl=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if let Some((ref p, _)) = preloaded {
        info!(path = p, "config file loaded");
    }

    let args = Args::parse();
    let file_cfg = preloaded.as_ref().map(|(_, c)| c);

    let ssh = if let Some(host) = args.pg_ssh_host {
        let username = args
            .pg_ssh_username
            .ok_or("--pg-ssh-username is required when SSH tunnel is enabled")?;
        Some(SshConfig {
            host,
            port: args.pg_ssh_port,
            username,
            password: args.pg_ssh_password,
            private_key_path: args.pg_ssh_private_key_path,
            private_key_passphrase: args.pg_ssh_private_key_passphrase,
        })
    } else {
        None
    };

    // If YAML defines kafka.targets, use them as-is.
    // Otherwise, create a single default target from CLI --kafka-topic-prefix.
    let targets = match file_cfg {
        Some(fc) if !fc.kafka.targets.is_empty() => fc.kafka.targets.clone(),
        _ => vec![config::KafkaTarget {
            name: Some("default".to_string()),
            topic_prefix: Some(args.kafka_topic_prefix),
            ..Default::default()
        }],
    };

    let cfg = PgPourConfig {
        postgres: PgConfig {
            host: args.pg_host,
            port: args.pg_port,
            database: args.pg_database,
            username: args.pg_username,
            password: args.pg_password,
            publication: args.publication,
            ssh,
        },
        kafka: config::KafkaConfig {
            brokers: Some(args.kafka_brokers),
            security_protocol: Some(args.kafka_security_protocol),
            sasl_mechanism: args.kafka_sasl_mechanism,
            sasl_username: args.kafka_sasl_username,
            sasl_password: args.kafka_sasl_password,
            ssl_ca_location: args.kafka_ssl_ca_location,
            ssl_certificate_location: args.kafka_ssl_certificate_location,
            ssl_key_location: args.kafka_ssl_key_location,
            targets,
        },
        batch_max_fill_ms: args.batch_max_fill_ms,
        max_table_sync_workers: args.max_table_sync_workers,
        #[cfg(feature = "metric")]
        otlp_endpoint: args.otlp_endpoint,
    };

    PgPour::new(cfg).await?.run().await
}
