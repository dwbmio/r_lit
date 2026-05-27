//! `ObjectStorageSink` — upload to any S3-compatible endpoint.
//!
//! The same implementation talks to:
//!   * AWS S3              — endpoint `https://s3.{region}.amazonaws.com`
//!   * Aliyun OSS          — endpoint `https://oss-{region}.aliyuncs.com`
//!   * Cloudflare R2       — endpoint `https://{account_id}.r2.cloudflarestorage.com`
//!   * GCP via S3-compat   — endpoint `https://storage.googleapis.com`
//!   * MinIO / SeaweedFS   — endpoint of your local cluster
//!
//! Configuration is **entirely env-driven**:
//!
//! | env var                         | required | example |
//! |---------------------------------|----------|---------|
//! | `GAMEREEL_S3_ENDPOINT`          | optional, defaults to AWS | `https://oss-cn-hangzhou.aliyuncs.com` |
//! | `GAMEREEL_S3_REGION`            | yes      | `cn-hangzhou` / `us-east-1` |
//! | `GAMEREEL_S3_BUCKET`            | yes      | `gamereel-replays` |
//! | `GAMEREEL_S3_ACCESS_KEY_ID`     | yes      | LTAI… / AKIA… |
//! | `GAMEREEL_S3_SECRET_ACCESS_KEY` | yes      | … |
//! | `GAMEREEL_S3_PREFIX`            | optional | `prod/match3/` |
//! | `GAMEREEL_S3_PUBLIC_URL_BASE`   | optional | for CDN-mapped URLs, e.g. `https://cdn.example.com/` |
//! | `GAMEREEL_S3_PATH_STYLE`        | optional, set `1` for MinIO / non-AWS | |
//!
//! `DeliveryReceipt::location` is the *public* URL: either
//! `{PUBLIC_URL_BASE}{key}` if set, or the S3-style virtual-hosted URL.

use crate::{DeliveryReceipt, OutputSink, SinkError};
use async_trait::async_trait;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use gamereel_farm::RenderResult;
use std::env;

#[derive(Debug, Clone)]
pub struct ObjectStorageConfig {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub prefix: String,
    pub public_url_base: Option<String>,
    pub force_path_style: bool,
}

impl ObjectStorageConfig {
    /// Construct from env vars per the table in the module docs.
    /// Missing required vars → SinkError::Config.
    pub fn from_env() -> Result<Self, SinkError> {
        fn req(name: &str) -> Result<String, SinkError> {
            env::var(name).map_err(|_| SinkError::Config(format!("env var {name} not set")))
        }
        Ok(ObjectStorageConfig {
            endpoint: env::var("GAMEREEL_S3_ENDPOINT").ok(),
            region: req("GAMEREEL_S3_REGION")?,
            bucket: req("GAMEREEL_S3_BUCKET")?,
            access_key_id: req("GAMEREEL_S3_ACCESS_KEY_ID")?,
            secret_access_key: req("GAMEREEL_S3_SECRET_ACCESS_KEY")?,
            prefix: env::var("GAMEREEL_S3_PREFIX").unwrap_or_default(),
            public_url_base: env::var("GAMEREEL_S3_PUBLIC_URL_BASE").ok(),
            force_path_style: env::var("GAMEREEL_S3_PATH_STYLE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        })
    }
}

pub struct ObjectStorageSink {
    cfg: ObjectStorageConfig,
    client: S3Client,
}

impl ObjectStorageSink {
    pub async fn new(cfg: ObjectStorageConfig) -> Result<Self, SinkError> {
        let creds = Credentials::new(
            cfg.access_key_id.clone(),
            cfg.secret_access_key.clone(),
            None, None, "gamereel-output",
        );
        let mut builder = aws_sdk_s3::Config::builder()
            .region(Region::new(cfg.region.clone()))
            .credentials_provider(creds)
            .behavior_version_latest()
            .force_path_style(cfg.force_path_style);
        if let Some(ep) = cfg.endpoint.as_ref() {
            builder = builder.endpoint_url(ep);
        }
        let client = S3Client::from_conf(builder.build());
        log::info!(
            "ObjectStorageSink ready (endpoint={:?} region={} bucket={} path_style={})",
            cfg.endpoint, cfg.region, cfg.bucket, cfg.force_path_style,
        );
        Ok(Self { cfg, client })
    }

    /// Construct from env. Convenience wrapper for the common case.
    pub async fn from_env() -> Result<Self, SinkError> {
        Self::new(ObjectStorageConfig::from_env()?).await
    }

    fn key_for(&self, job_id: &str) -> String {
        let safe = job_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect::<String>();
        format!("{}{}.mp4", self.cfg.prefix, safe)
    }

    fn public_url_for(&self, key: &str) -> String {
        if let Some(base) = self.cfg.public_url_base.as_ref() {
            // Caller-defined CDN URL takes precedence (always preferred
            // for end-user-shared links since the bucket may not be
            // publicly readable).
            let sep = if base.ends_with('/') { "" } else { "/" };
            format!("{base}{sep}{key}")
        } else if let Some(ep) = self.cfg.endpoint.as_ref() {
            // Best effort: virtual-host URL on the configured endpoint.
            // For path-style buckets we'd need bucket in path; the
            // `public_url_base` env var is the recommended path.
            format!("{ep}/{}/{key}", self.cfg.bucket)
        } else {
            format!(
                "https://{}.s3.{}.amazonaws.com/{key}",
                self.cfg.bucket, self.cfg.region
            )
        }
    }
}

#[async_trait]
impl OutputSink for ObjectStorageSink {
    fn name(&self) -> &'static str { "object_storage" }

    async fn deliver(
        &self,
        result: &RenderResult,
        mp4_bytes: &[u8],
    ) -> Result<DeliveryReceipt, SinkError> {
        let key = self.key_for(&result.job_id);
        let body = ByteStream::from(mp4_bytes.to_vec());
        self.client
            .put_object()
            .bucket(&self.cfg.bucket)
            .key(&key)
            .content_type("video/mp4")
            .body(body)
            .send()
            .await
            .map_err(|e| SinkError::Transport(format!("s3 put_object: {e}")))?;
        let url = self.public_url_for(&key);
        Ok(DeliveryReceipt {
            job_id: result.job_id.clone(),
            sink: "object_storage",
            location: url,
            bytes: mp4_bytes.len() as u64,
            extra: serde_json::json!({
                "bucket": self.cfg.bucket,
                "key": key,
                "endpoint": self.cfg.endpoint,
            }),
        })
    }
}
