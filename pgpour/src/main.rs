#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod config;
mod kafka_destination;
#[cfg(feature = "metric")]
mod metrics;
mod ssh_tunnel;

use clap::Parser;
use etl::config::{
    BatchConfig, InvalidatedSlotBehavior, MemoryBackpressureConfig, PgConnectionConfig,
    PipelineConfig, TableSyncCopyConfig, TcpKeepaliveConfig, TlsConfig,
};
use etl::pipeline::Pipeline;
use etl::store::both::memory::MemoryStore;
use kafka_destination::{KafkaAuth, KafkaDestination};
use tokio::signal;
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
    // YAML config → env vars (lowest priority; before clap so env= attrs resolve)
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

    let (pg_host, pg_port) = if let Some(ref ssh_host) = args.pg_ssh_host {
        let ssh_username = args
            .pg_ssh_username
            .clone()
            .ok_or("--pg-ssh-username is required when SSH tunnel is enabled")?;
        let tunnel = ssh_tunnel::SshTunnel::start(ssh_tunnel::SshTunnelConfig {
            host: ssh_host.clone(),
            port: args.pg_ssh_port,
            username: ssh_username,
            password: args.pg_ssh_password.clone(),
            private_key_path: args.pg_ssh_private_key_path.clone(),
            private_key_passphrase: args.pg_ssh_private_key_passphrase.clone(),
            remote_host: args.pg_host.clone(),
            remote_port: args.pg_port,
        })
        .await?;
        ("127.0.0.1".to_string(), tunnel.local_port)
    } else {
        (args.pg_host.clone(), args.pg_port)
    };

    let pg = PgConnectionConfig {
        host: pg_host,
        port: pg_port,
        name: args.pg_database.clone(),
        username: args.pg_username.clone(),
        password: args.pg_password.map(Into::into),
        tls: TlsConfig {
            enabled: false,
            trusted_root_certs: String::new(),
        },
        keepalive: TcpKeepaliveConfig::default(),
    };

    let store = MemoryStore::new();

    #[cfg(feature = "metric")]
    let cdc_metrics = if let Some(ref ep) = args.otlp_endpoint {
        let m = metrics::CdcMetrics::init(ep)?;
        info!(endpoint = ep, "OTLP metrics exporter initialized");
        Some(m)
    } else {
        None
    };

    let kafka_auth = KafkaAuth {
        security_protocol: args.kafka_security_protocol.clone(),
        sasl_mechanism: args.kafka_sasl_mechanism.clone(),
        sasl_username: args.kafka_sasl_username.clone(),
        sasl_password: args.kafka_sasl_password.clone(),
        ssl_ca_location: args.kafka_ssl_ca_location.clone(),
        ssl_certificate_location: args.kafka_ssl_certificate_location.clone(),
        ssl_key_location: args.kafka_ssl_key_location.clone(),
    };

    let mut destination = KafkaDestination::new(
        &args.kafka_brokers,
        args.kafka_topic_prefix.clone(),
        store.clone(),
        &kafka_auth,
    )?;

    if let Some((_, ref file_cfg)) = preloaded {
        // Apply table filter to primary target (YAML-only)
        if file_cfg.kafka.tables.is_some() {
            destination.set_primary_table_filter(file_cfg.kafka.tables.clone());
            info!(tables = ?file_cfg.kafka.tables, "primary target table filter set");
        }

        for (i, extra) in file_cfg.kafka_destinations.iter().enumerate() {
            let name = extra
                .name
                .clone()
                .unwrap_or_else(|| format!("destination-{}", i + 1));
            let brokers = extra
                .brokers
                .as_deref()
                .unwrap_or(&args.kafka_brokers);
            let topic_prefix = extra
                .topic_prefix
                .clone()
                .unwrap_or_else(|| args.kafka_topic_prefix.clone());
            let auth = KafkaAuth {
                security_protocol: extra
                    .security_protocol
                    .clone()
                    .unwrap_or_else(|| args.kafka_security_protocol.clone()),
                sasl_mechanism: extra.sasl_mechanism.clone().or(args.kafka_sasl_mechanism.clone()),
                sasl_username: extra.sasl_username.clone().or(args.kafka_sasl_username.clone()),
                sasl_password: extra.sasl_password.clone().or(args.kafka_sasl_password.clone()),
                ssl_ca_location: extra.ssl_ca_location.clone().or(args.kafka_ssl_ca_location.clone()),
                ssl_certificate_location: extra.ssl_certificate_location.clone().or(args.kafka_ssl_certificate_location.clone()),
                ssl_key_location: extra.ssl_key_location.clone().or(args.kafka_ssl_key_location.clone()),
            };
            destination.add_target(&name, brokers, topic_prefix, &auth, extra.tables.clone())?;
            info!(target = name, brokers, tables = ?extra.tables, "added extra Kafka destination");
        }
    }

    #[cfg(feature = "metric")]
    let destination = match cdc_metrics {
        Some(ref m) => destination.with_metrics(m.clone()),
        None => destination,
    };

    let config = PipelineConfig {
        id: 1,
        publication_name: args.publication.clone(),
        pg_connection: pg,
        batch: BatchConfig {
            max_fill_ms: args.batch_max_fill_ms,
            memory_budget_ratio: BatchConfig::DEFAULT_MEMORY_BUDGET_RATIO,
        },
        table_error_retry_delay_ms: 10_000,
        table_error_retry_max_attempts: 5,
        max_table_sync_workers: args.max_table_sync_workers,
        max_copy_connections_per_table: PipelineConfig::DEFAULT_MAX_COPY_CONNECTIONS_PER_TABLE,
        memory_refresh_interval_ms: 100,
        memory_backpressure: Some(MemoryBackpressureConfig::default()),
        table_sync_copy: TableSyncCopyConfig::default(),
        invalidated_slot_behavior: InvalidatedSlotBehavior::default(),
    };

    info!(
        pg_host = args.pg_host,
        pg_database = args.pg_database,
        publication = args.publication,
        kafka_targets = ?destination.target_names(),
        "starting CDC pipeline"
    );

    let mut pipeline = Pipeline::new(config, store, destination);
    pipeline.start().await?;

    info!("pipeline running — Ctrl+C to stop");

    let shutdown = async {
        match signal::ctrl_c().await {
            Ok(()) => info!("ctrl+c received, shutting down"),
            Err(e) => {
                error!(%e, "failed to install ctrl+c handler, graceful shutdown disabled");
                std::future::pending::<()>().await;
            }
        }
    };

    tokio::select! {
        result = pipeline.wait() => {
            result?;
        }
        _ = shutdown => {}
    }

    #[cfg(feature = "metric")]
    if let Some(ref m) = cdc_metrics {
        m.shutdown();
    }

    info!("pipeline stopped");
    Ok(())
}
