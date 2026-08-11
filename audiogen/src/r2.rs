//! R2 / S3 upload — push generated audio to Cloudflare R2 under the per-game namespace.
//!
//! Default pattern (matches user-specified rustyme-like flow):
//!   gamesci-<game>/<format>/<name>.<ext>      e.g. gamesci-classroom/mp3/bgm_lobby.mp3
//!
//! R2 is S3-compatible; we use `rusty-s3` (pure Rust, Sans-IO) + `reqwest` for HTTPS PUT.
//! No AWS SDK, no heavy aws-sdk-* dep tree.
//!
//! Credentials (auto-detected in order):
//!   1. Env: S3_BUCKET, S3_ACCESS_KEY, S3_SECRET_KEY, S3_ENDPOINT, S3_REGION
//!   2. `.s3` file (KEY=VALUE lines; R2 producer convention)
//!   3. `~/.config/r2/credentials` (AWS-like format)

use anyhow::{Context, Result};
use rusty_s3::{Bucket, Credentials, S3Action};
use std::path::{Path, PathBuf};
use std::time::Duration;
use url::Url;

/// Per-file upload result.
#[derive(Debug, Clone)]
pub struct Uploaded {
    pub key: String,
    pub bytes: u64,
    pub url: String,
    pub etag: Option<String>,
}

/// R2 client + config.
pub struct R2Client {
    bucket: Bucket,
    credentials: Credentials,
    public_base: Url,
}

impl R2Client {
    /// Load credentials from `.s3` file or env. Returns `(bucket, key, secret, endpoint, region)`.
    pub fn load_env_or_file(file: Option<&Path>) -> Result<(String, String, String, String, String)> {
        // Try env first.
        if let (Some(b), Some(k), Some(s), Some(e)) = (
            std::env::var("S3_BUCKET").ok(),
            std::env::var("S3_ACCESS_KEY").ok(),
            std::env::var("S3_SECRET_KEY").ok(),
            std::env::var("S3_ENDPOINT").ok(),
        ) {
            let region = std::env::var("S3_REGION").unwrap_or_else(|_| "auto".to_string());
            return Ok((b, k, s, e, region));
        }
        // Try .s3 file.
        let candidates: Vec<PathBuf> = match file {
            Some(p) => vec![p.to_path_buf()],
            None => vec![
                PathBuf::from(".s3"),
                PathBuf::from("scripts/.s3"),
                PathBuf::from("../.s3"),
                dirs_home().map(|h| h.join(".config/r2/credentials")).unwrap_or_default(),
            ],
        };
        for c in candidates {
            if c.is_file() {
                let content = std::fs::read_to_string(&c)
                    .with_context(|| format!("read {}", c.display()))?;
                let parsed = parse_s3_env(&content);
                if let (Some(b), Some(k), Some(s), Some(e)) = (
                    parsed.get("S3_BUCKET").cloned(),
                    parsed.get("S3_ACCESS_KEY").cloned(),
                    parsed.get("S3_SECRET_KEY").cloned(),
                    parsed.get("S3_ENDPOINT").cloned(),
                ) {
                    let region = parsed.get("S3_REGION").cloned().unwrap_or_else(|| "auto".into());
                    return Ok((b, k, s, e, region));
                }
            }
        }
        anyhow::bail!("no S3/R2 credentials found; checked env vars and .s3 file candidates")
    }

    /// Build client. `public_base` is the user-facing URL prefix (e.g.
    /// `https://r2.gamesci-lite.com/gamesci-classroom/...`) — used for returned URLs only.
    pub fn new(
        bucket_name: &str,
        region: &str,
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
        public_base: &str,
    ) -> Result<Self> {
        let endpoint_url = Url::parse(endpoint).context("parse S3_ENDPOINT")?;
        let path_style = rusty_s3::UrlStyle::VirtualHost;
        let bucket = Bucket::new(endpoint_url, path_style, bucket_name.to_string(), region.to_string())
            .context("create rusty-s3 Bucket")?;
        let credentials = Credentials::new(access_key, secret_key);
        let public_base = Url::parse(public_base).context("parse public_base")?;
        Ok(Self { bucket, credentials, public_base })
    }

    /// Upload one file. `key` is the full R2 object key (e.g. "gamesci-classroom/mp3/bgm_lobby.mp3").
    pub async fn upload_file(
        &self,
        local: &Path,
        key: &str,
        content_type: &str,
    ) -> Result<Uploaded> {
        let bytes = std::fs::read(local)
            .with_context(|| format!("read {}", local.display()))?;
        let size = bytes.len() as u64;

        // Build signed PUT request.
        let action = self.bucket.put_object(Some(&self.credentials), key);
        let presigned = action.sign(Duration::from_secs(60 * 10)); // 10 min URL

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("build reqwest client")?;

        let resp = client
            .put(presigned)
            .header("Content-Type", content_type)
            .header("Content-Length", size)
            .body(bytes)
            .send()
            .await
            .context("PUT to R2")?;

        let status = resp.status();
        let etag = resp.headers().get("ETag").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("R2 PUT failed: HTTP {status} body={body}");
        }

        // Compute public URL from base + key.
        let url = self.public_base.join(key)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| format!("{}/{}", self.public_base.as_str().trim_end_matches('/'), key));

        Ok(Uploaded { key: key.to_string(), bytes: size, url, etag })
    }
}

/// Build the canonical R2 key for a generated audio file.
pub fn default_key(game: &str, fmt: &str, name: &str) -> String {
    format!("gamesci-{game}/{fmt}/{name}.{fmt}")
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn parse_s3_env(content: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some((k, v)) = line.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_s3_env() {
        let s = "# comment\nS3_BUCKET=foo\nS3_ACCESS_KEY=bar\nS3_SECRET_KEY=baz\nS3_ENDPOINT=https://x.r2.cloudflarestorage.com\nS3_REGION=auto\n";
        let p = parse_s3_env(s);
        assert_eq!(p.get("S3_BUCKET").unwrap(), "foo");
        assert_eq!(p.get("S3_REGION").unwrap(), "auto");
    }

    #[test]
    fn default_key_format() {
        assert_eq!(default_key("classroom", "mp3", "bgm_lobby"), "gamesci-classroom/mp3/bgm_lobby.mp3");
        assert_eq!(default_key("racing", "ogg", "sfx_engine"), "gamesci-racing/ogg/sfx_engine.ogg");
    }
}