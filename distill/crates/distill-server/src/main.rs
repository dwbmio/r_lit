mod config;
mod investigate;
mod metrics;
mod vlogs;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use config::{ClientAcl, ServerConfig};
use vlogs::VictoriaLogsClient;
use dashmap::DashMap;
use governor::{Quota, RateLimiter};
use iroh::endpoint::presets;
use iroh::{Endpoint, SecretKey};
use distill_crypto::{DlpGate, EnvelopeCipher};
use distill_git::GitRepoManager;
use distill_index::IndexService;
use distill_proto::{
    APP_NAME, DISTILL_ALPN, DistillRequest, DistillResponse, FetchRepoResult, MAX_FRAME_BYTES,
    RepoSummary, SemanticKind, ServerStatusInfo, decode_payload, encode_frame, resolve_app_home,
};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(
    name = "distilld",
    version,
    about = "Pure Rust P2P (iroh QUIC) multi-branch semantic code Q&A server (zero raw source leak)"
)]
struct Cli {
    /// Root data directory (defaults to $HOME/.distill or $DISTILL_HOME)
    #[arg(long, global = true)]
    home: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize $HOME/.distill, generate persistent Ed25519 identity & master key
    Init {
        /// Optional local path or remote git URL for initial twosite repo entry
        #[arg(long)]
        repo_path: Option<PathBuf>,
        #[arg(long)]
        repo_url: Option<String>,
    },
    /// Print this server's iroh EndpointId (public key)
    PrintId,
    /// Start the iroh P2P QUIC server
    Serve {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Local smoke command: index a repo+branch and run a sanitized semantic search
    LocalSearch {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
}

type PeerLimiter = RateLimiter<
    governor::state::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

pub(crate) struct ServerState {
    pub(crate) app_home: PathBuf,
    cfg: RwLock<Arc<ServerConfig>>,
    pub(crate) index: Arc<IndexService>,
    pub(crate) vlogs: VictoriaLogsClient,
    limiters: DashMap<String, Arc<PeerLimiter>>,
    pub(crate) started_at: Instant,
    pub(crate) total_requests: AtomicU64,
    pub(crate) total_dlp_redactions: AtomicU64,
    pub(crate) config_reloads_total: AtomicU64,
    pub(crate) gc_evictions_total: AtomicU64,
}

impl ServerState {
    pub(crate) fn current_config(&self) -> Arc<ServerConfig> {
        Arc::clone(&self.cfg.read().unwrap())
    }

    pub(crate) fn apply_reloaded_config(&self, next: ServerConfig) {
        let max_wt = next.effective_max_worktrees();
        let max_snaps = next.effective_max_memory_snapshots();
        let max_snap_mb = next.effective_max_snapshot_memory_mb();
        let specs = next.repos.iter().map(Into::into).collect();
        self.index.git().set_max_worktrees(max_wt);
        self.index.git().set_snapshot_limits(max_snaps, max_snap_mb);
        self.index.git().reload_repos(specs);
        self.vlogs.update_config(next.vlogs.clone());
        self.limiters.clear();
        if let Ok(mut guard) = self.cfg.write() {
            *guard = Arc::new(next);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,iroh=warn,tantivy=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let app_home = cli
        .home
        .unwrap_or_else(|| resolve_app_home(APP_NAME, "DISTILL_HOME"));
    fs::create_dir_all(&app_home)?;

    match cli.command {
        Commands::Init {
            repo_path,
            repo_url,
        } => cmd_init(&app_home, repo_path, repo_url),
        Commands::PrintId => {
            let key = load_or_create_iroh_key(&app_home.join("server.key"))?;
            println!("{}", key.public());
            Ok(())
        }
        Commands::Serve { config } => {
            let cfg_path = config.unwrap_or_else(|| app_home.join("config.toml"));
            cmd_serve(&app_home, &cfg_path).await
        }
        Commands::LocalSearch {
            config,
            repo,
            branch,
            query,
            limit,
        } => {
            let cfg_path = config.unwrap_or_else(|| app_home.join("config.toml"));
            let cfg = ServerConfig::load(&cfg_path)?;
            let cipher = EnvelopeCipher::load_or_generate(&app_home.join("master.key"))?;
            let specs = cfg.repos.iter().map(Into::into).collect();
            let git = Arc::new(GitRepoManager::new(
                &app_home,
                specs,
                cfg.effective_max_worktrees(),
            ));
            let idx = IndexService::new(
                app_home.clone(),
                git,
                cipher,
                cfg.effective_max_concurrent_builds(),
            );
            let res = idx.search_branch(&repo, &branch, &query, None, limit).await?;
            println!("{}", serde_json::to_string_pretty(&res)?);
            Ok(())
        }
    }
}

fn cmd_init(app_home: &Path, repo_path: Option<PathBuf>, repo_url: Option<String>) -> Result<()> {
    let server_key = load_or_create_iroh_key(&app_home.join("server.key"))?;
    let _cipher = EnvelopeCipher::load_or_generate(&app_home.join("master.key"))?;

    let cfg_path = app_home.join("config.toml");
    if !cfg_path.exists() {
        let repo_line = if let Some(p) = repo_path {
            format!("local_path = \"{}\"", p.display())
        } else if let Some(u) = repo_url {
            format!("remote_url = \"{u}\"")
        } else {
            "remote_url = \"https://git.7k7k.com/starlink/twosite.git\"".to_string()
        };

        let sample = format!(
            r#"# 一级程序管理配置（修改本文件后每 5 秒自动热加载，无需重启 distilld）
[server]
# 固定 UDP 监听端口（0 = 随机分配；填入固定端口后重启服务不换 direct_addr）
bind_port = 0
# HTTP /metrics（Prometheus）、/metrics.json 与 /healthz 监控监听地址（空串 "" 禁用）
metrics_listen = "0.0.0.0:9464"
# Worktree 与分支语义索引活跃保留阈值（超出按最近活跃 LRU 定时清理）
max_worktrees = 10
max_cached_indexes = 10
# 定时清理周期（秒，0 = 禁用）与闲置过期时间（秒，默认 1800 = 30 分钟，0 = 仅按数量阈值清理）
gc_interval_secs = 60
worktree_idle_ttl_secs = 1800
# 磁盘占用告警阈值（MiB，默认 4096 = 4 GiB）
disk_warn_threshold_mb = 4096
# 外部配置热重载轮询周期（秒，0 = 禁用）
config_reload_interval_secs = 5
max_concurrent_builds = 4
rate_limit_per_min = 120

allow_any_peer_dev_only = false

[vlogs]
enabled = true
endpoint = "http://vlogs.hs99.vip:9428"
default_range = "15m"
default_limit = 50

[[repos]]
name = "twosite"
description = "Rigel / TwoSite 项目工作管理与流程编排平台"
{repo_line}
default_branch = "main"
allowed_branches = ["main", "dev/*"]
# 0 = 禁用自动/定时拉取，仅允许授权客户端点击 [⟳ Fetch] 或执行 `distill fetch` 时手动拉取（推荐，满足安全审计）
fetch_interval_secs = 0

# 为每个客户端添加一条 [[clients]]（客户端执行 `distill print-id` 获取 endpoint_id）
# [[clients]]
# endpoint_id = "<CLIENT_ENDPOINT_ID>"
# label = "pm-alice"
# allowed_repos = ["twosite"]
# allowed_branches = ["main", "dev/*"]
# can_fetch = false   # 仅管理员/研发设为 true，普通只读客户端可设为 false 禁止触发上游 Fetch
"#
        );
        fs::write(&cfg_path, sample)?;
    }

    println!("Initialized Distill home: {}", app_home.display());
    println!("Server EndpointId: {}", server_key.public());
    println!("Config path: {}", cfg_path.display());
    Ok(())
}

async fn cmd_serve(app_home: &Path, cfg_path: &Path) -> Result<()> {
    let cfg = ServerConfig::load(cfg_path)?;
    let server_key = load_or_create_iroh_key(&app_home.join("server.key"))?;
    let cipher = EnvelopeCipher::load_or_generate(&app_home.join("master.key"))?;

    let specs = cfg.repos.iter().map(Into::into).collect();
    let git = Arc::new(GitRepoManager::new_with_limits(
        app_home,
        specs,
        cfg.effective_max_worktrees(),
        cfg.effective_max_memory_snapshots(),
        cfg.effective_max_snapshot_memory_mb(),
        cfg.server.rayon_threads,
    ));
    let index = Arc::new(IndexService::new(
        app_home.to_path_buf(),
        git,
        cipher,
        cfg.effective_max_concurrent_builds(),
    ));

    let vlogs = VictoriaLogsClient::new(cfg.vlogs.clone())?;
    let bind_port = cfg.server.bind_port;
    let metrics_addr = cfg.metrics_listen_addr();

    init_async_audit_writer(app_home.join("audit.jsonl"));

    let state = Arc::new(ServerState {
        app_home: app_home.to_path_buf(),
        cfg: RwLock::new(Arc::new(cfg)),
        index,
        vlogs,
        limiters: DashMap::new(),
        started_at: Instant::now(),
        total_requests: AtomicU64::new(0),
        total_dlp_redactions: AtomicU64::new(0),
        config_reloads_total: AtomicU64::new(0),
        gc_evictions_total: AtomicU64::new(0),
    });

    if let Some(ref addr) = metrics_addr {
        metrics::spawn_metrics_http_server(addr.clone(), Arc::clone(&state));
    }
    metrics::spawn_maintenance_tasks(Arc::clone(&state), cfg_path.to_path_buf());

    let mut ep_builder = Endpoint::builder(presets::N0)
        .secret_key(server_key)
        .alpns(vec![DISTILL_ALPN.to_vec()]);
    if bind_port > 0 {
        ep_builder = ep_builder.bind_addr(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, bind_port))?;
    }
    let endpoint = ep_builder
        .bind()
        .await
        .context("failed to bind iroh QUIC endpoint")?;

    // Give discovery/relay a moment to settle and print reachable address info
    let addr = endpoint.addr();
    info!("Distill server started");
    info!("  Home dir   : {}", app_home.display());
    info!("  EndpointId : {}", endpoint.id());
    info!("  Addrs      : {:?}", addr);
    println!("SERVER_ENDPOINT_ID={}", endpoint.id());
    println!("SERVER_ADDR_JSON={}", serde_json::to_string(&addr)?);

    loop {
        tokio::select! {
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    info!("iroh endpoint closed");
                    break;
                };
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(err) = handle_connection(incoming, state).await {
                        warn!("connection ended with error: {err:#}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                info!("received Ctrl-C, shutting down server");
                break;
            }
        }
    }

    endpoint.close().await;
    Ok(())
}

async fn handle_connection(
    incoming: iroh::endpoint::Incoming,
    state: Arc<ServerState>,
) -> Result<()> {
    let conn = incoming.await.context("failed to accept QUIC connection")?;
    let peer_id = conn.remote_id().to_string();

    let Some(acl) = state.current_config().authorize_peer(&peer_id) else {
        warn!("rejected unauthorized peer: {peer_id}");
        append_audit(
            &state.app_home,
            &peer_id,
            "UNAUTHORIZED",
            "connect",
            "",
            "",
            0,
            "DENIED",
        );
        conn.close(403u32.into(), b"unauthorized peer");
        return Ok(());
    };

    info!("accepted authorized peer '{}' ({peer_id})", acl.label);

    loop {
        let (send, recv) = match conn.accept_bi().await {
            Ok(streams) => streams,
            Err(_) => break,
        };
        let state = Arc::clone(&state);
        let acl = acl.clone();
        let peer_id = peer_id.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_stream(send, recv, state, acl, peer_id).await {
                error!("stream error: {err:#}");
            }
        });
    }

    Ok(())
}

async fn handle_stream(
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
    state: Arc<ServerState>,
    acl: ClientAcl,
    peer_id: String,
) -> Result<()> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let frame_len = u32::from_be_bytes(len_buf) as usize;
    if frame_len > MAX_FRAME_BYTES {
        let resp = DistillResponse::Error {
            code: "FRAME_TOO_LARGE".into(),
            message: "请求帧超出最大允许长度".into(),
        };
        send.write_all(&encode_frame(&resp)?).await?;
        send.finish()?;
        return Ok(());
    }

    let mut payload = vec![0u8; frame_len];
    recv.read_exact(&mut payload).await?;
    let request: DistillRequest = decode_payload(&payload)?;

    // Re-evaluate live hot-reloaded ACL per stream so whitelist / can_fetch updates apply immediately
    let live_cfg = state.current_config();
    let Some(live_acl) = live_cfg.authorize_peer(&peer_id) else {
        let resp = forbidden("客户端已被移出白名单");
        send.write_all(&encode_frame(&resp)?).await?;
        send.finish()?;
        return Ok(());
    };
    let _ = acl;
    let acl = live_acl;

    // Per-peer token bucket rate limiting
    let rate_limit = live_cfg.effective_rate_limit_per_min();
    let limiter = state
        .limiters
        .entry(peer_id.clone())
        .or_insert_with(|| {
            let quota = Quota::per_minute(NonZeroU32::new(rate_limit.max(1)).unwrap());
            Arc::new(RateLimiter::direct(quota))
        })
        .clone();

    let response = if limiter.check().is_err() {
        append_audit(
            &state.app_home,
            &peer_id,
            &acl.label,
            "rate_limited",
            "",
            "",
            0,
            "RATE_LIMITED",
        );
        DistillResponse::Error {
            code: "RATE_LIMITED".into(),
            message: "请求频率超限，请稍后重试".into(),
        }
    } else {
        dispatch_request(&state, &acl, &peer_id, request).await
    };

    let bytes = encode_frame(&response)?;
    send.write_all(&bytes).await?;
    send.finish()?;
    Ok(())
}

async fn dispatch_request(
    state: &ServerState,
    acl: &ClientAcl,
    peer_id: &str,
    req: DistillRequest,
) -> DistillResponse {
    let req_started = Instant::now();
    state.total_requests.fetch_add(1, Ordering::Relaxed);
    match req {
        DistillRequest::Ping => DistillResponse::Pong {
            server_version: env!("CARGO_PKG_VERSION").into(),
            protocol: String::from_utf8_lossy(DISTILL_ALPN).into_owned(),
        },
        DistillRequest::ServerStatus => {
            let repos = state
                .index
                .git()
                .repo_statuses()
                .await
                .into_iter()
                .filter(|r| acl.allows_repo(&r.name))
                .collect();
            DistillResponse::Status(ServerStatusInfo {
                server_version: env!("CARGO_PKG_VERSION").into(),
                protocol: String::from_utf8_lossy(DISTILL_ALPN).into_owned(),
                uptime_secs: state.started_at.elapsed().as_secs(),
                peer_label: acl.label.clone(),
                cached_branch_indexes: state.index.cached_index_count(),
                active_worktrees: state.index.git().active_worktree_count(),
                total_requests: state.total_requests.load(Ordering::Relaxed),
                total_dlp_redactions: state.total_dlp_redactions.load(Ordering::Relaxed),
                repos,
                vlogs_endpoint: state.vlogs.endpoint(),
            })
        }
        DistillRequest::ListRepos => {
            let repos = state
                .index
                .git()
                .list_repo_specs()
                .into_iter()
                .filter(|r| acl.allows_repo(&r.name))
                .map(|r| RepoSummary {
                    name: r.name,
                    description: r.description,
                    default_branch: r.default_branch,
                    allowed_branches: r.allowed_branches,
                })
                .collect();
            append_audit(&state.app_home, peer_id, &acl.label, "list_repos", "", "", 0, "OK");
            DistillResponse::Repos(repos)
        }
        DistillRequest::ListBranches { repo } => {
            if !acl.allows_repo(&repo) {
                return forbidden("无权访问指定仓库");
            }
            match state.index.git().list_branches(&repo).await {
                Ok(branches) => {
                    let filtered = branches
                        .into_iter()
                        .filter(|b| acl.allows_branch(&b.name))
                        .map(|mut b| {
                            b.indexed_cards = state.index.indexed_cards_for(&repo, &b.commit_sha);
                            b
                        })
                        .collect();
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "list_branches",
                        &repo,
                        "",
                        0,
                        "OK",
                    );
                    DistillResponse::Branches {
                        repo,
                        branches: filtered,
                    }
                }
                Err(e) => err_resp("BRANCH_LIST_FAILED", e),
            }
        }
        DistillRequest::FetchRepo { repo } => {
            if !acl.allows_repo(&repo) {
                return forbidden("无权访问指定仓库");
            }
            if !acl.can_fetch {
                append_audit(
                    &state.app_home,
                    peer_id,
                    &acl.label,
                    "fetch_repo",
                    &repo,
                    "",
                    0,
                    "FORBIDDEN_NO_FETCH_PERM",
                );
                return forbidden("当前客户端未授权触发远端 Fetch（服务端 can_fetch = false）");
            }
            match state.index.git().force_fetch(&repo).await {
                Ok((fetched, message)) => {
                    info!(
                        "client-driven fetch by '{}' ({}) on repo '{}': fetched={} msg='{}'",
                        acl.label, peer_id, repo, fetched, message
                    );
                    let branches = state
                        .index
                        .git()
                        .list_branches(&repo)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|b| acl.allows_branch(&b.name))
                        .map(|mut b| {
                            b.indexed_cards = state.index.indexed_cards_for(&repo, &b.commit_sha);
                            b
                        })
                        .collect();
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "fetch_repo",
                        &repo,
                        "",
                        0,
                        if fetched { "OK" } else { "SKIPPED" },
                    );
                    DistillResponse::FetchResult(FetchRepoResult {
                        repo,
                        fetched,
                        message,
                        branches,
                    })
                }
                Err(e) => {
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "fetch_repo",
                        &repo,
                        "",
                        0,
                        "ERROR",
                    );
                    err_resp("FETCH_FAILED", e)
                }
            }
        }
        DistillRequest::SearchSemantics {
            repo,
            branch,
            query,
            kind,
            limit,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问该仓库或分支");
            }
            match state
                .index
                .search_branch(&repo, &branch, &query, kind, limit.unwrap_or(10))
                .await
            {
                Ok(payload) => {
                    state
                        .total_dlp_redactions
                        .fetch_add(payload.dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "search_semantics",
                        &repo,
                        &branch,
                        payload.dlp_redactions,
                        "OK",
                    );
                    DistillResponse::SearchResult(payload)
                }
                Err(e) => err_resp("SEARCH_FAILED", e),
            }
        }
        DistillRequest::GetApiContracts {
            repo,
            branch,
            keyword,
            limit,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问该仓库或分支");
            }
            match state
                .index
                .search_branch(
                    &repo,
                    &branch,
                    &keyword,
                    Some(SemanticKind::ApiEndpoint),
                    limit.unwrap_or(10),
                )
                .await
            {
                Ok(payload) => {
                    state
                        .total_dlp_redactions
                        .fetch_add(payload.dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "get_api_contracts",
                        &repo,
                        &branch,
                        payload.dlp_redactions,
                        "OK",
                    );
                    DistillResponse::SearchResult(payload)
                }
                Err(e) => err_resp("API_CONTRACT_QUERY_FAILED", e),
            }
        }
        DistillRequest::GetDomainRules {
            repo,
            branch,
            topic,
            limit,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问该仓库或分支");
            }
            match state
                .index
                .search_branch(&repo, &branch, &topic, None, limit.unwrap_or(12))
                .await
            {
                Ok(payload) => {
                    state
                        .total_dlp_redactions
                        .fetch_add(payload.dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "get_domain_rules",
                        &repo,
                        &branch,
                        payload.dlp_redactions,
                        "OK",
                    );
                    DistillResponse::SearchResult(payload)
                }
                Err(e) => err_resp("DOMAIN_RULES_QUERY_FAILED", e),
            }
        }
        DistillRequest::DiffBranchBehavior {
            repo,
            base_branch,
            target_branch,
        } => {
            if !acl.allows_repo(&repo)
                || !acl.allows_branch(&base_branch)
                || !acl.allows_branch(&target_branch)
            {
                return forbidden("无权访问指定仓库或目标分支");
            }
            match state
                .index
                .diff_branch_behavior(&repo, &base_branch, &target_branch)
                .await
            {
                Ok(diff) => {
                    state
                        .total_dlp_redactions
                        .fetch_add(diff.dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "diff_branch_behavior",
                        &repo,
                        &format!("{base_branch}..{target_branch}"),
                        diff.dlp_redactions,
                        "OK",
                    );
                    DistillResponse::BranchDiff(diff)
                }
                Err(e) => err_resp("DIFF_FAILED", e),
            }
        }
        DistillRequest::RecentCommits {
            repo,
            branch,
            limit,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问指定仓库或分支");
            }
            match state
                .index
                .git()
                .recent_commits(&repo, &branch, limit.unwrap_or(15))
                .await
            {
                Ok(mut commits) => {
                    let mut dlp_redactions = 0usize;
                    for c in &mut commits {
                        let (s, r1) = DlpGate::sanitize_text(&c.subject, None);
                        let (b, r2) = DlpGate::sanitize_text(&c.body, None);
                        c.subject = s;
                        c.body = b;
                        dlp_redactions += r1 + r2;
                    }
                    state
                        .total_dlp_redactions
                        .fetch_add(dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "recent_commits",
                        &repo,
                        &branch,
                        dlp_redactions,
                        "OK",
                    );
                    DistillResponse::Commits {
                        repo,
                        branch,
                        commits,
                        dlp_redactions,
                    }
                }
                Err(e) => err_resp("RECENT_COMMITS_FAILED", e),
            }
        }
        DistillRequest::ListTree {
            repo,
            branch,
            path,
            max_depth,
            limit,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问指定仓库或分支");
            }
            match state
                .index
                .git()
                .list_tree(
                    &repo,
                    &branch,
                    path.as_deref(),
                    max_depth.unwrap_or(3),
                    limit.unwrap_or(200),
                )
                .await
            {
                Ok((commit_sha, norm_path, entries, truncated)) => {
                    append_audit_timed(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "list_tree",
                        &repo,
                        &branch,
                        0,
                        "OK",
                        Some(req_started.elapsed().as_micros() as u64),
                    );
                    DistillResponse::Tree {
                        repo,
                        branch,
                        commit_sha,
                        path: norm_path,
                        entries,
                        truncated,
                    }
                }
                Err(e) => err_resp("LIST_TREE_FAILED", e),
            }
        }
        DistillRequest::GrepRepo {
            repo,
            branch,
            pattern,
            path_prefix,
            case_insensitive,
            context_lines,
            limit,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问指定仓库或分支");
            }
            match state
                .index
                .git()
                .grep_repo(
                    &repo,
                    &branch,
                    &pattern,
                    path_prefix.as_deref(),
                    case_insensitive,
                    context_lines.unwrap_or(2),
                    limit.unwrap_or(40),
                )
                .await
            {
                Ok((commit_sha, matches, truncated, dlp_redactions)) => {
                    // Matches are already pre-sanitized inside `RawBlob` at snapshot hydration time!
                    if dlp_redactions > 0 {
                        state
                            .total_dlp_redactions
                            .fetch_add(dlp_redactions as u64, Ordering::Relaxed);
                    }
                    append_audit_timed(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "grep_repo",
                        &repo,
                        &branch,
                        dlp_redactions,
                        "OK",
                        Some(req_started.elapsed().as_micros() as u64),
                    );
                    DistillResponse::GrepResult {
                        repo,
                        branch,
                        commit_sha,
                        pattern,
                        matches,
                        truncated,
                        dlp_redactions,
                    }
                }
                Err(e) => err_resp("GREP_FAILED", e),
            }
        }
        DistillRequest::ReadFile {
            repo,
            branch,
            path,
            start_line,
            end_line,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问指定仓库或分支");
            }
            match state
                .index
                .git()
                .read_file_lines(&repo, &branch, &path, start_line, end_line)
                .await
            {
                Ok((commit_sha, norm_path, s, e, total, content, truncated, dlp_redactions)) => {
                    // Content is already pre-sanitized inside `RawBlob` (or sanitized in gix fallback).
                    if dlp_redactions > 0 {
                        state
                            .total_dlp_redactions
                            .fetch_add(dlp_redactions as u64, Ordering::Relaxed);
                    }
                    append_audit_timed(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "read_file",
                        &repo,
                        &branch,
                        dlp_redactions,
                        "OK",
                        Some(req_started.elapsed().as_micros() as u64),
                    );
                    DistillResponse::FileContent {
                        repo,
                        branch,
                        commit_sha,
                        path: norm_path,
                        start_line: s,
                        end_line: e,
                        total_lines: total,
                        content,
                        truncated,
                        dlp_redactions,
                    }
                }
                Err(e) => err_resp("READ_FILE_FAILED", e),
            }
        }
        DistillRequest::QueryLogs {
            query,
            range,
            start,
            end,
            limit,
            repo_hint,
        } => {
            if let Some(ref rh) = repo_hint
                && !acl.allows_repo(rh)
            {
                return forbidden("无权访问指定仓库的日志范围");
            }
            let cfg_snap = state.current_config();
            let default_filter = repo_hint.as_deref().and_then(|r| {
                cfg_snap
                    .repos
                    .iter()
                    .find(|x| x.name == r)
                    .and_then(|x| x.vlogs_filter.as_deref())
                    .or_else(|| match r.to_ascii_lowercase().as_str() {
                        "twosite" | "rigel" => Some("container:twosite-api OR container:rigel"),
                        _ => None,
                    })
            });
            match state
                .vlogs
                .query(
                    &query,
                    range.as_deref(),
                    start.as_deref(),
                    end.as_deref(),
                    limit,
                    default_filter,
                )
                .await
            {
                Ok(res) => {
                    state
                        .total_dlp_redactions
                        .fetch_add(res.dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "query_logs",
                        repo_hint.as_deref().unwrap_or(""),
                        &res.range,
                        res.dlp_redactions,
                        "OK",
                    );
                    DistillResponse::Logs(res)
                }
                Err(e) => err_resp("QUERY_LOGS_FAILED", e),
            }
        }
        DistillRequest::Investigate {
            repo,
            branch,
            question,
            include_logs,
            log_range,
        } => {
            if !acl.allows_repo(&repo) || !acl.allows_branch(&branch) {
                return forbidden("无权访问指定仓库或分支");
            }
            let cfg_snap = state.current_config();
            match investigate::run_investigation(
                &cfg_snap,
                &state.index,
                &state.vlogs,
                acl,
                &repo,
                &branch,
                &question,
                include_logs,
                log_range.as_deref(),
            )
            .await
            {
                Ok(bundle) => {
                    state
                        .total_dlp_redactions
                        .fetch_add(bundle.dlp_redactions as u64, Ordering::Relaxed);
                    append_audit(
                        &state.app_home,
                        peer_id,
                        &acl.label,
                        "investigate",
                        &repo,
                        &bundle.effective_branch,
                        bundle.dlp_redactions,
                        "OK",
                    );
                    DistillResponse::Investigation(bundle)
                }
                Err(e) => err_resp("INVESTIGATE_FAILED", e),
            }
        }
        DistillRequest::Metrics => {
            let m = metrics::collect_server_metrics(state).await;
            DistillResponse::Metrics(m)
        }
    }
}

fn forbidden(msg: &str) -> DistillResponse {
    DistillResponse::Error {
        code: "FORBIDDEN".into(),
        message: msg.into(),
    }
}

fn err_resp(code: &str, err: anyhow::Error) -> DistillResponse {
    DistillResponse::Error {
        code: code.into(),
        message: format!("{err}"),
    }
}

fn load_or_create_iroh_key(path: &Path) -> Result<SecretKey> {
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        let bytes = hex::decode(raw.trim()).context("invalid hex in server.key")?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("server.key must be 32 bytes"))?;
        return Ok(SecretKey::from_bytes(&arr));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let key = SecretKey::generate();
    fs::write(path, format!("{}\n", hex::encode(key.to_bytes())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(key)
}

struct AuditEvent {
    peer_id: String,
    label: String,
    action: &'static str,
    repo: String,
    branch: String,
    dlp_redactions: usize,
    status: &'static str,
    elapsed_us: Option<u64>,
}

impl AuditEvent {
    fn to_json_line(&self) -> String {
        let ts = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".into());
        let mut record = serde_json::json!({
            "ts": ts,
            "peer_id": self.peer_id,
            "label": self.label,
            "action": self.action,
            "repo": self.repo,
            "branch": self.branch,
            "dlp_redactions": self.dlp_redactions,
            "status": self.status,
        });
        if let Some(us) = self.elapsed_us
            && let Some(obj) = record.as_object_mut()
        {
            obj.insert("elapsed_us".into(), serde_json::json!(us));
        }
        record.to_string()
    }
}

static AUDIT_TX: OnceLock<std::sync::mpsc::Sender<AuditEvent>> = OnceLock::new();

fn init_async_audit_writer(audit_path: PathBuf) {
    let _ = AUDIT_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<AuditEvent>();
        let _ = std::thread::Builder::new()
            .name("distill-audit".into())
            .spawn(move || {
                let mut writer: Option<BufWriter<std::fs::File>> = None;
                while let Ok(first) = rx.recv() {
                    if writer.is_none() {
                        writer = OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&audit_path)
                            .ok()
                            .map(BufWriter::new);
                    }
                    if let Some(ref mut w) = writer {
                        let _ = writeln!(w, "{}", first.to_json_line());
                        while let Ok(next) = rx.try_recv() {
                            let _ = writeln!(w, "{}", next.to_json_line());
                        }
                        if w.flush().is_err() {
                            writer = None;
                        }
                    }
                }
            });
        tx
    });
}

#[allow(clippy::too_many_arguments)]
fn append_audit(
    app_home: &Path,
    peer_id: &str,
    label: &str,
    action: &'static str,
    repo: &str,
    branch: &str,
    dlp_redactions: usize,
    status: &'static str,
) {
    append_audit_timed(
        app_home,
        peer_id,
        label,
        action,
        repo,
        branch,
        dlp_redactions,
        status,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
fn append_audit_timed(
    app_home: &Path,
    peer_id: &str,
    label: &str,
    action: &'static str,
    repo: &str,
    branch: &str,
    dlp_redactions: usize,
    status: &'static str,
    elapsed_us: Option<u64>,
) {
    let event = AuditEvent {
        peer_id: peer_id.to_string(),
        label: label.to_string(),
        action,
        repo: repo.to_string(),
        branch: branch.to_string(),
        dlp_redactions,
        status,
        elapsed_us,
    };
    if let Some(tx) = AUDIT_TX.get()
        && tx.send(event).is_ok()
    {
        return;
    }
    // Fallback if called outside `cmd_serve`
    let fallback = AuditEvent {
        peer_id: peer_id.to_string(),
        label: label.to_string(),
        action,
        repo: repo.to_string(),
        branch: branch.to_string(),
        dlp_redactions,
        status,
        elapsed_us,
    };
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(app_home.join("audit.jsonl"))
    {
        let _ = writeln!(file, "{}", fallback.to_json_line());
    }
}
