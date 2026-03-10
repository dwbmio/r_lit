pub mod config;
pub mod kafka_destination;
mod ssh_tunnel;
#[cfg(feature = "metric")]
mod metrics;

use etl::config::{
    BatchConfig, InvalidatedSlotBehavior, MemoryBackpressureConfig, PgConnectionConfig,
    PipelineConfig, TableSyncCopyConfig, TcpKeepaliveConfig, TlsConfig,
};
use etl::pipeline::Pipeline;
use etl::store::both::memory::MemoryStore;
use kafka_destination::KafkaDestination;
use tracing::{error, info};

// ── Public config types ─────────────────────────────────────────

/// Postgres connection configuration (resolved, all required fields present).
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub publication: String,
    pub ssh: Option<SshConfig>,
}

/// SSH tunnel for reaching the Postgres server via a bastion host.
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: Option<String>,
    pub private_key_path: Option<String>,
    pub private_key_passphrase: Option<String>,
}

/// Runtime configuration for the pgpour CDC pipeline.
pub struct PgPourConfig {
    pub postgres: PgConfig,
    /// Kafka shared defaults + flat list of independent routing targets.
    pub kafka: config::KafkaConfig,
    pub batch_max_fill_ms: u64,
    pub max_table_sync_workers: u16,
    #[cfg(feature = "metric")]
    pub otlp_endpoint: Option<String>,
}

// ── PgPour ──────────────────────────────────────────────────────

/// Core CDC pipeline — connects to Postgres via logical replication
/// and produces change events to one or more Kafka targets.
pub struct PgPour {
    pipeline: Pipeline<MemoryStore, KafkaDestination>,
    #[cfg(feature = "metric")]
    cdc_metrics: Option<metrics::CdcMetrics>,
}

impl PgPour {
    /// Build the pipeline: validate config, set up SSH tunnel if needed,
    /// create Kafka producers and the etl replication pipeline.
    pub async fn new(cfg: PgPourConfig) -> Result<Self, Box<dyn std::error::Error>> {
        config::validate_kafka_targets(&cfg.kafka)
            .map_err(|e| format!("configuration error:\n{e}"))?;

        // SSH tunnel
        let (pg_host, pg_port) = if let Some(ref ssh) = cfg.postgres.ssh {
            let tunnel = ssh_tunnel::SshTunnel::start(ssh_tunnel::SshTunnelConfig {
                host: ssh.host.clone(),
                port: ssh.port,
                username: ssh.username.clone(),
                password: ssh.password.clone(),
                private_key_path: ssh.private_key_path.clone(),
                private_key_passphrase: ssh.private_key_passphrase.clone(),
                remote_host: cfg.postgres.host.clone(),
                remote_port: cfg.postgres.port,
            })
            .await?;
            ("127.0.0.1".to_string(), tunnel.local_port)
        } else {
            (cfg.postgres.host.clone(), cfg.postgres.port)
        };

        let pg = PgConnectionConfig {
            host: pg_host,
            port: pg_port,
            name: cfg.postgres.database.clone(),
            username: cfg.postgres.username.clone(),
            password: cfg.postgres.password.clone().map(Into::into),
            tls: TlsConfig {
                enabled: false,
                trusted_root_certs: String::new(),
            },
            keepalive: TcpKeepaliveConfig::default(),
        };

        let store = MemoryStore::new();

        #[cfg(feature = "metric")]
        let cdc_metrics = if let Some(ref ep) = cfg.otlp_endpoint {
            let m = metrics::CdcMetrics::init(ep)?;
            info!(endpoint = ep, "OTLP metrics exporter initialized");
            Some(m)
        } else {
            None
        };

        let mut destination = KafkaDestination::new(store.clone());

        for target_cfg in &cfg.kafka.targets {
            let resolved = target_cfg.with_defaults(&cfg.kafka);
            let name = resolved.name.as_deref().expect("validated");
            let brokers = resolved.brokers.as_deref().expect("validated: every target must have brokers");
            let topic_prefix = resolved.topic_prefix.clone().expect("validated");
            let auth = resolved.to_auth();
            destination.add_target(name, brokers, topic_prefix.clone(), &auth, resolved.tables.clone())?;
            info!(target = name, brokers, topic_prefix, tables = ?resolved.tables, "added Kafka target");
        }

        #[cfg(feature = "metric")]
        let destination = match cdc_metrics {
            Some(ref m) => destination.with_metrics(m.clone()),
            None => destination,
        };

        let pipeline_config = PipelineConfig {
            id: 1,
            publication_name: cfg.postgres.publication.clone(),
            pg_connection: pg,
            batch: BatchConfig {
                max_fill_ms: cfg.batch_max_fill_ms,
                memory_budget_ratio: BatchConfig::DEFAULT_MEMORY_BUDGET_RATIO,
            },
            table_error_retry_delay_ms: 10_000,
            table_error_retry_max_attempts: 5,
            max_table_sync_workers: cfg.max_table_sync_workers,
            max_copy_connections_per_table: PipelineConfig::DEFAULT_MAX_COPY_CONNECTIONS_PER_TABLE,
            memory_refresh_interval_ms: 100,
            memory_backpressure: Some(MemoryBackpressureConfig::default()),
            table_sync_copy: TableSyncCopyConfig::default(),
            invalidated_slot_behavior: InvalidatedSlotBehavior::default(),
        };

        info!(
            pg_host = cfg.postgres.host,
            pg_database = cfg.postgres.database,
            publication = cfg.postgres.publication,
            kafka_targets = ?destination.target_names(),
            "starting CDC pipeline"
        );

        let pipeline = Pipeline::new(pipeline_config, store, destination);

        Ok(Self {
            pipeline,
            #[cfg(feature = "metric")]
            cdc_metrics,
        })
    }

    /// Start the pipeline and block until shutdown (Ctrl+C) or pipeline error.
    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.pipeline.start().await?;
        info!("pipeline running — Ctrl+C to stop");

        let shutdown = async {
            match tokio::signal::ctrl_c().await {
                Ok(()) => info!("ctrl+c received, shutting down"),
                Err(e) => {
                    error!(%e, "failed to install ctrl+c handler, graceful shutdown disabled");
                    std::future::pending::<()>().await;
                }
            }
        };

        tokio::select! {
            result = self.pipeline.wait() => {
                result?;
            }
            _ = shutdown => {}
        }

        #[cfg(feature = "metric")]
        if let Some(ref m) = self.cdc_metrics {
            m.shutdown();
        }

        info!("pipeline stopped");
        Ok(())
    }
}
