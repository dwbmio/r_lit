use crate::ServerState;
use crate::config::ServerConfig;
use distill_git::dir_size_bytes;
use distill_proto::{RepoDiskMetric, ServerMetrics};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{info, warn};

/// Collect live disk usage, worktree inventory, branch index inventory, thresholds, and counters.
pub async fn collect_server_metrics(state: &ServerState) -> ServerMetrics {
    let cfg = state.current_config();
    let git = state.index.git();

    let worktrees = git.worktree_metrics();
    let cached_indexes = state.index.snapshot_index_metrics();
    let snap_pool = git.snapshot_metrics();
    let repo_specs = git.list_repo_specs();

    let mut repos = Vec::with_capacity(repo_specs.len());
    let mut repos_disk_bytes = 0u64;

    for spec in &repo_specs {
        let repo_disk = git
            .repo_disk_path(&spec.name)
            .map(|p| dir_size_bytes(&p))
            .unwrap_or(0);
        repos_disk_bytes = repos_disk_bytes.saturating_add(repo_disk);

        let wt_for_repo: Vec<_> = worktrees.iter().filter(|w| w.repo == spec.name).collect();
        let wt_bytes: u64 = wt_for_repo.iter().map(|w| w.disk_bytes).sum();

        let idx_for_repo: Vec<_> = cached_indexes
            .iter()
            .filter(|i| i.repo == spec.name)
            .collect();
        let idx_bytes: u64 = idx_for_repo.iter().map(|i| i.disk_bytes).sum();

        let branches_count = git
            .list_branches(&spec.name)
            .await
            .map(|b| b.len())
            .unwrap_or(0);

        let mut active_set = BTreeSet::new();
        for w in &wt_for_repo {
            if !w.branch.is_empty() {
                active_set.insert(w.branch.clone());
            }
        }
        for i in &idx_for_repo {
            if !i.branch.is_empty() {
                active_set.insert(i.branch.clone());
            }
        }
        for s in snap_pool.snapshots.iter().filter(|s| s.repo == spec.name) {
            if !s.branch.is_empty() {
                active_set.insert(s.branch.clone());
            }
        }

        repos.push(RepoDiskMetric {
            name: spec.name.clone(),
            mode: if spec.local_path.is_some() {
                "local_mirror".into()
            } else {
                "remote_bare".into()
            },
            repo_disk_bytes: repo_disk,
            worktrees_count: wt_for_repo.len(),
            worktrees_disk_bytes: wt_bytes,
            cached_indexes_count: idx_for_repo.len(),
            indexes_disk_bytes: idx_bytes,
            branches_count,
            active_branches: active_set.into_iter().collect(),
        });
    }

    let worktrees_disk_bytes = dir_size_bytes(&state.app_home.join("worktrees"));
    let indexes_disk_bytes = dir_size_bytes(&state.app_home.join("indexes"));
    let blob_cache_disk_bytes = dir_size_bytes(&state.app_home.join("blob-cache"));
    let audit_log_bytes = dir_size_bytes(&state.app_home.join("audit.jsonl"));
    let total_disk_bytes = repos_disk_bytes
        .saturating_add(worktrees_disk_bytes)
        .saturating_add(indexes_disk_bytes)
        .saturating_add(blob_cache_disk_bytes)
        .saturating_add(audit_log_bytes);

    let warn_bytes = cfg.server.disk_warn_threshold_mb.saturating_mul(1024 * 1024);
    let disk_over_threshold = warn_bytes > 0 && total_disk_bytes >= warn_bytes;

    ServerMetrics {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: state.started_at.elapsed().as_secs(),
        metrics_listen: cfg.metrics_listen_addr(),
        total_requests: state.total_requests.load(Ordering::Relaxed),
        total_dlp_redactions: state.total_dlp_redactions.load(Ordering::Relaxed),
        config_reloads_total: state.config_reloads_total.load(Ordering::Relaxed),
        gc_evictions_total: state.gc_evictions_total.load(Ordering::Relaxed),
        max_worktrees: cfg.effective_max_worktrees(),
        max_cached_indexes: cfg.effective_max_cached_indexes(),
        max_memory_snapshots: snap_pool.max_snapshots,
        max_snapshot_memory_mb: snap_pool.max_memory_bytes / (1024 * 1024),
        rayon_threads: snap_pool.rayon_threads,
        worktree_idle_ttl_secs: cfg.server.worktree_idle_ttl_secs,
        gc_interval_secs: cfg.server.gc_interval_secs,
        disk_warn_threshold_mb: cfg.server.disk_warn_threshold_mb,
        config_reload_interval_secs: cfg.server.config_reload_interval_secs,
        repos_disk_bytes,
        worktrees_disk_bytes,
        indexes_disk_bytes,
        blob_cache_disk_bytes,
        audit_log_bytes,
        total_disk_bytes,
        disk_over_threshold,
        snapshot_memory_bytes: snap_pool.memory_bytes,
        snapshot_blobs_cached: snap_pool.cached_blobs,
        repos,
        worktrees,
        cached_indexes,
        memory_snapshots: snap_pool.snapshots,
    }
}

/// Render `ServerMetrics` in Prometheus text exposition format v0.0.4.
pub fn render_prometheus(m: &ServerMetrics) -> String {
    let mut out = String::with_capacity(4096);

    out.push_str("# HELP distill_uptime_seconds Server uptime in seconds.\n");
    out.push_str("# TYPE distill_uptime_seconds gauge\n");
    out.push_str(&format!("distill_uptime_seconds {}\n", m.uptime_secs));

    out.push_str("# HELP distill_requests_total Total authenticated RPC requests served.\n");
    out.push_str("# TYPE distill_requests_total counter\n");
    out.push_str(&format!("distill_requests_total {}\n", m.total_requests));

    out.push_str("# HELP distill_dlp_redactions_total Total outbound secret redactions.\n");
    out.push_str("# TYPE distill_dlp_redactions_total counter\n");
    out.push_str(&format!("distill_dlp_redactions_total {}\n", m.total_dlp_redactions));

    out.push_str("# HELP distill_config_reloads_total Total hot-reloads of config.toml.\n");
    out.push_str("# TYPE distill_config_reloads_total counter\n");
    out.push_str(&format!("distill_config_reloads_total {}\n", m.config_reloads_total));

    out.push_str("# HELP distill_gc_evictions_total Total worktree and index cache evictions.\n");
    out.push_str("# TYPE distill_gc_evictions_total counter\n");
    out.push_str(&format!("distill_gc_evictions_total {}\n", m.gc_evictions_total));

    out.push_str("# HELP distill_worktree_max_threshold Configured LRU worktree retention threshold.\n");
    out.push_str("# TYPE distill_worktree_max_threshold gauge\n");
    out.push_str(&format!("distill_worktree_max_threshold {}\n", m.max_worktrees));

    out.push_str("# HELP distill_cached_indexes_max_threshold Configured LRU branch index retention threshold.\n");
    out.push_str("# TYPE distill_cached_indexes_max_threshold gauge\n");
    out.push_str(&format!(
        "distill_cached_indexes_max_threshold {}\n",
        m.max_cached_indexes
    ));

    out.push_str("# HELP distill_worktree_idle_ttl_seconds Configured idle TTL before eviction.\n");
    out.push_str("# TYPE distill_worktree_idle_ttl_seconds gauge\n");
    out.push_str(&format!(
        "distill_worktree_idle_ttl_seconds {}\n",
        m.worktree_idle_ttl_secs
    ));

    out.push_str("# HELP distill_worktrees_active Active detached git worktrees.\n");
    out.push_str("# TYPE distill_worktrees_active gauge\n");
    out.push_str(&format!("distill_worktrees_active {}\n", m.worktrees.len()));

    out.push_str("# HELP distill_cached_indexes_active Active cached branch semantic indexes.\n");
    out.push_str("# TYPE distill_cached_indexes_active gauge\n");
    out.push_str(&format!(
        "distill_cached_indexes_active {}\n",
        m.cached_indexes.len()
    ));

    out.push_str("# HELP distill_snapshots_active Active in-memory structured raw commit snapshots.\n");
    out.push_str("# TYPE distill_snapshots_active gauge\n");
    out.push_str(&format!(
        "distill_snapshots_active {}\n",
        m.memory_snapshots.len()
    ));

    out.push_str("# HELP distill_snapshots_max_threshold Configured LRU memory snapshot retention threshold.\n");
    out.push_str("# TYPE distill_snapshots_max_threshold gauge\n");
    out.push_str(&format!(
        "distill_snapshots_max_threshold {}\n",
        m.max_memory_snapshots
    ));

    out.push_str("# HELP distill_snapshot_blobs_cached Deduplicated pre-sanitized raw text blobs in RAM.\n");
    out.push_str("# TYPE distill_snapshot_blobs_cached gauge\n");
    out.push_str(&format!(
        "distill_snapshot_blobs_cached {}\n",
        m.snapshot_blobs_cached
    ));

    out.push_str("# HELP distill_snapshot_memory_bytes Estimated RAM used by deduplicated raw blob pool.\n");
    out.push_str("# TYPE distill_snapshot_memory_bytes gauge\n");
    out.push_str(&format!(
        "distill_snapshot_memory_bytes {}\n",
        m.snapshot_memory_bytes
    ));

    out.push_str("# HELP distill_snapshot_memory_limit_bytes Configured hard RAM limit for raw blob pool.\n");
    out.push_str("# TYPE distill_snapshot_memory_limit_bytes gauge\n");
    out.push_str(&format!(
        "distill_snapshot_memory_limit_bytes {}\n",
        m.max_snapshot_memory_mb.saturating_mul(1024 * 1024)
    ));

    out.push_str("# HELP distill_rayon_threads Dedicated Rayon worker threads for parallel raw scans.\n");
    out.push_str("# TYPE distill_rayon_threads gauge\n");
    out.push_str(&format!("distill_rayon_threads {}\n", m.rayon_threads));

    out.push_str("# HELP distill_data_dir_disk_bytes Disk usage by distilld storage area.\n");
    out.push_str("# TYPE distill_data_dir_disk_bytes gauge\n");
    out.push_str(&format!(
        "distill_data_dir_disk_bytes{{area=\"repos\"}} {}\n",
        m.repos_disk_bytes
    ));
    out.push_str(&format!(
        "distill_data_dir_disk_bytes{{area=\"worktrees\"}} {}\n",
        m.worktrees_disk_bytes
    ));
    out.push_str(&format!(
        "distill_data_dir_disk_bytes{{area=\"indexes\"}} {}\n",
        m.indexes_disk_bytes
    ));
    out.push_str(&format!(
        "distill_data_dir_disk_bytes{{area=\"blob_cache\"}} {}\n",
        m.blob_cache_disk_bytes
    ));
    out.push_str(&format!(
        "distill_data_dir_disk_bytes{{area=\"audit_log\"}} {}\n",
        m.audit_log_bytes
    ));

    out.push_str("# HELP distill_total_disk_bytes Total managed disk usage in bytes.\n");
    out.push_str("# TYPE distill_total_disk_bytes gauge\n");
    out.push_str(&format!("distill_total_disk_bytes {}\n", m.total_disk_bytes));

    out.push_str("# HELP distill_disk_warn_threshold_bytes Configured disk warning threshold in bytes.\n");
    out.push_str("# TYPE distill_disk_warn_threshold_bytes gauge\n");
    out.push_str(&format!(
        "distill_disk_warn_threshold_bytes {}\n",
        m.disk_warn_threshold_mb.saturating_mul(1024 * 1024)
    ));

    out.push_str("# HELP distill_disk_over_threshold 1 if total disk usage meets/exceeds warn threshold.\n");
    out.push_str("# TYPE distill_disk_over_threshold gauge\n");
    out.push_str(&format!(
        "distill_disk_over_threshold {}\n",
        if m.disk_over_threshold { 1 } else { 0 }
    ));

    out.push_str("# HELP distill_repo_disk_bytes Bare repository disk usage in bytes.\n");
    out.push_str("# TYPE distill_repo_disk_bytes gauge\n");
    for r in &m.repos {
        let name = prom_escape(&r.name);
        let mode = prom_escape(&r.mode);
        out.push_str(&format!(
            "distill_repo_disk_bytes{{repo=\"{name}\",mode=\"{mode}\"}} {}\n",
            r.repo_disk_bytes
        ));
        out.push_str(&format!(
            "distill_repo_branches_total{{repo=\"{name}\"}} {}\n",
            r.branches_count
        ));
        out.push_str(&format!(
            "distill_repo_worktrees_active{{repo=\"{name}\"}} {}\n",
            r.worktrees_count
        ));
        out.push_str(&format!(
            "distill_repo_cached_indexes{{repo=\"{name}\"}} {}\n",
            r.cached_indexes_count
        ));
    }

    if !m.worktrees.is_empty() {
        out.push_str("# HELP distill_worktree_disk_bytes Per-worktree disk usage by repo and branch.\n");
        out.push_str("# TYPE distill_worktree_disk_bytes gauge\n");
        for w in &m.worktrees {
            let repo = prom_escape(&w.repo);
            let branch = prom_escape(&w.branch);
            let sha = prom_escape(&w.commit_sha[..w.commit_sha.len().min(10)]);
            out.push_str(&format!(
                "distill_worktree_disk_bytes{{repo=\"{repo}\",branch=\"{branch}\",sha=\"{sha}\"}} {}\n",
                w.disk_bytes
            ));
            out.push_str(&format!(
                "distill_worktree_idle_seconds{{repo=\"{repo}\",branch=\"{branch}\",sha=\"{sha}\"}} {}\n",
                w.idle_secs
            ));
        }
    }

    if !m.cached_indexes.is_empty() {
        out.push_str("# HELP distill_branch_index_disk_bytes Cached branch index disk usage.\n");
        out.push_str("# TYPE distill_branch_index_disk_bytes gauge\n");
        for idx in &m.cached_indexes {
            let repo = prom_escape(&idx.repo);
            let branch = prom_escape(&idx.branch);
            let sha = prom_escape(&idx.commit_sha[..idx.commit_sha.len().min(10)]);
            out.push_str(&format!(
                "distill_branch_index_disk_bytes{{repo=\"{repo}\",branch=\"{branch}\",sha=\"{sha}\"}} {}\n",
                idx.disk_bytes
            ));
            out.push_str(&format!(
                "distill_branch_index_idle_seconds{{repo=\"{repo}\",branch=\"{branch}\",sha=\"{sha}\"}} {}\n",
                idx.idle_secs
            ));
        }
    }

    out
}

fn prom_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Spawn lightweight pure-Rust HTTP `/metrics`, `/metrics.json`, and `/healthz` listener.
pub fn spawn_metrics_http_server(bind_addr: String, state: Arc<ServerState>) {
    tokio::spawn(async move {
        let listener = match TcpListener::bind(&bind_addr).await {
            Ok(l) => {
                info!("HTTP metrics server listening on http://{bind_addr}/metrics");
                l
            }
            Err(e) => {
                warn!("failed to bind HTTP metrics listener on '{bind_addr}': {e}");
                return;
            }
        };

        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                continue;
            };
            let st = Arc::clone(&state);
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let Ok(n) = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut buf))
                    .await
                    .unwrap_or(Ok(0))
                else {
                    return;
                };
                if n == 0 {
                    return;
                }
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let path = first_line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("/")
                    .split('?')
                    .next()
                    .unwrap_or("/");

                let (status, content_type, body) = match path {
                    "/metrics" => {
                        let m = collect_server_metrics(&st).await;
                        (
                            "200 OK",
                            "text/plain; version=0.0.4; charset=utf-8",
                            render_prometheus(&m),
                        )
                    }
                    "/metrics.json" => {
                        let m = collect_server_metrics(&st).await;
                        let body = serde_json::to_string_pretty(&m).unwrap_or_else(|_| "{}".into());
                        ("200 OK", "application/json; charset=utf-8", body)
                    }
                    "/healthz" | "/" => (
                        "200 OK",
                        "application/json; charset=utf-8",
                        format!(
                            "{{\"status\":\"ok\",\"version\":\"{}\",\"uptime_secs\":{}}}\n",
                            env!("CARGO_PKG_VERSION"),
                            st.started_at.elapsed().as_secs()
                        ),
                    ),
                    _ => (
                        "404 Not Found",
                        "text/plain; charset=utf-8",
                        "not found (available: /metrics, /metrics.json, /healthz)\n".to_string(),
                    ),
                };

                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
}

/// Spawn periodic local hygiene (worktree + branch index LRU/TTL GC) and external `config.toml` hot-reload.
/// Note: NEVER triggers any upstream Git fetch — strictly local maintenance & config reload.
pub fn spawn_maintenance_tasks(state: Arc<ServerState>, config_path: PathBuf) {
    // Task 1: Periodic worktree & cached branch index GC based on activity & threshold
    let gc_state = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            let cfg = gc_state.current_config();
            let interval_secs = cfg.server.gc_interval_secs;
            let sleep_secs = if interval_secs == 0 {
                30
            } else {
                interval_secs.clamp(5, 3600)
            };
            tokio::time::sleep(Duration::from_secs(sleep_secs)).await;

            let cfg = gc_state.current_config();
            if cfg.server.gc_interval_secs == 0 {
                continue;
            }
            let max_wt = cfg.effective_max_worktrees();
            let max_idx = cfg.effective_max_cached_indexes();
            let max_snaps = cfg.effective_max_memory_snapshots();
            let max_snap_mb = cfg.effective_max_snapshot_memory_mb();
            let ttl = cfg.server.worktree_idle_ttl_secs;

            let wt_evicted = gc_state.index.git().gc_worktrees(max_wt, ttl);
            let idx_evicted = gc_state.index.gc_indexes(max_idx, ttl);
            let snap_evicted = gc_state
                .index
                .git()
                .gc_snapshots(max_snaps, max_snap_mb, ttl);
            let total_evicted = wt_evicted + idx_evicted + snap_evicted;
            if total_evicted > 0 {
                gc_state
                    .gc_evictions_total
                    .fetch_add(total_evicted as u64, Ordering::Relaxed);
                info!(
                    "periodic GC evicted {} worktrees, {} cached indexes, {} memory snapshots (thresholds: wt={}, idx={}, snap={}, mem={}MiB, idle_ttl={}s)",
                    wt_evicted, idx_evicted, snap_evicted, max_wt, max_idx, max_snaps, max_snap_mb, ttl
                );
            }
        }
    });

    // Task 2: External `config.toml` hot-reload watcher
    let reload_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut last_raw = std::fs::read_to_string(&config_path).unwrap_or_default();
        loop {
            let poll_secs = reload_state
                .current_config()
                .server
                .config_reload_interval_secs;
            let wait = if poll_secs == 0 {
                15
            } else {
                poll_secs.clamp(2, 300)
            };
            tokio::time::sleep(Duration::from_secs(wait)).await;

            if reload_state
                .current_config()
                .server
                .config_reload_interval_secs
                == 0
            {
                continue;
            }

            let Ok(raw) = std::fs::read_to_string(&config_path) else {
                continue;
            };
            if raw == last_raw {
                continue;
            }
            match ServerConfig::load(&config_path) {
                Ok(new_cfg) => {
                    last_raw = raw;
                    reload_state.apply_reloaded_config(new_cfg);
                    let count = reload_state
                        .config_reloads_total
                        .fetch_add(1, Ordering::Relaxed)
                        + 1;
                    info!(
                        "hot-reloaded {} (#{}): clients={}, repos={}, max_worktrees={}, max_cached_indexes={}",
                        config_path.display(),
                        count,
                        reload_state.current_config().clients.len(),
                        reload_state.current_config().repos.len(),
                        reload_state.current_config().effective_max_worktrees(),
                        reload_state.current_config().effective_max_cached_indexes(),
                    );
                }
                Err(e) => {
                    warn!(
                        "ignored invalid external edit to {} (keeping active config): {e:#}",
                        config_path.display()
                    );
                    last_raw = raw;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use distill_proto::{BranchIndexMetric, WorktreeMetric};

    #[test]
    fn renders_valid_prometheus_metrics() {
        let sample = ServerMetrics {
            server_version: "1.1.0".into(),
            uptime_secs: 120,
            metrics_listen: Some("0.0.0.0:9464".into()),
            total_requests: 42,
            total_dlp_redactions: 3,
            config_reloads_total: 1,
            gc_evictions_total: 2,
            max_worktrees: 10,
            max_cached_indexes: 10,
            max_memory_snapshots: 10,
            max_snapshot_memory_mb: 512,
            rayon_threads: 8,
            worktree_idle_ttl_secs: 1800,
            gc_interval_secs: 60,
            disk_warn_threshold_mb: 4096,
            config_reload_interval_secs: 5,
            repos_disk_bytes: 100_000_000,
            worktrees_disk_bytes: 20_000_000,
            indexes_disk_bytes: 5_000_000,
            blob_cache_disk_bytes: 2_000_000,
            audit_log_bytes: 50_000,
            total_disk_bytes: 127_050_000,
            disk_over_threshold: false,
            snapshot_memory_bytes: 32_000_000,
            snapshot_blobs_cached: 2800,
            repos: vec![RepoDiskMetric {
                name: "twosite".into(),
                mode: "local_mirror".into(),
                repo_disk_bytes: 100_000_000,
                worktrees_count: 1,
                worktrees_disk_bytes: 20_000_000,
                cached_indexes_count: 1,
                indexes_disk_bytes: 5_000_000,
                branches_count: 2,
                active_branches: vec!["dev/v1".into()],
            }],
            worktrees: vec![WorktreeMetric {
                repo: "twosite".into(),
                branch: "dev/v1".into(),
                commit_sha: "e0f0c73c8d92f806".into(),
                disk_bytes: 20_000_000,
                idle_secs: 15,
            }],
            cached_indexes: vec![BranchIndexMetric {
                repo: "twosite".into(),
                branch: "dev/v1".into(),
                commit_sha: "e0f0c73c8d92f806".into(),
                total_cards: 7018,
                disk_bytes: 5_000_000,
                idle_secs: 10,
            }],
            memory_snapshots: Vec::new(),
        };
        let text = render_prometheus(&sample);
        assert!(text.contains("distill_worktree_max_threshold 10"));
        assert!(text.contains("distill_repo_disk_bytes{repo=\"twosite\",mode=\"local_mirror\"} 100000000"));
        assert!(text.contains("distill_worktree_disk_bytes{repo=\"twosite\",branch=\"dev/v1\",sha=\"e0f0c73c8d\"} 20000000"));
        assert!(text.contains("distill_config_reloads_total 1"));
    }
}
