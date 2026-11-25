use anyhow::{Context, Result, anyhow};
use distill_git::{RepoConfigSpec, wildcard_match};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VlogsConfig {
    #[serde(default = "default_vlogs_enabled")]
    pub enabled: bool,
    #[serde(default = "default_vlogs_endpoint")]
    pub endpoint: String,
    #[serde(default = "default_vlogs_range")]
    pub default_range: String,
    #[serde(default = "default_vlogs_limit")]
    pub default_limit: usize,
    #[serde(default)]
    pub bearer_token: Option<String>,
}

impl Default for VlogsConfig {
    fn default() -> Self {
        Self {
            enabled: default_vlogs_enabled(),
            endpoint: default_vlogs_endpoint(),
            default_range: default_vlogs_range(),
            default_limit: default_vlogs_limit(),
            bearer_token: None,
        }
    }
}

fn default_vlogs_enabled() -> bool {
    true
}
fn default_vlogs_endpoint() -> String {
    "http://vlogs.hs99.vip:9428".into()
}
fn default_vlogs_range() -> String {
    "15m".into()
}
fn default_vlogs_limit() -> usize {
    50
}

/// First-class program management configuration (`[server]` section in `config.toml`).
/// Supports external hot-reloading without restarting `distilld`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramConfig {
    /// Fixed UDP port for iroh QUIC listener (`0` = OS-assigned ephemeral port).
    #[serde(default)]
    pub bind_port: u16,
    /// HTTP `/metrics` (Prometheus), `/metrics.json`, and `/healthz` bind address.
    /// Set to empty string `""` to disable HTTP listener (P2P `distill metrics` still works).
    #[serde(default = "default_metrics_listen")]
    pub metrics_listen: String,
    /// Maximum active git worktrees retained (LRU threshold, default 10).
    #[serde(default = "default_max_worktrees")]
    pub max_worktrees: usize,
    /// Maximum cached branch semantic indexes retained on disk & memory (LRU threshold, default 10).
    #[serde(default = "default_max_cached_indexes")]
    pub max_cached_indexes: usize,
    /// Maximum in-memory structured raw commit snapshots retained (LRU threshold, default 10).
    #[serde(default = "default_max_memory_snapshots")]
    pub max_memory_snapshots: usize,
    /// Hard memory budget in MiB for the deduplicated raw blob pool (default 512 MiB).
    #[serde(default = "default_max_snapshot_memory_mb")]
    pub max_snapshot_memory_mb: u64,
    /// Dedicated Rayon CPU pool threads for parallel raw grep/hydration (`0` = auto `cpus/2` clamped 2..=12).
    #[serde(default)]
    pub rayon_threads: usize,
    /// Periodic GC interval in seconds for worktrees & cached indexes (default 60; 0 = disabled).
    #[serde(default = "default_gc_interval_secs")]
    pub gc_interval_secs: u64,
    /// Idle TTL in seconds before unused worktrees / cached indexes are evicted (default 1800 = 30m; 0 = LRU count only).
    #[serde(default = "default_worktree_idle_ttl_secs")]
    pub worktree_idle_ttl_secs: u64,
    /// Disk usage warning threshold in MiB (default 4096 = 4 GiB).
    #[serde(default = "default_disk_warn_threshold_mb")]
    pub disk_warn_threshold_mb: u64,
    /// External `config.toml` hot-reload poll interval in seconds (default 5; 0 = disabled).
    #[serde(default = "default_config_reload_interval_secs")]
    pub config_reload_interval_secs: u64,
    /// Optional override for concurrent index builds.
    #[serde(default)]
    pub max_concurrent_builds: Option<usize>,
    /// Optional override for per-peer rate limit per minute.
    #[serde(default)]
    pub rate_limit_per_min: Option<u32>,
}

impl Default for ProgramConfig {
    fn default() -> Self {
        Self {
            bind_port: 0,
            metrics_listen: default_metrics_listen(),
            max_worktrees: default_max_worktrees(),
            max_cached_indexes: default_max_cached_indexes(),
            max_memory_snapshots: default_max_memory_snapshots(),
            max_snapshot_memory_mb: default_max_snapshot_memory_mb(),
            rayon_threads: 0,
            gc_interval_secs: default_gc_interval_secs(),
            worktree_idle_ttl_secs: default_worktree_idle_ttl_secs(),
            disk_warn_threshold_mb: default_disk_warn_threshold_mb(),
            config_reload_interval_secs: default_config_reload_interval_secs(),
            max_concurrent_builds: None,
            rate_limit_per_min: None,
        }
    }
}

fn default_metrics_listen() -> String {
    "0.0.0.0:9464".into()
}
fn default_max_worktrees() -> usize {
    10
}
fn default_max_cached_indexes() -> usize {
    10
}
fn default_max_memory_snapshots() -> usize {
    10
}
fn default_max_snapshot_memory_mb() -> u64 {
    512
}
fn default_gc_interval_secs() -> u64 {
    60
}
fn default_worktree_idle_ttl_secs() -> u64 {
    1800
}
fn default_disk_warn_threshold_mb() -> u64 {
    4096
}
fn default_config_reload_interval_secs() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub server: ProgramConfig,
    #[serde(default = "default_max_concurrent_builds")]
    pub max_concurrent_builds: usize,
    /// Legacy top-level fallback; prefer `[server].max_worktrees` (default 10).
    #[serde(default)]
    pub max_worktrees: Option<usize>,
    #[serde(default = "default_rate_limit_per_min")]
    pub rate_limit_per_min: u32,
    #[serde(default)]
    pub allow_any_peer_dev_only: bool,
    #[serde(default)]
    pub vlogs: VlogsConfig,
    #[serde(default)]
    pub repos: Vec<RepoEntry>,
    #[serde(default)]
    pub clients: Vec<ClientAcl>,
}

fn default_max_concurrent_builds() -> usize {
    4
}
fn default_rate_limit_per_min() -> u32 {
    120
}

impl ServerConfig {
    pub fn effective_max_worktrees(&self) -> usize {
        if self.server.max_worktrees != default_max_worktrees() {
            self.server.max_worktrees.max(1)
        } else {
            self.max_worktrees.unwrap_or(self.server.max_worktrees).max(1)
        }
    }

    pub fn effective_max_cached_indexes(&self) -> usize {
        self.server.max_cached_indexes.max(1)
    }

    pub fn effective_max_memory_snapshots(&self) -> usize {
        self.server.max_memory_snapshots.max(1)
    }

    pub fn effective_max_snapshot_memory_mb(&self) -> u64 {
        self.server.max_snapshot_memory_mb.max(32)
    }

    pub fn effective_max_concurrent_builds(&self) -> usize {
        self.server
            .max_concurrent_builds
            .unwrap_or(self.max_concurrent_builds)
            .max(1)
    }

    pub fn effective_rate_limit_per_min(&self) -> u32 {
        self.server
            .rate_limit_per_min
            .unwrap_or(self.rate_limit_per_min)
            .max(1)
    }

    pub fn metrics_listen_addr(&self) -> Option<String> {
        let s = self.server.metrics_listen.trim();
        (!s.is_empty()).then(|| s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub remote_url: Option<String>,
    pub local_path: Option<PathBuf>,
    #[serde(default = "default_branch")]
    pub default_branch: String,
    #[serde(default = "default_allowed_branches")]
    pub allowed_branches: Vec<String>,
    #[serde(default = "default_fetch_interval")]
    pub fetch_interval_secs: u64,
    /// Optional default VictoriaLogs LogsQL filter scoped to this repository (e.g. `container:twosite-api OR container:rigel`)
    #[serde(default)]
    pub vlogs_filter: Option<String>,
}

fn default_branch() -> String {
    "main".into()
}
fn default_allowed_branches() -> Vec<String> {
    vec!["main".into(), "dev/*".into()]
}
fn default_fetch_interval() -> u64 {
    0
}

impl From<&RepoEntry> for RepoConfigSpec {
    fn from(entry: &RepoEntry) -> Self {
        Self {
            name: entry.name.clone(),
            description: entry.description.clone(),
            remote_url: entry.remote_url.clone(),
            local_path: entry.local_path.clone(),
            default_branch: entry.default_branch.clone(),
            allowed_branches: entry.allowed_branches.clone(),
            fetch_interval_secs: entry.fetch_interval_secs,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientAcl {
    pub endpoint_id: String,
    pub label: String,
    #[serde(default = "default_star")]
    pub allowed_repos: Vec<String>,
    #[serde(default = "default_star")]
    pub allowed_branches: Vec<String>,
    /// Whether this client is permitted to trigger remote `git fetch` via FetchRepo.
    #[serde(default = "default_can_fetch")]
    pub can_fetch: bool,
}

fn default_star() -> Vec<String> {
    vec!["*".into()]
}

fn default_can_fetch() -> bool {
    true
}

impl ServerConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read server config at {}", path.display()))?;
        let cfg: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse TOML config at {}", path.display()))?;
        if cfg.repos.is_empty() {
            return Err(anyhow!("config must define at least one [[repos]] entry"));
        }
        Ok(cfg)
    }

    pub fn authorize_peer(&self, peer_id: &str) -> Option<ClientAcl> {
        if let Some(acl) = self.clients.iter().find(|c| c.endpoint_id == peer_id) {
            return Some(acl.clone());
        }
        if self.allow_any_peer_dev_only {
            return Some(ClientAcl {
                endpoint_id: peer_id.to_string(),
                label: "dev-open".into(),
                allowed_repos: vec!["*".into()],
                allowed_branches: vec!["*".into()],
                can_fetch: true,
            });
        }
        None
    }
}

impl ClientAcl {
    pub fn allows_repo(&self, repo: &str) -> bool {
        self.allowed_repos.iter().any(|p| wildcard_match(p, repo))
    }

    pub fn allows_branch(&self, branch: &str) -> bool {
        self.allowed_branches
            .iter()
            .any(|p| wildcard_match(p, branch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_manual_fetch_and_parses_can_fetch() {
        let raw = r#"
[[repos]]
name = "twosite"
local_path = "/tmp/twosite.git"

[[clients]]
endpoint_id = "aaa"
label = "admin"

[[clients]]
endpoint_id = "bbb"
label = "readonly-agent"
can_fetch = false
"#;
        let cfg: ServerConfig = toml::from_str(raw).unwrap();
        assert_eq!(cfg.repos[0].fetch_interval_secs, 0);
        assert!(cfg.clients[0].can_fetch);
        assert!(!cfg.clients[1].can_fetch);
    }
}
