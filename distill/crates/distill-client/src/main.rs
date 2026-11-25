mod ai;
mod clipboard;
mod markdown;
mod mcp;
mod tui;

use ai::{AI_PRESETS, AiFileConfig, LocalAiAgent, ResolvedAiConfig, find_preset, preset_readiness};
use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use std::io::{self, Write};
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey};
use distill_proto::{
    CLIENT_APP_NAME, MAX_FRAME_BYTES, DISTILL_ALPN, DistillRequest, DistillResponse, SemanticKind,
    decode_payload, encode_frame, resolve_app_home,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(
    name = "distill",
    version,
    about = "Pure Rust P2P (iroh QUIC) interactive TUI, CLI & MCP bridge for Distill"
)]
struct Cli {
    /// Client home directory (defaults to $HOME/.distill-client or $DISTILL_CLIENT_HOME)
    #[arg(long, global = true)]
    home: Option<PathBuf>,

    /// Server EndpointId (overrides client.toml)
    #[arg(long, global = true, env = "DISTILL_SERVER_ID")]
    server_id: Option<String>,

    /// Optional direct IP:PORT hint for instant LAN connection (e.g. 10.254.144.213:12345)
    #[arg(long, global = true, env = "DISTILL_DIRECT_ADDR")]
    direct_addr: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch interactive conversational TUI with remote status & branch sidebar (default)
    Tui,
    /// Generate persistent Ed25519 client identity and print EndpointId (send to admin to whitelist)
    PrintId,
    /// Save default server connection settings to $HOME/.distill-client/client.toml
    Configure {
        #[arg(long)]
        server_id: String,
        #[arg(long)]
        direct_addr: Option<String>,
    },
    /// Manage local AI provider (pi-style: `distill ai`, `distill ai list`, `distill ai set deepseek ...`)
    Ai {
        #[command(subcommand)]
        action: Option<AiAction>,
    },
    /// Alias for configuring local AI provider directly via flags
    ConfigureAi {
        #[arg(long, default_value = "codex")]
        provider: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        cli_path: Option<String>,
    },
    /// Ask a natural-language question using local AI + service-side Raw Git & VictoriaLogs investigation
    Ask {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        /// Natural-language question
        question: String,
        /// Force including VictoriaLogs runtime log query alongside raw Git code
        #[arg(long, default_value_t = false, conflicts_with = "no_logs")]
        with_logs: bool,
        /// Disable VictoriaLogs log query (raw Git code only)
        #[arg(long, default_value_t = false)]
        no_logs: bool,
        /// Override VictoriaLogs time range (e.g. 5m, 15m, 1h, 6h, 24h)
        #[arg(long)]
        log_range: Option<String>,
        /// Also copy final AI conclusion to clipboard
        #[arg(long, default_value_t = false)]
        copy: bool,
    },
    /// Query the solidified VictoriaLogs endpoint directly via service side (LogsQL)
    Vlogs {
        /// LogsQL query expression (e.g. `container:twosite-api error` or a traceId)
        #[arg(default_value = "*")]
        query: String,
        /// Relative time range (e.g. 5m, 15m, 1h, 6h, 24h; default 15m)
        #[arg(long)]
        range: Option<String>,
        /// Optional RFC3339 start timestamp
        #[arg(long)]
        start: Option<String>,
        /// Optional RFC3339 end timestamp
        #[arg(long)]
        end: Option<String>,
        /// Max log entries to return (default 50, max 200)
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Optional repo hint for default container/namespace scoping (e.g. twosite)
        #[arg(long)]
        repo: Option<String>,
        /// Print compact human-readable log lines instead of JSON
        #[arg(long, default_value_t = false)]
        raw: bool,
    },
    /// Run service-side unified Raw Git + optional VictoriaLogs investigation and print raw bundle JSON
    Investigate {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        /// Natural-language question
        question: String,
        #[arg(long, default_value_t = false, conflicts_with = "no_logs")]
        with_logs: bool,
        #[arg(long, default_value_t = false)]
        no_logs: bool,
        #[arg(long)]
        log_range: Option<String>,
    },
    /// Ping the remote server over encrypted iroh QUIC
    Ping,
    /// Fetch remote server status summary (uptime, cached indexes, worktrees, sync state)
    Status,
    /// Fetch remote server /metrics (repo disk usage, active worktrees & branches, LRU GC & thresholds)
    Metrics {
        /// Print human-readable summary table instead of JSON
        #[arg(long, default_value_t = false)]
        raw: bool,
    },
    /// List authorized repositories
    Repos,
    /// List authorized branches for a repository
    Branches {
        #[arg(long)]
        repo: String,
    },
    /// Trigger remote git fetch for a repository and list updated branches
    Fetch {
        #[arg(long)]
        repo: String,
    },
    /// Search sanitized semantic index on a specific repo + branch
    Search {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        query: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// View recent sanitized commits for a repo + branch (no raw source diffs)
    Log {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        #[arg(long, default_value_t = 15)]
        limit: usize,
    },
    /// List raw directory tree of any branch (fast, zero checkout, any language)
    Tree {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 3)]
        depth: usize,
        #[arg(long, default_value_t = 200)]
        limit: usize,
    },
    /// Fast raw `git grep` across any branch & any file (secrets redacted)
    Grep {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        /// Regex or fixed-string search pattern
        pattern: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(short = 'i', long, default_value_t = true)]
        ignore_case: bool,
        #[arg(short = 'C', long, default_value_t = 2)]
        context: usize,
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// Read any text file directly from a branch (secrets redacted)
    Read {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        branch: String,
        /// Relative file path inside the repository
        path: String,
        #[arg(long)]
        start: Option<usize>,
        #[arg(long)]
        end: Option<usize>,
        /// Print raw numbered file content directly instead of JSON wrapper
        #[arg(long, default_value_t = false)]
        raw: bool,
    },
    /// Compare semantic behavior differences between two branches
    Diff {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        base: String,
        #[arg(long)]
        target: String,
    },
    /// Run as a local MCP (Model Context Protocol) stdio server for Antigravity / Claude Code / Cursor
    Mcp,
}

#[derive(Subcommand)]
enum AiAction {
    /// Interactive setup wizard (select from built-in AI providers, model & API key)
    Setup,
    /// List all supported AI provider presets and current active AI
    List,
    /// Switch to a provider preset directly (e.g. `distill ai set deepseek --api-key sk-...`)
    Set {
        /// Provider preset: codex / claude / openai / anthropic / deepseek / gemini / openrouter / qwen / moonshot / zhipu / ollama / custom
        provider: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        api_key_env: Option<String>,
        #[arg(long)]
        cli_path: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ClientConfig {
    server_id: Option<String>,
    direct_addr: Option<String>,
    ai: Option<AiFileConfig>,
}

pub(crate) struct P2pClient {
    endpoint: Endpoint,
    target: EndpointAddr,
    conn: Mutex<Option<iroh::endpoint::Connection>>,
}

impl P2pClient {
    async fn new(key: SecretKey, target: EndpointAddr) -> Result<Self> {
        let endpoint = Endpoint::builder(presets::N0)
            .secret_key(key)
            .bind()
            .await
            .context("failed to bind local iroh client endpoint")?;
        Ok(Self {
            endpoint,
            target,
            conn: Mutex::new(None),
        })
    }

    pub(crate) async fn call(&self, request: DistillRequest) -> Result<DistillResponse> {
        let conn = {
            let mut guard = self.conn.lock().await;
            if let Some(existing) = guard.as_ref()
                && existing.close_reason().is_none()
            {
                existing.clone()
            } else {
                let fresh = self
                    .endpoint
                    .connect(self.target.clone(), DISTILL_ALPN)
                    .await
                    .context("failed to connect to Distill server over iroh QUIC")?;
                *guard = Some(fresh.clone());
                fresh
            }
        };

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .context("failed to open QUIC bidirectional stream")?;
        let frame = encode_frame(&request)?;
        send.write_all(&frame).await?;
        send.finish()?;

        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(anyhow!("server response exceeds max frame size"));
        }
        let mut payload = vec![0u8; len];
        recv.read_exact(&mut payload).await?;
        Ok(decode_payload(&payload)?)
    }

    async fn close(&self) {
        self.endpoint.close().await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let app_home = cli
        .home
        .unwrap_or_else(|| resolve_app_home(CLIENT_APP_NAME, "DISTILL_CLIENT_HOME"));
    fs::create_dir_all(&app_home)?;

    let key = load_or_create_client_key(&app_home.join("client.key"))?;

    let command = cli.command.unwrap_or(Commands::Tui);

    let cfg_path = app_home.join("client.toml");

    match &command {
        Commands::PrintId => {
            println!("{}", key.public());
            return Ok(());
        }
        Commands::Configure {
            server_id,
            direct_addr,
        } => {
            let mut cfg = load_client_config(&cfg_path);
            cfg.server_id = Some(server_id.clone());
            cfg.direct_addr = direct_addr.clone();
            fs::write(&cfg_path, toml::to_string_pretty(&cfg)?)?;
            println!("Saved client config to {}", cfg_path.display());
            println!("Your client EndpointId (send to admin to whitelist): {}", key.public());
            return Ok(());
        }
        Commands::Ai { action } => {
            match action {
                None | Some(AiAction::Setup) => run_ai_setup_wizard(&cfg_path)?,
                Some(AiAction::List) => print_ai_providers(&cfg_path),
                Some(AiAction::Set {
                    provider,
                    model,
                    base_url,
                    api_key,
                    api_key_env,
                    cli_path,
                }) => {
                    apply_ai_preset_cli(
                        &cfg_path,
                        provider,
                        model.clone(),
                        base_url.clone(),
                        api_key.clone(),
                        api_key_env.clone(),
                        cli_path.clone(),
                    )?;
                }
            }
            return Ok(());
        }
        Commands::ConfigureAi {
            provider,
            base_url,
            model,
            api_key,
            api_key_env,
            cli_path,
        } => {
            apply_ai_preset_cli(
                &cfg_path,
                provider,
                model.clone(),
                base_url.clone(),
                api_key.clone(),
                api_key_env.clone(),
                cli_path.clone(),
            )?;
            return Ok(());
        }
        _ => {}
    }

    let file_cfg = load_client_config(&cfg_path);
    let server_id_str = cli
        .server_id
        .or(file_cfg.server_id)
        .ok_or_else(|| anyhow!("missing server_id: pass --server-id or run `distill configure`"))?;
    let direct_str = cli.direct_addr.or(file_cfg.direct_addr);

    let ai_agent = ResolvedAiConfig::discover(file_cfg.ai.as_ref())
        .and_then(|c| LocalAiAgent::new(c).ok())
        .map(Arc::new);

    let endpoint_id = EndpointId::from_str(server_id_str.trim())
        .with_context(|| format!("invalid iroh EndpointId: {server_id_str}"))?;
    let mut target = EndpointAddr::new(endpoint_id);
    let server_hint = if let Some(ref addr_str) = direct_str {
        let sock: SocketAddr = addr_str
            .trim()
            .parse()
            .with_context(|| format!("invalid direct_addr SocketAddr: {addr_str}"))?;
        target = target.with_ip_addr(sock);
        format!("{} ({}…)", addr_str.trim(), &server_id_str[..server_id_str.len().min(8)])
    } else {
        format!("{}…", &server_id_str[..server_id_str.len().min(12)])
    };

    let client = Arc::new(P2pClient::new(key, target).await?);

    match command {
        Commands::Tui => {
            tui::run_tui(Arc::clone(&client), ai_agent, cfg_path.clone(), server_hint).await?;
        }
        Commands::Ask {
            repo,
            branch,
            question,
            with_logs,
            no_logs,
            log_range,
            copy,
        } => {
            let ai = ai_agent.ok_or_else(|| {
                anyhow!("未检测到本地 AI 配置：请设置 OPENAI_API_KEY / DEEPSEEK_API_KEY / ANTHROPIC_API_KEY，或运行 `distill ai`")
            })?;
            let include_logs = if with_logs {
                Some(true)
            } else if no_logs {
                Some(false)
            } else {
                None
            };
            let ans = ai
                .ask(
                    &client,
                    &repo,
                    &branch,
                    "",
                    &question,
                    include_logs,
                    log_range,
                    &[],
                    |p| {
                        eprintln!("[distill-ai] {p}");
                    },
                )
                .await?;
            println!("{}", ans.content);
            eprintln!(
                "\n--- AI={} | 整合 {} 条证据 ({}) | DLP 脱敏 {} 处 | {:.1}s ---",
                ans.model,
                ans.cards_consulted,
                ans.tool_calls.join(", "),
                ans.dlp_redactions,
                ans.elapsed_ms as f64 / 1000.0
            );
            if copy {
                match clipboard::copy_to_clipboard(&ans.content) {
                    Ok(dest) => eprintln!("已复制到{dest}"),
                    Err(e) => eprintln!("复制失败: {e}"),
                }
            }
        }
        Commands::Vlogs {
            query,
            range,
            start,
            end,
            limit,
            repo,
            raw,
        } => {
            let resp = client
                .call(DistillRequest::QueryLogs {
                    query,
                    range,
                    start,
                    end,
                    limit: Some(limit),
                    repo_hint: repo,
                })
                .await?;
            if raw {
                if let DistillResponse::Logs(logs) = &resp {
                    eprintln!(
                        "# VictoriaLogs {} | range={} | query=`{}` | hits={} | dlp={}",
                        logs.endpoint,
                        logs.range,
                        logs.effective_query,
                        logs.entries.len(),
                        logs.dlp_redactions
                    );
                    for entry in &logs.entries {
                        let scope = match (entry.namespace.is_empty(), entry.container.is_empty()) {
                            (false, false) => format!("{}/{}", entry.namespace, entry.container),
                            (true, false) => entry.container.clone(),
                            (false, true) => entry.namespace.clone(),
                            (true, true) => {
                                if !entry.stream.is_empty() {
                                    entry.stream.clone()
                                } else {
                                    "-".to_string()
                                }
                            }
                        };
                        println!("[{}] ({}) {}", entry.timestamp, scope, entry.message);
                    }
                } else {
                    print_resp(&resp)?;
                }
            } else {
                print_resp(&resp)?;
            }
        }
        Commands::Investigate {
            repo,
            branch,
            question,
            with_logs,
            no_logs,
            log_range,
        } => {
            let include_logs = if with_logs {
                Some(true)
            } else if no_logs {
                Some(false)
            } else {
                None
            };
            let resp = client
                .call(DistillRequest::Investigate {
                    repo,
                    branch,
                    question,
                    include_logs,
                    log_range,
                })
                .await?;
            print_resp(&resp)?;
        }
        Commands::Mcp => {
            let c = Arc::clone(&client);
            mcp::run_mcp_stdio(move |req| {
                let c = Arc::clone(&c);
                async move { c.call(req).await }
            })
            .await?;
        }
        Commands::Ping => {
            print_resp(&client.call(DistillRequest::Ping).await?)?;
        }
        Commands::Status => {
            print_resp(&client.call(DistillRequest::ServerStatus).await?)?;
        }
        Commands::Metrics { raw } => {
            let resp = client.call(DistillRequest::Metrics).await?;
            if raw {
                if let DistillResponse::Metrics(m) = &resp {
                    println!("{}", format_metrics_text(m));
                } else {
                    print_resp(&resp)?;
                }
            } else {
                print_resp(&resp)?;
            }
        }
        Commands::Repos => {
            print_resp(&client.call(DistillRequest::ListRepos).await?)?;
        }
        Commands::Branches { repo } => {
            print_resp(&client.call(DistillRequest::ListBranches { repo }).await?)?;
        }
        Commands::Fetch { repo } => {
            print_resp(&client.call(DistillRequest::FetchRepo { repo }).await?)?;
        }
        Commands::Search {
            repo,
            branch,
            query,
            kind,
            limit,
        } => {
            let resp = client
                .call(DistillRequest::SearchSemantics {
                    repo,
                    branch,
                    query,
                    kind: kind.as_deref().and_then(SemanticKind::from_str_opt),
                    limit: Some(limit),
                })
                .await?;
            print_resp(&resp)?;
        }
        Commands::Log {
            repo,
            branch,
            limit,
        } => {
            let resp = client
                .call(DistillRequest::RecentCommits {
                    repo,
                    branch,
                    limit: Some(limit),
                })
                .await?;
            print_resp(&resp)?;
        }
        Commands::Tree {
            repo,
            branch,
            path,
            depth,
            limit,
        } => {
            let resp = client
                .call(DistillRequest::ListTree {
                    repo,
                    branch,
                    path,
                    max_depth: Some(depth),
                    limit: Some(limit),
                })
                .await?;
            print_resp(&resp)?;
        }
        Commands::Grep {
            repo,
            branch,
            pattern,
            path,
            ignore_case,
            context,
            limit,
        } => {
            let resp = client
                .call(DistillRequest::GrepRepo {
                    repo,
                    branch,
                    pattern,
                    path_prefix: path,
                    case_insensitive: ignore_case,
                    context_lines: Some(context),
                    limit: Some(limit),
                })
                .await?;
            print_resp(&resp)?;
        }
        Commands::Read {
            repo,
            branch,
            path,
            start,
            end,
            raw,
        } => {
            let resp = client
                .call(DistillRequest::ReadFile {
                    repo,
                    branch,
                    path,
                    start_line: start,
                    end_line: end,
                })
                .await?;
            if raw {
                if let DistillResponse::FileContent { content, .. } = &resp {
                    print!("{content}");
                } else {
                    print_resp(&resp)?;
                }
            } else {
                print_resp(&resp)?;
            }
        }
        Commands::Diff { repo, base, target } => {
            let resp = client
                .call(DistillRequest::DiffBranchBehavior {
                    repo,
                    base_branch: base,
                    target_branch: target,
                })
                .await?;
            print_resp(&resp)?;
        }
        Commands::PrintId
        | Commands::Configure { .. }
        | Commands::Ai { .. }
        | Commands::ConfigureAi { .. } => {
            unreachable!()
        }
    }

    client.close().await;
    Ok(())
}

pub(crate) fn save_ai_config(cfg_path: &Path, ai_cfg: AiFileConfig) -> Result<()> {
    let mut cfg = load_client_config(cfg_path);
    cfg.ai = Some(ai_cfg);
    fs::write(cfg_path, toml::to_string_pretty(&cfg)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(cfg_path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn print_ai_providers(cfg_path: &Path) {
    let file_cfg = load_client_config(cfg_path);
    if let Some(active) = ResolvedAiConfig::discover(file_cfg.ai.as_ref()) {
        println!(
            "当前生效本地 AI: {} [{}] {}",
            active.model,
            active.source_label,
            if active.base_url.is_empty() {
                "(local CLI)".to_string()
            } else {
                active.base_url
            }
        );
    } else {
        println!("当前未配置本地 AI（运行 `distill ai` 进行交互式配置）");
    }
    println!();
    println!("{:<12} {:<24} {:<26} {}", "ID", "Provider", "默认模型", "状态");
    println!("{}", "-".repeat(78));
    for p in AI_PRESETS {
        let (status, ok) = preset_readiness(p);
        let mark = if ok { "✓" } else { "·" };
        println!(
            "{:<12} {:<24} {:<26} {} {}",
            p.id, p.name, p.default_model, mark, status
        );
    }
    println!();
    println!("快速切换示例：");
    println!("  distill ai                         # 交互式向导（类似 pi /model）");
    println!("  distill ai set codex               # 复用本机 Codex CLI");
    println!("  distill ai set deepseek --api-key sk-xxx");
    println!("  distill ai set openai --model gpt-4o-mini --api-key-env OPENAI_API_KEY");
    println!("  distill ai set ollama --model qwen2.5:14b");
}

fn apply_ai_preset_cli(
    cfg_path: &Path,
    provider: &str,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    api_key_env: Option<String>,
    cli_path: Option<String>,
) -> Result<()> {
    let preset = find_preset(provider);
    let ai_cfg = AiFileConfig {
        provider: Some(provider.trim().to_ascii_lowercase()),
        base_url: base_url.or_else(|| {
            preset
                .filter(|p| !p.default_base_url.is_empty())
                .map(|p| p.default_base_url.to_string())
        }),
        model: model.or_else(|| {
            preset
                .filter(|p| !p.default_model.is_empty())
                .map(|p| p.default_model.to_string())
        }),
        api_key,
        api_key_env: api_key_env.or_else(|| {
            preset
                .filter(|p| !p.default_key_env.is_empty())
                .map(|p| p.default_key_env.to_string())
        }),
        cli_path,
    };
    save_ai_config(cfg_path, ai_cfg.clone())?;
    println!("已保存本地 AI 配置到 {}", cfg_path.display());
    match ResolvedAiConfig::discover(Some(&ai_cfg)) {
        Some(resolved) => println!("当前生效 AI: {}", resolved.display_badge()),
        None => println!(
            "提示：已保存预设 `{provider}`，但尚未检测到可用的 API Key 或本机 CLI（可通过 `--api-key` 或对应环境变量提供）。"
        ),
    }
    Ok(())
}

fn run_ai_setup_wizard(cfg_path: &Path) -> Result<()> {
    println!("=== Distill 本地 AI 配置向导 ===");
    print_ai_providers(cfg_path);
    println!("请输入序号 (1-{}) 或 Provider ID（直接回车取消）：", AI_PRESETS.len());
    for (i, p) in AI_PRESETS.iter().enumerate() {
        let (st, ok) = preset_readiness(p);
        println!(
            "  {:>2}. {:<11} {} [{} {}]",
            i + 1,
            p.id,
            p.name,
            if ok { "✓" } else { "·" },
            st
        );
    }

    let choice = prompt_line("选择 Provider")?;
    if choice.is_empty() {
        println!("已取消。");
        return Ok(());
    }
    let preset = if let Ok(idx) = choice.parse::<usize>()
        && idx >= 1
        && idx <= AI_PRESETS.len()
    {
        &AI_PRESETS[idx - 1]
    } else if let Some(p) = find_preset(&choice) {
        p
    } else {
        return Err(anyhow!("未找到 Provider `{choice}`"));
    };

    println!("\n已选择：{} ({})", preset.name, preset.desc);
    let model_input = prompt_line(&format!("模型名 [默认: {}]", preset.default_model))?;
    let model = if model_input.is_empty() {
        Some(preset.default_model.to_string())
    } else {
        Some(model_input)
    };

    let (base_url, api_key, api_key_env) = if preset.wire == "codex" || preset.wire == "claude" {
        (None, None, None)
    } else {
        let url_input = prompt_line(&format!("Base URL [默认: {}]", preset.default_base_url))?;
        let base_url = if url_input.is_empty() {
            Some(preset.default_base_url.to_string())
        } else {
            Some(url_input)
        };
        if preset.needs_api_key {
            let env_hint = if preset.default_key_env.is_empty() {
                "留空不写入".to_string()
            } else {
                format!("留空则自动读 ${}", preset.default_key_env)
            };
            let key_input = prompt_line(&format!("API Key ({env_hint})"))?;
            let api_key = (!key_input.is_empty()).then_some(key_input);
            let api_key_env = (!preset.default_key_env.is_empty())
                .then(|| preset.default_key_env.to_string());
            (base_url, api_key, api_key_env)
        } else {
            (base_url, None, None)
        }
    };

    apply_ai_preset_cli(
        cfg_path,
        preset.id,
        model,
        base_url,
        api_key,
        api_key_env,
        None,
    )
}

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_string())
}

pub(crate) fn format_bytes_human(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.2} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.1} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

pub(crate) fn format_metrics_text(m: &distill_proto::ServerMetrics) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "distilld v{} | 运行 {}s | HTTP /metrics: {}\n",
        m.server_version,
        m.uptime_secs,
        m.metrics_listen.as_deref().unwrap_or("已关闭")
    ));
    out.push_str(&format!(
        "总请求: {} | DLP脱敏: {} | 配置热重载: {} 次 | GC清理: {} 次\n",
        m.total_requests, m.total_dlp_redactions, m.config_reloads_total, m.gc_evictions_total
    ));
    out.push_str(&format!(
        "阈值配置: worktree≤{} | 分支索引≤{} | 内存快照≤{} ({} MiB) | Rayon {} 核 | 闲置TTL={}s | GC周期={}s | 磁盘预警={} MiB | 热加载={}s\n",
        m.max_worktrees,
        m.max_cached_indexes,
        m.max_memory_snapshots,
        m.max_snapshot_memory_mb,
        m.rayon_threads,
        m.worktree_idle_ttl_secs,
        m.gc_interval_secs,
        m.disk_warn_threshold_mb,
        m.config_reload_interval_secs
    ));
    out.push_str(&format!(
        "内存快照: {}/{} 个 | 去重Blob {} 个 | 占用 {} / {} MiB\n",
        m.memory_snapshots.len(),
        m.max_memory_snapshots,
        m.snapshot_blobs_cached,
        format_bytes_human(m.snapshot_memory_bytes),
        m.max_snapshot_memory_mb
    ));
    out.push_str(&format!(
        "磁盘总占用: {} {} (裸仓 {} | Worktrees {} | 索引 {} | Blob缓存 {} | 审计日志 {})\n",
        format_bytes_human(m.total_disk_bytes),
        if m.disk_over_threshold { "[⚠ 超出告警阈值]" } else { "[正常]" },
        format_bytes_human(m.repos_disk_bytes),
        format_bytes_human(m.worktrees_disk_bytes),
        format_bytes_human(m.indexes_disk_bytes),
        format_bytes_human(m.blob_cache_disk_bytes),
        format_bytes_human(m.audit_log_bytes),
    ));

    out.push_str("\n[仓库磁盘与分支统计]\n");
    for r in &m.repos {
        let active = if r.active_branches.is_empty() {
            "-".to_string()
        } else {
            r.active_branches.join(", ")
        };
        out.push_str(&format!(
            "  • {} ({}) | 裸仓 {} | 分支数 {} | 活跃Worktree {}/{} ({}) | 索引缓存 {}/{} ({}) | 活跃分支: {}\n",
            r.name,
            r.mode,
            format_bytes_human(r.repo_disk_bytes),
            r.branches_count,
            r.worktrees_count,
            m.max_worktrees,
            format_bytes_human(r.worktrees_disk_bytes),
            r.cached_indexes_count,
            m.max_cached_indexes,
            format_bytes_human(r.indexes_disk_bytes),
            active
        ));
    }

    if !m.worktrees.is_empty() {
        out.push_str("\n[活跃 Worktree 明细]\n");
        for w in &m.worktrees {
            let sha = &w.commit_sha[..w.commit_sha.len().min(10)];
            out.push_str(&format!(
                "  • {}@{} ({sha}) | 占用 {} | 闲置 {}s\n",
                w.repo,
                if w.branch.is_empty() { "detached" } else { &w.branch },
                format_bytes_human(w.disk_bytes),
                w.idle_secs
            ));
        }
    }

    if !m.cached_indexes.is_empty() {
        out.push_str("\n[已缓存分支索引明细]\n");
        for i in &m.cached_indexes {
            let sha = &i.commit_sha[..i.commit_sha.len().min(10)];
            out.push_str(&format!(
                "  • {}@{} ({sha}) | {} 卡 | 占用 {} | 闲置 {}s\n",
                i.repo,
                if i.branch.is_empty() { "-" } else { &i.branch },
                i.total_cards,
                format_bytes_human(i.disk_bytes),
                i.idle_secs
            ));
        }
    }

    if !m.memory_snapshots.is_empty() {
        out.push_str("\n[内存态 Raw 结构化快照明细]\n");
        for s in &m.memory_snapshots {
            let sha = &s.commit_sha[..s.commit_sha.len().min(10)];
            out.push_str(&format!(
                "  • {}@{} ({sha}) | {} 文本文件 / {} 目录项 | 闲置 {}s\n",
                s.repo,
                if s.branch.is_empty() { "-" } else { &s.branch },
                s.files_count,
                s.tree_entries_count,
                s.idle_secs
            ));
        }
    }

    out.trim_end().to_string()
}

fn print_resp(resp: &DistillResponse) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(resp)?);
    Ok(())
}

fn load_client_config(path: &Path) -> ClientConfig {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_or_create_client_key(path: &Path) -> Result<SecretKey> {
    if path.exists() {
        let raw = fs::read_to_string(path)?;
        let bytes = hex::decode(raw.trim()).context("invalid hex in client.key")?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow!("client.key must be 32 bytes"))?;
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
