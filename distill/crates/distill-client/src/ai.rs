use crate::P2pClient;
use anyhow::{Context, Result, anyhow};
use distill_proto::{
    DistillRequest, DistillResponse, InvestigationBundle, LogsQueryResult, SemanticCard,
    SemanticKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Copy)]
pub struct AiPreset {
    pub id: &'static str,
    pub name: &'static str,
    /// Wire protocol: "codex" | "claude" | "openai" | "anthropic"
    pub wire: &'static str,
    pub default_base_url: &'static str,
    pub default_model: &'static str,
    pub default_key_env: &'static str,
    pub needs_api_key: bool,
    pub desc: &'static str,
}

pub const AI_PRESETS: &[AiPreset] = &[
    AiPreset {
        id: "codex",
        name: "Codex CLI (本机已登录)",
        wire: "codex",
        default_base_url: "",
        default_model: "codex-local",
        default_key_env: "",
        needs_api_key: false,
        desc: "复用本机 codex 登录态，免配 API Key",
    },
    AiPreset {
        id: "claude",
        name: "Claude Code CLI (本机已登录)",
        wire: "claude",
        default_base_url: "",
        default_model: "claude-local",
        default_key_env: "",
        needs_api_key: false,
        desc: "复用本机 claude 登录态，免配 API Key",
    },
    AiPreset {
        id: "openai",
        name: "OpenAI",
        wire: "openai",
        default_base_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini",
        default_key_env: "OPENAI_API_KEY",
        needs_api_key: true,
        desc: "GPT-4o / GPT-4.1 / o4-mini 官方接口",
    },
    AiPreset {
        id: "anthropic",
        name: "Anthropic Claude",
        wire: "anthropic",
        default_base_url: "https://api.anthropic.com",
        default_model: "claude-sonnet-4-20250514",
        default_key_env: "ANTHROPIC_API_KEY",
        needs_api_key: true,
        desc: "Claude Sonnet / Haiku / Opus 官方接口",
    },
    AiPreset {
        id: "deepseek",
        name: "DeepSeek",
        wire: "openai",
        default_base_url: "https://api.deepseek.com",
        default_model: "deepseek-chat",
        default_key_env: "DEEPSEEK_API_KEY",
        needs_api_key: true,
        desc: "DeepSeek-V3 / Reasoner 官方接口",
    },
    AiPreset {
        id: "gemini",
        name: "Google Gemini",
        wire: "openai",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        default_model: "gemini-2.5-flash",
        default_key_env: "GEMINI_API_KEY",
        needs_api_key: true,
        desc: "Gemini 2.5 Flash / Pro OpenAI 兼容接口",
    },
    AiPreset {
        id: "openrouter",
        name: "OpenRouter",
        wire: "openai",
        default_base_url: "https://openrouter.ai/api/v1",
        default_model: "anthropic/claude-sonnet-4",
        default_key_env: "OPENROUTER_API_KEY",
        needs_api_key: true,
        desc: "一站式聚合 Claude / GPT / DeepSeek / Gemini",
    },
    AiPreset {
        id: "qwen",
        name: "通义千问 (DashScope)",
        wire: "openai",
        default_base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen-plus",
        default_key_env: "DASHSCOPE_API_KEY",
        needs_api_key: true,
        desc: "阿里云百炼 qwen-plus / qwen-max / qwen3",
    },
    AiPreset {
        id: "moonshot",
        name: "Kimi (Moonshot)",
        wire: "openai",
        default_base_url: "https://api.moonshot.cn/v1",
        default_model: "kimi-k2-0711-preview",
        default_key_env: "MOONSHOT_API_KEY",
        needs_api_key: true,
        desc: "月之暗面 Kimi / Moonshot 接口",
    },
    AiPreset {
        id: "zhipu",
        name: "智谱 GLM",
        wire: "openai",
        default_base_url: "https://open.bigmodel.cn/api/paas/v4",
        default_model: "glm-4-plus",
        default_key_env: "ZHIPU_API_KEY",
        needs_api_key: true,
        desc: "智谱 BigModel GLM-4 系列接口",
    },
    AiPreset {
        id: "ollama",
        name: "Ollama (本机离线模型)",
        wire: "openai",
        default_base_url: "http://127.0.0.1:11434/v1",
        default_model: "qwen2.5:14b",
        default_key_env: "",
        needs_api_key: false,
        desc: "纯本地运行，数据不出本机，免 API Key",
    },
    AiPreset {
        id: "custom",
        name: "自定义 OpenAI 兼容网关",
        wire: "openai",
        default_base_url: "http://127.0.0.1:8000/v1",
        default_model: "gpt-4o-mini",
        default_key_env: "DISTILL_AI_API_KEY",
        needs_api_key: true,
        desc: "企业内部代理 / vLLM / LiteLLM / OneAPI",
    },
];

pub fn find_preset(id: &str) -> Option<&'static AiPreset> {
    let lower = id.trim().to_ascii_lowercase();
    AI_PRESETS.iter().find(|p| p.id == lower)
}

pub fn preset_readiness(preset: &AiPreset) -> (String, bool) {
    match preset.wire {
        "codex" => {
            if find_cli_binary("codex").is_some() {
                ("本机已安装".into(), true)
            } else {
                ("未检测到 codex".into(), false)
            }
        }
        "claude" => {
            if find_cli_binary("claude").is_some() {
                ("本机已安装".into(), true)
            } else {
                ("未检测到 claude".into(), false)
            }
        }
        _ if !preset.needs_api_key => ("本机免 Key".into(), true),
        _ => {
            if !preset.default_key_env.is_empty() && non_empty_env(preset.default_key_env).is_ok() {
                (format!("${} 已就绪", preset.default_key_env), true)
            } else {
                ("需配置 Key".into(), false)
            }
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiFileConfig {
    /// Preset or wire id: "codex" | "claude" | "openai" | "anthropic" | "deepseek" | "gemini" | "openrouter" | "qwen" | "moonshot" | "zhipu" | "ollama" | "custom"
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    /// Optional env var name holding the API key (so key doesn't need to be written into TOML)
    pub api_key_env: Option<String>,
    /// Optional explicit binary path for local AI CLIs ("codex" / "claude")
    pub cli_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiProvider {
    OpenAi,
    Anthropic,
    CodexCli,
    ClaudeCli,
}

#[derive(Debug, Clone)]
pub struct ResolvedAiConfig {
    pub provider: AiProvider,
    pub base_url: String,
    pub model: String,
    pub fallback_model: Option<String>,
    pub api_key: String,
    pub cli_path: Option<PathBuf>,
    pub source_label: String,
}

#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub question: String,
    pub answer: String,
}

#[derive(Debug, Clone)]
pub struct AiAnswer {
    pub content: String,
    pub model: String,
    pub tool_calls: Vec<String>,
    pub cards_consulted: usize,
    pub total_cards_indexed: Option<usize>,
    pub dlp_redactions: usize,
    pub elapsed_ms: u64,
}

impl ResolvedAiConfig {
    /// Resolve local AI configuration from `[ai]` in `client.toml`, env vars, or installed local AI CLIs.
    pub fn discover(file_cfg: Option<&AiFileConfig>) -> Option<Self> {
        // 1. Explicit `[ai]` in client.toml or DISTILL_AI_* env vars
        if let Some(cfg) = Self::from_explicit(file_cfg) {
            return Some(cfg);
        }

        // 2. Standard OpenAI env vars
        if let Ok(key) = non_empty_env("OPENAI_API_KEY") {
            let base = non_empty_env("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let model = non_empty_env("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            return Some(Self {
                provider: AiProvider::OpenAi,
                base_url: base,
                model,
                fallback_model: None,
                api_key: key,
                cli_path: None,
                source_label: "OPENAI".into(),
            });
        }

        // 3. Standard Anthropic env vars
        if let Ok(key) = non_empty_env("ANTHROPIC_API_KEY") {
            let base = non_empty_env("ANTHROPIC_BASE_URL")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
            let model = non_empty_env("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-3-5-sonnet-latest".to_string());
            return Some(Self {
                provider: AiProvider::Anthropic,
                base_url: base,
                model,
                fallback_model: None,
                api_key: key,
                cli_path: None,
                source_label: "ANTHROPIC".into(),
            });
        }

        // 4. Local signed-in Codex CLI (`codex`)
        if let Some(bin) = find_cli_binary("codex") {
            return Some(Self {
                provider: AiProvider::CodexCli,
                base_url: String::new(),
                model: "codex-local".into(),
                fallback_model: None,
                api_key: String::new(),
                cli_path: Some(bin),
                source_label: "local CLI".into(),
            });
        }

        // 5. Local signed-in Claude CLI (`claude`)
        if let Some(bin) = find_cli_binary("claude") {
            return Some(Self {
                provider: AiProvider::ClaudeCli,
                base_url: String::new(),
                model: "claude-local".into(),
                fallback_model: None,
                api_key: String::new(),
                cli_path: Some(bin),
                source_label: "local CLI".into(),
            });
        }

        // 6. DeepSeek env vars
        if let Ok(key) = non_empty_env("DEEPSEEK_API_KEY") {
            let base = non_empty_env("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com".to_string());
            let primary = non_empty_env("DEEPSEEK_MODEL")
                .or_else(|_| non_empty_env("YOUXI_DEEPSEEK_MODEL"))
                .unwrap_or_else(|_| "deepseek-chat".to_string());
            let fallback = (primary != "deepseek-chat").then(|| "deepseek-chat".to_string());
            return Some(Self {
                provider: AiProvider::OpenAi,
                base_url: base,
                model: primary,
                fallback_model: fallback,
                api_key: key,
                cli_path: None,
                source_label: "DEEPSEEK".into(),
            });
        }

        // 7. Local Ollama via env
        if let Ok(model) = non_empty_env("OLLAMA_MODEL") {
            let host = non_empty_env("OLLAMA_HOST")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
            let base = format!("{}/v1", host.trim_end_matches('/'));
            return Some(Self {
                provider: AiProvider::OpenAi,
                base_url: base,
                model,
                fallback_model: None,
                api_key: "ollama".to_string(),
                cli_path: None,
                source_label: "OLLAMA".into(),
            });
        }

        None
    }

    fn from_explicit(file_cfg: Option<&AiFileConfig>) -> Option<Self> {
        let env_provider = non_empty_env("DISTILL_AI_PROVIDER").ok();
        let env_key = non_empty_env("DISTILL_AI_API_KEY").ok();
        let env_base = non_empty_env("DISTILL_AI_BASE_URL").ok();
        let env_model = non_empty_env("DISTILL_AI_MODEL").ok();

        let file_provider = file_cfg.and_then(|c| c.provider.clone().filter(|s| !s.trim().is_empty()));
        let file_key = file_cfg.and_then(|c| {
            c.api_key
                .clone()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| c.api_key_env.as_deref().and_then(|k| non_empty_env(k).ok()))
        });
        let file_base = file_cfg.and_then(|c| c.base_url.clone().filter(|s| !s.trim().is_empty()));
        let file_model = file_cfg.and_then(|c| c.model.clone().filter(|s| !s.trim().is_empty()));
        let file_cli = file_cfg
            .and_then(|c| c.cli_path.clone().filter(|s| !s.trim().is_empty()))
            .map(PathBuf::from);

        let provider_raw = env_provider.or(file_provider)?;
        let p_lower = provider_raw.trim().to_ascii_lowercase();
        let preset = find_preset(&p_lower);
        let wire = preset.map(|p| p.wire).unwrap_or(p_lower.as_str());

        if wire == "codex" || p_lower == "codex-cli" {
            let bin = file_cli.or_else(|| find_cli_binary("codex"))?;
            let model = env_model
                .or(file_model)
                .unwrap_or_else(|| "codex-local".to_string());
            return Some(Self {
                provider: AiProvider::CodexCli,
                base_url: String::new(),
                model,
                fallback_model: None,
                api_key: String::new(),
                cli_path: Some(bin),
                source_label: "codex".into(),
            });
        }

        if wire == "claude" || p_lower == "claude-cli" {
            let bin = file_cli.or_else(|| find_cli_binary("claude"))?;
            let model = env_model
                .or(file_model)
                .unwrap_or_else(|| "claude-local".to_string());
            return Some(Self {
                provider: AiProvider::ClaudeCli,
                base_url: String::new(),
                model,
                fallback_model: None,
                api_key: String::new(),
                cli_path: Some(bin),
                source_label: "claude".into(),
            });
        }

        let preset_env_key = preset
            .filter(|p| !p.default_key_env.is_empty())
            .and_then(|p| non_empty_env(p.default_key_env).ok());
        let needs_key = preset.map(|p| p.needs_api_key).unwrap_or(true);
        let api_key = match env_key.or(file_key).or(preset_env_key) {
            Some(k) => k,
            None if !needs_key => "local".to_string(),
            None => return None,
        };

        let provider = if wire == "anthropic" {
            AiProvider::Anthropic
        } else {
            AiProvider::OpenAi
        };
        let base_url = env_base
            .or(file_base)
            .or_else(|| {
                preset
                    .filter(|p| !p.default_base_url.is_empty())
                    .map(|p| p.default_base_url.to_string())
            })
            .unwrap_or_else(|| match provider {
                AiProvider::OpenAi => "https://api.openai.com/v1".to_string(),
                AiProvider::Anthropic => "https://api.anthropic.com".to_string(),
                _ => String::new(),
            });
        let model = env_model
            .or(file_model)
            .or_else(|| {
                preset
                    .filter(|p| !p.default_model.is_empty())
                    .map(|p| p.default_model.to_string())
            })
            .unwrap_or_else(|| match provider {
                AiProvider::OpenAi => "gpt-4o-mini".to_string(),
                AiProvider::Anthropic => "claude-sonnet-4-20250514".to_string(),
                _ => String::new(),
            });

        Some(Self {
            provider,
            base_url,
            model,
            fallback_model: None,
            api_key,
            cli_path: None,
            source_label: preset.map(|p| p.id.to_string()).unwrap_or_else(|| p_lower),
        })
    }

    pub fn display_badge(&self) -> String {
        if self.model.contains(&self.source_label) {
            self.model.clone()
        } else {
            format!("{} ({})", self.model, self.source_label)
        }
    }
}

pub struct LocalAiAgent {
    cfg: ResolvedAiConfig,
    http: reqwest::Client,
}

impl LocalAiAgent {
    pub fn new(cfg: ResolvedAiConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .context("failed to build HTTP client for local AI")?;
        Ok(Self { cfg, http })
    }

    pub fn config(&self) -> &ResolvedAiConfig {
        &self.cfg
    }

    /// Answer a user's natural-language question using service-side raw Git code + optional VictoriaLogs
    /// investigation combined with local AI synthesis.
    #[allow(clippy::too_many_arguments)]
    pub async fn ask<F>(
        &self,
        p2p: &Arc<P2pClient>,
        repo: &str,
        branch: &str,
        commit_sha: &str,
        question: &str,
        include_logs: Option<bool>,
        log_range: Option<String>,
        history: &[ConversationTurn],
        on_progress: F,
    ) -> Result<AiAnswer>
    where
        F: Fn(String) + Send + Sync,
    {
        let started = Instant::now();

        // Fast conversational liveness check: `ping` -> real remote P2P ping + instant `pong!`
        let q_norm = question
            .trim()
            .trim_end_matches(['!', '！', '?', '？', '.', '。'])
            .to_ascii_lowercase();
        if q_norm == "ping" {
            on_progress("正在探测远端 distilld 加密通道…".to_string());
            let sha_short = &commit_sha[..commit_sha.len().min(8)];
            return match p2p.call(DistillRequest::Ping).await {
                Ok(DistillResponse::Pong {
                    server_version,
                    protocol,
                }) => {
                    let ms = started.elapsed().as_millis() as u64;
                    Ok(AiAnswer {
                        content: format!(
                            "**pong!** 远端服务与本地 AI 均在线就绪。\n\n\
                             - 远端通道：已连通（`{protocol}` · `v{server_version}` · RTT `{ms}ms`）\n\
                             - 当前上下文：`{repo} @ {branch}`（commit `{sha_short}`）\n\
                             - 本地 AI：`{}`\n\
                             - 可直接提问代码实现、接口/字段、分支差异，或排查 VictoriaLogs 线上日志（支持自动识别日志意图）。",
                            self.cfg.display_badge()
                        ),
                        model: self.cfg.model.clone(),
                        tool_calls: vec![format!("ping→pong({ms}ms)")],
                        cards_consulted: 0,
                        total_cards_indexed: None,
                        dlp_redactions: 0,
                        elapsed_ms: ms,
                    })
                }
                Ok(DistillResponse::Error { code, message }) => {
                    Err(anyhow!("远端 ping 异常 {code}: {message}"))
                }
                Err(e) => Err(anyhow!("远端 ping 失败: {e:#}")),
                _ => Err(anyhow!("远端 ping 返回了意外响应")),
            };
        }

        let system_prompt = build_system_prompt(repo, branch, commit_sha);
        on_progress("正在由远端服务并行整合 Raw 代码与日志上下文…".to_string());

        match self.cfg.provider {
            AiProvider::OpenAi => {
                self.run_openai_loop(
                    p2p,
                    repo,
                    branch,
                    &system_prompt,
                    question,
                    include_logs,
                    log_range.as_deref(),
                    history,
                    &on_progress,
                    started,
                )
                .await
            }
            AiProvider::Anthropic => {
                self.run_anthropic_loop(
                    p2p,
                    repo,
                    branch,
                    &system_prompt,
                    question,
                    include_logs,
                    log_range.as_deref(),
                    history,
                    &on_progress,
                    started,
                )
                .await
            }
            AiProvider::CodexCli => {
                self.run_codex_cli(
                    p2p,
                    repo,
                    branch,
                    commit_sha,
                    question,
                    include_logs,
                    log_range,
                    history,
                    &on_progress,
                    started,
                )
                .await
            }
            AiProvider::ClaudeCli => {
                self.run_claude_cli(
                    p2p,
                    repo,
                    branch,
                    commit_sha,
                    question,
                    include_logs,
                    log_range,
                    history,
                    &on_progress,
                    started,
                )
                .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_codex_cli<F>(
        &self,
        p2p: &Arc<P2pClient>,
        repo: &str,
        branch: &str,
        commit_sha: &str,
        question: &str,
        include_logs: Option<bool>,
        log_range: Option<String>,
        history: &[ConversationTurn],
        on_progress: &F,
        started: Instant,
    ) -> Result<AiAnswer>
    where
        F: Fn(String) + Send + Sync,
    {
        let bundle = run_service_investigate(
            p2p,
            repo,
            branch,
            question,
            include_logs,
            log_range,
            on_progress,
        )
        .await?;
        let autonomous = !is_casual_greeting(question) || include_logs == Some(true);
        let prompt = build_grounded_cli_prompt(
            repo,
            branch,
            commit_sha,
            question,
            history,
            &bundle,
            autonomous,
        );

        on_progress(if autonomous {
            "本地 Codex AI 正在自主决策探索远端源码与日志…".to_string()
        } else {
            "本地 Codex AI 正在回复…".to_string()
        });

        let bin = self
            .cfg
            .cli_path
            .clone()
            .or_else(|| find_cli_binary("codex"))
            .ok_or_else(|| anyhow!("未找到本机 `codex` 可执行文件"))?;

        // Isolated empty temp directory so Codex explores ONLY the remote repo & VictoriaLogs via distill MCP
        let tmp_dir = std::env::temp_dir().join(format!(
            "distill-codex-{}-{}",
            std::process::id(),
            started.elapsed().as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir)?;
        let out_file = tmp_dir.join("answer.md");

        let mut cmd = Command::new(&bin);
        cmd.args([
            "exec",
            "--skip-git-repo-check",
            "--ephemeral",
            "--ignore-user-config",
            "-s",
            "read-only",
            "-C",
        ])
        .arg(&tmp_dir)
        .args([
            "-c",
            r#"model_reasoning_effort="low""#,
            "-c",
            "plugins={}",
            "-c",
            "notify=[]",
        ]);

        if autonomous {
            let self_exe = env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(str::to_string))
                .unwrap_or_else(|| "distill".to_string());
            let escaped_exe = self_exe.replace('\\', "\\\\").replace('"', "\\\"");
            cmd.args([
                "-c",
                &format!(r#"mcp_servers.distill.command="{escaped_exe}""#),
                "-c",
                r#"mcp_servers.distill.args=["mcp"]"#,
                "-c",
                "mcp_servers.distill.startup_timeout_sec=15",
                "-c",
                "mcp_servers.distill.tool_timeout_sec=30",
            ]);
            for tool_name in DISTILL_READONLY_MCP_TOOLS {
                cmd.args([
                    "-c",
                    &format!(r#"mcp_servers.distill.tools.{tool_name}.approval_mode="approve""#),
                ]);
            }
            cmd.arg("--json");
        } else {
            cmd.args(["-c", "mcp_servers={}"]);
        }

        cmd.arg("-o").arg(&out_file);

        if self.cfg.model != "codex-local" && !self.cfg.model.trim().is_empty() {
            cmd.args(["-m", self.cfg.model.trim()]);
        }

        cmd.arg("-")
            .stdin(Stdio::piped())
            .stdout(if autonomous {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("启动本机 codex 失败 ({})", bin.display()))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await?;
        }

        let mut extra_tool_calls: Vec<String> = Vec::new();
        let mut extra_evidence = 0usize;
        let mut last_agent_msg = String::new();

        if autonomous
            && let Some(stdout) = child.stdout.take()
        {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let Ok(ev) = serde_json::from_str::<Value>(trimmed) else {
                    continue;
                };
                let ev_type = ev.get("type").and_then(Value::as_str).unwrap_or("");
                let Some(item) = ev.get("item") else {
                    continue;
                };
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                match (ev_type, item_type) {
                    ("item.started", "mcp_tool_call") => {
                        let tool = item.get("tool").and_then(Value::as_str).unwrap_or("");
                        let args = item.get("arguments").unwrap_or(&Value::Null);
                        on_progress(format_mcp_progress_label(tool, args));
                    }
                    ("item.completed", "mcp_tool_call") => {
                        let tool = item.get("tool").and_then(Value::as_str).unwrap_or("");
                        let args = item.get("arguments").unwrap_or(&Value::Null);
                        let status = item.get("status").and_then(Value::as_str).unwrap_or("");
                        if status == "completed" {
                            extra_tool_calls.push(format_mcp_short_step(tool, args));
                            extra_evidence += 1;
                        }
                    }
                    ("item.completed", "agent_message") => {
                        if let Some(txt) = item.get("text").and_then(Value::as_str)
                            && !txt.trim().is_empty()
                        {
                            last_agent_msg = txt.trim().to_string();
                        }
                    }
                    _ => {}
                }
            }
        }

        let output = child.wait_with_output().await?;
        let answer_text = std::fs::read_to_string(&out_file).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&tmp_dir);

        let trimmed = if !answer_text.trim().is_empty() {
            answer_text.trim().to_string()
        } else {
            last_agent_msg
        };
        if !output.status.success() && trimmed.is_empty() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "codex 执行失败 ({}): {}",
                output.status,
                truncate_err(err.trim(), 240)
            ));
        }
        if trimmed.is_empty() {
            return Err(anyhow!("codex 未返回回答内容"));
        }

        let evidence_count = bundle_evidence_count(&bundle) + extra_evidence;
        let mut all_steps = bundle.steps;
        all_steps.extend(extra_tool_calls);
        Ok(AiAnswer {
            content: trimmed,
            model: self.cfg.model.clone(),
            tool_calls: all_steps,
            cards_consulted: evidence_count,
            total_cards_indexed: bundle.total_cards_indexed,
            dlp_redactions: bundle.dlp_redactions,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_claude_cli<F>(
        &self,
        p2p: &Arc<P2pClient>,
        repo: &str,
        branch: &str,
        commit_sha: &str,
        question: &str,
        include_logs: Option<bool>,
        log_range: Option<String>,
        history: &[ConversationTurn],
        on_progress: &F,
        started: Instant,
    ) -> Result<AiAnswer>
    where
        F: Fn(String) + Send + Sync,
    {
        let bundle = run_service_investigate(
            p2p,
            repo,
            branch,
            question,
            include_logs,
            log_range,
            on_progress,
        )
        .await?;
        let autonomous = !is_casual_greeting(question) || include_logs == Some(true);
        let prompt = build_grounded_cli_prompt(
            repo,
            branch,
            commit_sha,
            question,
            history,
            &bundle,
            autonomous,
        );

        on_progress("本地 Claude AI 正在自主探索远端源码与日志并生成结论…".to_string());

        let bin = self
            .cfg
            .cli_path
            .clone()
            .or_else(|| find_cli_binary("claude"))
            .ok_or_else(|| anyhow!("未找到本机 `claude` 可执行文件"))?;

        let tmp_dir = std::env::temp_dir().join(format!(
            "distill-claude-{}-{}",
            std::process::id(),
            started.elapsed().as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir)?;

        let mut cmd = Command::new(&bin);
        cmd.current_dir(&tmp_dir)
            .args(["-p", "--output-format", "text", "--tools", ""]);
        if autonomous {
            let self_exe = env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(str::to_string))
                .unwrap_or_else(|| "distill".to_string());
            let mcp_cfg_path = tmp_dir.join("mcp.json");
            let mcp_cfg = json!({
                "mcpServers": {
                    "distill": {
                        "command": self_exe,
                        "args": ["mcp"]
                    }
                }
            });
            std::fs::write(&mcp_cfg_path, serde_json::to_string_pretty(&mcp_cfg)?)?;
            cmd.arg("--mcp-config")
                .arg(&mcp_cfg_path)
                .args([
                    "--strict-mcp-config",
                    "--allowedTools",
                    "mcp__distill__*",
                ]);
        }
        if self.cfg.model != "claude-local" && !self.cfg.model.trim().is_empty() {
            cmd.args(["--model", self.cfg.model.trim()]);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .with_context(|| format!("启动本机 claude 失败 ({})", bin.display()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(prompt.as_bytes()).await?;
            stdin.shutdown().await?;
        }

        let output = child.wait_with_output().await?;
        let _ = std::fs::remove_dir_all(&tmp_dir);

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() || stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = if !stdout.is_empty() { stdout } else { stderr.trim().to_string() };
            return Err(anyhow!("claude 执行失败: {}", truncate_err(&msg, 240)));
        }

        let evidence_count = bundle_evidence_count(&bundle);
        Ok(AiAnswer {
            content: stdout,
            model: self.cfg.model.clone(),
            tool_calls: bundle.steps,
            cards_consulted: evidence_count,
            total_cards_indexed: bundle.total_cards_indexed,
            dlp_redactions: bundle.dlp_redactions,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_openai_loop<F>(
        &self,
        p2p: &Arc<P2pClient>,
        repo: &str,
        branch: &str,
        system_prompt: &str,
        question: &str,
        include_logs: Option<bool>,
        log_range: Option<&str>,
        history: &[ConversationTurn],
        on_progress: &F,
        started: Instant,
    ) -> Result<AiAnswer>
    where
        F: Fn(String) + Send + Sync,
    {
        let url = openai_completions_url(&self.cfg.base_url);
        let tools = openai_tool_schemas();

        // Pre-fetch service-side consolidated evidence bundle (raw git code + optional VictoriaLogs)
        let pre_bundle = if !is_casual_greeting(question) {
            run_service_investigate(
                p2p,
                repo,
                branch,
                question,
                include_logs,
                log_range.map(str::to_string),
                on_progress,
            )
            .await
            .ok()
        } else {
            None
        };

        let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
        for turn in history.iter().rev().take(4).rev() {
            messages.push(json!({ "role": "user", "content": turn.question }));
            messages.push(json!({ "role": "assistant", "content": turn.answer }));
        }
        let user_msg = if let Some(ref b) = pre_bundle {
            format!(
                "{question}\n\n---\n以下为服务端已整合好的初步证据（如需进一步深挖可继续调用工具）：\n{}",
                format_bundle_for_llm(b)
            )
        } else {
            question.to_string()
        };
        messages.push(json!({ "role": "user", "content": user_msg }));

        let mut active_model = self.cfg.model.clone();
        let mut tool_calls_log = pre_bundle
            .as_ref()
            .map(|b| b.steps.clone())
            .unwrap_or_default();
        let mut cards_consulted = pre_bundle
            .as_ref()
            .map(bundle_evidence_count)
            .unwrap_or(0);
        let mut total_cards_indexed = pre_bundle.as_ref().and_then(|b| b.total_cards_indexed);
        let mut dlp_redactions = pre_bundle.as_ref().map(|b| b.dlp_redactions).unwrap_or(0);

        for round in 0..12 {
            let mut body = json!({
                "model": active_model,
                "messages": messages,
                "temperature": 0.2,
            });
            if round < 11 {
                body["tools"] = tools.clone();
                body["tool_choice"] = json!("auto");
            }

            let resp = self
                .http
                .post(&url)
                .bearer_auth(&self.cfg.api_key)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("调用本地 AI 接口失败 ({url})"))?;

            let status = resp.status();
            let raw_text = resp.text().await.unwrap_or_default();

            if !status.is_success() {
                if round == 0
                    && let Some(fb) = &self.cfg.fallback_model
                    && active_model != *fb
                {
                    on_progress(format!("模型 `{active_model}` 不可用，自动切换到 `{fb}`…"));
                    active_model = fb.clone();
                    continue;
                }
                return Err(anyhow!(
                    "AI 接口返回 HTTP {}: {}",
                    status.as_u16(),
                    truncate_err(&raw_text, 240)
                ));
            }

            let parsed: Value = serde_json::from_str(&raw_text)
                .with_context(|| format!("解析 AI 响应 JSON 失败: {}", truncate_err(&raw_text, 200)))?;

            let Some(msg) = parsed
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("message"))
            else {
                return Err(anyhow!("AI 响应缺少 choices[0].message"));
            };

            let tool_calls = msg
                .get("tool_calls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            if !tool_calls.is_empty() && round < 11 {
                messages.push(msg.clone());

                for tc in tool_calls {
                    let tc_id = tc
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("call_0")
                        .to_string();
                    let func = tc.get("function").cloned().unwrap_or_else(|| json!({}));
                    let name = func
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args_str = func
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let args: Value = serde_json::from_str(args_str).unwrap_or_else(|_| json!({}));

                    let exec =
                        execute_remote_tool(p2p, repo, branch, &name, &args, on_progress).await;
                    tool_calls_log.push(exec.summary_label);
                    cards_consulted += exec.cards_count;
                    if exec.total_indexed.is_some() {
                        total_cards_indexed = exec.total_indexed;
                    }
                    dlp_redactions += exec.dlp_redactions;

                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tc_id,
                        "content": exec.content_for_llm,
                    }));
                }

                on_progress("AI 正在汇总远端语义事实并生成结论…".to_string());
                continue;
            }

            let content = msg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();

            if content.is_empty() {
                return Err(anyhow!("AI 返回了空内容"));
            }

            return Ok(AiAnswer {
                content,
                model: active_model,
                tool_calls: tool_calls_log,
                cards_consulted,
                total_cards_indexed,
                dlp_redactions,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }

        Err(anyhow!("AI 工具调用轮次超过上限"))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_anthropic_loop<F>(
        &self,
        p2p: &Arc<P2pClient>,
        repo: &str,
        branch: &str,
        system_prompt: &str,
        question: &str,
        include_logs: Option<bool>,
        log_range: Option<&str>,
        history: &[ConversationTurn],
        on_progress: &F,
        started: Instant,
    ) -> Result<AiAnswer>
    where
        F: Fn(String) + Send + Sync,
    {
        let url = anthropic_messages_url(&self.cfg.base_url);
        let tools = anthropic_tool_schemas();

        let pre_bundle = if !is_casual_greeting(question) {
            run_service_investigate(
                p2p,
                repo,
                branch,
                question,
                include_logs,
                log_range.map(str::to_string),
                on_progress,
            )
            .await
            .ok()
        } else {
            None
        };

        let mut messages = Vec::new();
        for turn in history.iter().rev().take(4).rev() {
            messages.push(json!({ "role": "user", "content": turn.question }));
            messages.push(json!({ "role": "assistant", "content": turn.answer }));
        }
        let user_msg = if let Some(ref b) = pre_bundle {
            format!(
                "{question}\n\n---\n以下为服务端已整合好的初步证据（如需进一步深挖可继续调用工具）：\n{}",
                format_bundle_for_llm(b)
            )
        } else {
            question.to_string()
        };
        messages.push(json!({ "role": "user", "content": user_msg }));

        let mut active_model = self.cfg.model.clone();
        let mut tool_calls_log = pre_bundle
            .as_ref()
            .map(|b| b.steps.clone())
            .unwrap_or_default();
        let mut cards_consulted = pre_bundle
            .as_ref()
            .map(bundle_evidence_count)
            .unwrap_or(0);
        let mut total_cards_indexed = pre_bundle.as_ref().and_then(|b| b.total_cards_indexed);
        let mut dlp_redactions = pre_bundle.as_ref().map(|b| b.dlp_redactions).unwrap_or(0);

        for round in 0..12 {
            let mut body = json!({
                "model": active_model,
                "max_tokens": 4096,
                "system": system_prompt,
                "messages": messages,
            });
            if round < 11 {
                body["tools"] = tools.clone();
            }

            let resp = self
                .http
                .post(&url)
                .header("x-api-key", &self.cfg.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send()
                .await
                .with_context(|| format!("调用本地 AI 接口失败 ({url})"))?;

            let status = resp.status();
            let raw_text = resp.text().await.unwrap_or_default();
            if !status.is_success() {
                if round == 0
                    && let Some(fb) = &self.cfg.fallback_model
                    && active_model != *fb
                {
                    on_progress(format!("模型 `{active_model}` 不可用，自动切换到 `{fb}`…"));
                    active_model = fb.clone();
                    continue;
                }
                return Err(anyhow!(
                    "AI 接口返回 HTTP {}: {}",
                    status.as_u16(),
                    truncate_err(&raw_text, 240)
                ));
            }

            let parsed: Value = serde_json::from_str(&raw_text)
                .with_context(|| format!("解析 AI 响应 JSON 失败: {}", truncate_err(&raw_text, 200)))?;
            let content_blocks = parsed
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();

            let mut tool_uses = Vec::new();
            let mut text_parts = Vec::new();
            for block in &content_blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("tool_use") => tool_uses.push(block.clone()),
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            text_parts.push(t.to_string());
                        }
                    }
                    _ => {}
                }
            }

            if !tool_uses.is_empty() && round < 11 {
                messages.push(json!({ "role": "assistant", "content": content_blocks }));
                let mut tool_results = Vec::new();
                for tu in tool_uses {
                    let id = tu
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("tu_0")
                        .to_string();
                    let name = tu
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args = tu.get("input").cloned().unwrap_or_else(|| json!({}));

                    let exec =
                        execute_remote_tool(p2p, repo, branch, &name, &args, on_progress).await;
                    tool_calls_log.push(exec.summary_label);
                    cards_consulted += exec.cards_count;
                    if exec.total_indexed.is_some() {
                        total_cards_indexed = exec.total_indexed;
                    }
                    dlp_redactions += exec.dlp_redactions;

                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": exec.content_for_llm,
                    }));
                }
                messages.push(json!({ "role": "user", "content": tool_results }));
                on_progress("AI 正在汇总远端语义事实并生成结论…".to_string());
                continue;
            }

            let final_text = text_parts.join("\n").trim().to_string();
            if final_text.is_empty() {
                return Err(anyhow!("AI 返回了空内容"));
            }

            return Ok(AiAnswer {
                content: final_text,
                model: active_model,
                tool_calls: tool_calls_log,
                cards_consulted,
                total_cards_indexed,
                dlp_redactions,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
        }

        Err(anyhow!("AI 工具调用轮次超过上限"))
    }
}

/// Delegate raw Git code + optional VictoriaLogs log-intent investigation to `distilld`
/// in a single encrypted iroh QUIC round-trip.
async fn run_service_investigate<F>(
    p2p: &Arc<P2pClient>,
    repo: &str,
    branch: &str,
    question: &str,
    include_logs: Option<bool>,
    log_range: Option<String>,
    on_progress: &F,
) -> Result<InvestigationBundle>
where
    F: Fn(String),
{
    if is_casual_greeting(question) && include_logs != Some(true) {
        return Ok(InvestigationBundle {
            repo: repo.to_string(),
            effective_branch: branch.to_string(),
            commit_sha: String::new(),
            detected_intents: Vec::new(),
            steps: Vec::new(),
            tree: Vec::new(),
            files: Vec::new(),
            grep_matches: Vec::new(),
            commits: Vec::new(),
            semantic_cards: Vec::new(),
            total_cards_indexed: None,
            branch_diff: None,
            logs: None,
            dlp_redactions: 0,
        });
    }

    let hint = match include_logs {
        Some(true) => format!("服务端正在联合检索 `{repo}@{branch}` 代码 + VictoriaLogs 日志…"),
        Some(false) => format!("服务端正在检索 `{repo}@{branch}` Raw Git 代码…"),
        None => format!("服务端正在分析意图并整合 `{repo}@{branch}` 代码与日志…"),
    };
    on_progress(hint);

    let t0 = Instant::now();
    match p2p
        .call(DistillRequest::Investigate {
            repo: repo.to_string(),
            branch: branch.to_string(),
            question: question.to_string(),
            include_logs,
            log_range,
        })
        .await?
    {
        DistillResponse::Investigation(bundle) => {
            if !bundle.steps.is_empty() {
                on_progress(format!(
                    "服务端整合完成 ({}ms): {}",
                    t0.elapsed().as_millis(),
                    bundle.steps.join(" · ")
                ));
            }
            Ok(bundle)
        }
        DistillResponse::Error { code, message } => {
            Err(anyhow!("服务端整合查询失败 [{code}]: {message}"))
        }
        other => Err(anyhow!("服务端返回意外响应: {other:?}")),
    }
}

fn bundle_evidence_count(b: &InvestigationBundle) -> usize {
    let diff_count = b
        .branch_diff
        .as_ref()
        .map(|d| d.added.len() + d.modified.len() + d.removed_titles.len())
        .unwrap_or(0);
    let log_count = b.logs.as_ref().map(|l| l.entries.len()).unwrap_or(0);
    b.tree.len()
        + b.files.len()
        + b.grep_matches.len()
        + b.commits.len()
        + b.semantic_cards.len()
        + diff_count
        + log_count
}

fn format_log_scope(entry: &distill_proto::LogEntry) -> String {
    match (entry.namespace.is_empty(), entry.container.is_empty()) {
        (false, false) => format!("{}/{}", entry.namespace, entry.container),
        (true, false) => entry.container.clone(),
        (false, true) => entry.namespace.clone(),
        (true, true) => {
            if !entry.cluster.is_empty() {
                entry.cluster.clone()
            } else if !entry.stream.is_empty() {
                entry.stream.clone()
            } else {
                "-".to_string()
            }
        }
    }
}

fn format_logs_for_llm(logs: &LogsQueryResult) -> String {
    let mut out = format!(
        "VictoriaLogs 端点: `{}`\n查询语句 (LogsQL): `{}`\n时间窗口: `{}` | 命中 {} 条{} | DLP 脱敏 {} 处\n",
        logs.endpoint,
        logs.effective_query,
        logs.range,
        logs.entries.len(),
        if logs.truncated { "（已截断）" } else { "" },
        logs.dlp_redactions
    );
    if !logs.note.trim().is_empty() {
        out.push_str(&format!("备注: {}\n", logs.note.trim()));
    }
    if logs.entries.is_empty() {
        out.push_str("（该时间窗内未命中日志记录）\n");
        return out;
    }
    out.push('\n');
    for (i, entry) in logs.entries.iter().enumerate() {
        let scope = format_log_scope(entry);
        let pod_tag = if entry.pod.is_empty() {
            String::new()
        } else {
            format!(" pod={}", entry.pod)
        };
        out.push_str(&format!(
            "{}. [{}] ({}{}): {}\n",
            i + 1,
            entry.timestamp,
            scope,
            pod_tag,
            entry.message
        ));
    }
    out
}

fn format_bundle_for_llm(b: &InvestigationBundle) -> String {
    let mut out = String::new();

    if !b.detected_intents.is_empty() {
        out.push_str(&format!(
            "（服务端判定查询意图: {} | 有效分支: `{}`）\n\n",
            b.detected_intents.join(", "),
            b.effective_branch
        ));
    }

    if let Some(ref logs) = b.logs {
        out.push_str("## VictoriaLogs 运行日志（服务端实时查询并已自动脱敏密钥）\n");
        out.push_str(&format_logs_for_llm(logs));
        out.push('\n');
    }

    if !b.tree.is_empty() {
        out.push_str(&format!(
            "## 分支 `{}` 目录与文件树（{} 项）\n",
            b.effective_branch,
            b.tree.len()
        ));
        for e in b.tree.iter().take(140) {
            out.push_str(&format!("- {}\n", e.path));
        }
        out.push('\n');
    }

    if !b.files.is_empty() {
        out.push_str("## 直读文件内容（已自动脱敏密钥）\n");
        for f in &b.files {
            out.push_str(&format!(
                "### `{}` (L{}-L{} / 共{}行)\n```\n{}\n```\n\n",
                f.path, f.start_line, f.end_line, f.total_lines, f.content
            ));
        }
    }

    if !b.grep_matches.is_empty() {
        out.push_str(&format!(
            "## 远端全文检索命中（`git grep`，{} 处，已自动脱敏密钥）\n",
            b.grep_matches.len()
        ));
        for m in &b.grep_matches {
            out.push_str(&format!("- `{}:{}`\n", m.path, m.line_number));
            for before in &m.before {
                out.push_str(&format!("    {before}\n"));
            }
            out.push_str(&format!("  > {}\n", m.line));
            for after in &m.after {
                out.push_str(&format!("    {after}\n"));
            }
        }
        out.push('\n');
    }

    if !b.commits.is_empty() {
        out.push_str(&format!(
            "## 分支 `{}` 最近 {} 次提交记录（已脱敏）\n",
            b.effective_branch,
            b.commits.len()
        ));
        for (i, c) in b.commits.iter().enumerate() {
            let sha8 = &c.commit_sha[..c.commit_sha.len().min(8)];
            let domains = if c.changed_domains.is_empty() {
                String::new()
            } else {
                format!(" | 涉及模块: {}", c.changed_domains.join(", "))
            };
            out.push_str(&format!(
                "{}. `{sha8}` [{}] {} — {} ({}个文件{})\n",
                i + 1,
                c.timestamp,
                c.author,
                c.subject,
                c.files_changed,
                domains
            ));
            if !c.body.is_empty() {
                out.push_str(&format!("   说明: {}\n", c.body.replace('\n', " / ")));
            }
        }
        out.push('\n');
    }

    if !b.semantic_cards.is_empty() {
        out.push_str("## 远端脱敏语义索引卡片\n");
        out.push_str(&format_cards_for_llm(
            &b.effective_branch,
            b.total_cards_indexed.unwrap_or(b.semantic_cards.len()),
            &b.semantic_cards,
        ));
        out.push('\n');
    }

    if let Some(ref d) = b.branch_diff {
        out.push_str(&format!(
            "## 分支行为差异 ({} → {})\n新增 {} 项，变更 {} 项，移除 {} 项。\n",
            d.base_branch,
            d.target_branch,
            d.added.len(),
            d.modified.len(),
            d.removed_titles.len()
        ));
        for c in d.added.iter().take(15) {
            out.push_str(&format!("- + [{}] {}: {}\n", c.kind.as_str(), c.title, c.summary));
        }
        for c in d.modified.iter().take(15) {
            out.push_str(&format!("- ~ [{}] {}: {}\n", c.kind.as_str(), c.title, c.summary));
        }
        for t in d.removed_titles.iter().take(10) {
            out.push_str(&format!("- - {t}\n"));
        }
        out.push('\n');
    }

    if out.is_empty() {
        out.push_str("（本次未检索到代码或日志证据）\n");
    }
    out
}

fn is_casual_greeting(q: &str) -> bool {
    let norm = q
        .trim()
        .trim_end_matches(['!', '！', '?', '？', '.', '。', '~'])
        .to_ascii_lowercase();
    matches!(
        norm.as_str(),
        "ping"
            | "hi"
            | "hello"
            | "hey"
            | "你好"
            | "您好"
            | "在吗"
            | "在不在"
            | "测试"
            | "test"
            | "你是谁"
            | "你能做什么"
            | "你能干什么"
            | "有什么功能"
    )
}

const DISTILL_READONLY_MCP_TOOLS: &[&str] = &[
    "server_status",
    "server_metrics",
    "list_repos",
    "list_branches",
    "list_tree",
    "grep_repo",
    "read_file",
    "get_recent_commits",
    "search_semantics",
    "get_api_contracts",
    "diff_branch_behavior",
    "query_vlogs",
    "investigate",
];

fn format_mcp_progress_label(tool: &str, args: &Value) -> String {
    match tool {
        "list_tree" => {
            let p = args.get("path").and_then(Value::as_str).unwrap_or("/");
            format!("🧭 AI 自主浏览目录: list_tree(\"{p}\")")
        }
        "read_file" => {
            let p = args.get("path").and_then(Value::as_str).unwrap_or("?");
            let s = args.get("start_line").and_then(Value::as_u64);
            let e = args.get("end_line").and_then(Value::as_u64);
            match (s, e) {
                (Some(a), Some(b)) => format!("📄 AI 自主通读源码: {p} (L{a}-L{b})"),
                (Some(a), None) => format!("📄 AI 自主通读源码: {p} (L{a}-)"),
                _ => format!("📄 AI 自主通读源码: {p}"),
            }
        }
        "grep_repo" => {
            let pat = args.get("pattern").and_then(Value::as_str).unwrap_or("");
            format!("🔎 AI 自主全文检索: grep(\"{pat}\")")
        }
        "get_recent_commits" => {
            let b = args.get("branch").and_then(Value::as_str).unwrap_or("");
            format!("🧾 AI 自主查看提交: commits({b})")
        }
        "query_vlogs" => {
            let q = args.get("query").and_then(Value::as_str).unwrap_or("*");
            let r = args.get("range").and_then(Value::as_str).unwrap_or("15m");
            format!("📜 AI 自主查线上日志: vlogs({r}, \"{q}\")")
        }
        "diff_branch_behavior" => {
            let b = args.get("base_branch").and_then(Value::as_str).unwrap_or("main");
            let t = args.get("target_branch").and_then(Value::as_str).unwrap_or("");
            format!("🔀 AI 自主对比分支: diff({b}..{t})")
        }
        "search_semantics" | "get_api_contracts" => {
            let q = args
                .get("query")
                .or_else(|| args.get("keyword"))
                .and_then(Value::as_str)
                .unwrap_or("");
            format!("🗂️ AI 自主查语义: {tool}(\"{q}\")")
        }
        other => format!("🔧 AI 自主调用远端工具: {other}"),
    }
}

fn format_mcp_short_step(tool: &str, args: &Value) -> String {
    match tool {
        "list_tree" => {
            let p = args.get("path").and_then(Value::as_str).unwrap_or("/");
            format!("tree({p})")
        }
        "read_file" => {
            let p = args.get("path").and_then(Value::as_str).unwrap_or("?");
            let base = p.rsplit('/').next().unwrap_or(p);
            format!("read({base})")
        }
        "grep_repo" => {
            let pat = args.get("pattern").and_then(Value::as_str).unwrap_or("");
            format!("grep(\"{pat}\")")
        }
        "get_recent_commits" => "commits".to_string(),
        "query_vlogs" => {
            let r = args.get("range").and_then(Value::as_str).unwrap_or("15m");
            format!("vlogs({r})")
        }
        "diff_branch_behavior" => "diff".to_string(),
        other => other.to_string(),
    }
}

fn build_grounded_cli_prompt(
    repo: &str,
    branch: &str,
    commit_sha: &str,
    question: &str,
    history: &[ConversationTurn],
    bundle: &InvestigationBundle,
    autonomous: bool,
) -> String {
    let eff_branch = if bundle.effective_branch.is_empty() {
        branch
    } else {
        &bundle.effective_branch
    };
    let eff_sha = if bundle.commit_sha.is_empty() {
        commit_sha
    } else {
        &bundle.commit_sha
    };
    let sha_short = &eff_sha[..eff_sha.len().min(8)];
    let mut out = if autonomous {
        format!(
            "你是 Distill 自主研发与运维分析助手，正在通过 IM 对话窗协助用户深度分析远端 Git 仓库 `{repo}` 分支 `{eff_branch}`（commit `{sha_short}`）的全部原始代码与线上 VictoriaLogs 运行日志（`http://vlogs.hs99.vip:9428`）。\n\
             核心工作方式（务必遵守）：\n\
             1. **你已挂载远端只读 `distill` MCP 工具，可以直接自主通篇访问整个远端 Git 仓库的任意原始代码与线上日志**：\n\
                - `list_tree(repo=\"{repo}\", branch=\"{eff_branch}\", path=..., max_depth=...)`：浏览任意层级的目录和文件；\n\
                - `read_file(repo=\"{repo}\", branch=\"{eff_branch}\", path=..., start_line=..., end_line=...)`：按需通读任意源码文件原文（支持分段续读）；\n\
                - `grep_repo(repo=\"{repo}\", branch=\"{eff_branch}\", pattern=...)`：全仓库高速正则/关键词全文检索；\n\
                - `get_recent_commits(repo=\"{repo}\", branch=\"{eff_branch}\")` / `diff_branch_behavior` / `search_semantics`；\n\
                - `query_vlogs(query=..., range=..., limit=..., repo_hint=\"{repo}\")`：随时按你的决策查询线上 VictoriaLogs 实时日志、报错、耗时或 `traceId`。\n\
             2. **严禁运行任何本地 shell 命令**（不要调用 bash/rg/ls/rmb 等；当前目录为空沙箱，目标代码全部在远端 `distill` MCP 中）。\n\
             3. **主动多步自主探索，绝不被动拒答**：下方先附上了服务端预取的基线上下文。如果基线不足以给出扎实、有据可查的结论（例如用户问“看下代码实现质量”“梳理核心架构”“排查隐患”“找为什么报错”），你必须**主动连续调用 `distill` MCP 工具**——例如先 `list_tree` 深入核心目录，再 `read_file` 通读代表性的核心业务/架构/异常处理/事务/测试/配置源码，配合 `grep_repo` 扫描关键模式或反模式，并**自主判断调用 `query_vlogs` 检查线上近期 error/warn/异常日志是否印证代码问题**——绝不要以“只提供了片段/语义索引未命中”为由推脱！\n\
             4. **严禁臆造**：结论中的每个文件路径、行号、函数名、配置项与日志条目必须来自你实际读取的证据。\n\
             5. **输出风格**：先给核心结论，再按维度/严重度分点列具体代码位置（`path:line`）+ 线上日志佐证（如有）+ 可执行改进建议；使用简洁中文 Markdown，方便直接复制转发，不要输出原始 JSON 或多余前后缀。\n\n"
        )
    } else {
        format!(
            "你是 Distill 研发与运维智能助手，正在通过 IM 对话窗协助用户（当前已连上 `{repo} @ {eff_branch}`，commit `{sha_short}`，并已接入远端全部原始代码与 VictoriaLogs 线上日志）。\n\
             请像自然的 IM 助手一样友好简洁回复用户的打招呼或连通性测试，无需调用任何工具，仅输出回答本身。\n\n"
        )
    };

    if !history.is_empty() {
        out.push_str("## 历史对话上下文\n");
        for turn in history.iter().rev().take(3).rev() {
            out.push_str(&format!("- 问：{}\n- 答：{}\n\n", turn.question, turn.answer));
        }
    }

    if autonomous {
        out.push_str("## 服务端预取基线上下文（如需通读更多文件或查更多日志，请直接调用 `distill` MCP 工具）\n");
        out.push_str(&format_bundle_for_llm(bundle));
    }
    out.push_str(&format!("\n## 用户当前问题\n{question}\n"));
    out
}

fn find_cli_binary(name: &str) -> Option<PathBuf> {
    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        let local_bin = home.join(".local/bin").join(name);
        if is_executable(&local_bin) {
            return Some(local_bin);
        }
    }
    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let cand = dir.join(name);
            if is_executable(&cand) {
                return Some(cand);
            }
        }
    }
    for extra in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let cand = Path::new(extra).join(name);
        if is_executable(&cand) {
            return Some(cand);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

struct ToolExecResult {
    summary_label: String,
    content_for_llm: String,
    cards_count: usize,
    total_indexed: Option<usize>,
    dlp_redactions: usize,
}

async fn execute_remote_tool<F>(
    p2p: &Arc<P2pClient>,
    repo: &str,
    branch: &str,
    tool_name: &str,
    args: &Value,
    on_progress: &F,
) -> ToolExecResult
where
    F: Fn(String),
{
    match tool_name {
        "search_semantics" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let kind = args
                .get("kind")
                .and_then(Value::as_str)
                .and_then(SemanticKind::from_str_opt);
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| (v as usize).clamp(1, 20))
                .unwrap_or(10);
            let target_branch = args
                .get("branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();

            on_progress(format!("AI 正在检索远端语义: \"{query}\"…"));
            let req = DistillRequest::SearchSemantics {
                repo: repo.to_string(),
                branch: target_branch,
                query: query.clone(),
                kind,
                limit: Some(limit),
            };
            match p2p.call(req).await {
                Ok(DistillResponse::SearchResult(payload)) => ToolExecResult {
                    summary_label: format!("search(\"{query}\")→{}条", payload.hits.len()),
                    content_for_llm: format_cards_for_llm(
                        &payload.branch,
                        payload.total_cards_indexed,
                        &payload.hits,
                    ),
                    cards_count: payload.hits.len(),
                    total_indexed: Some(payload.total_cards_indexed),
                    dlp_redactions: payload.dlp_redactions,
                },
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: format!("search(\"{query}\")→错误"),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: format!("search(\"{query}\")→失败"),
                    content_for_llm: format!("RPC 调用失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: format!("search(\"{query}\")"),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "get_api_contracts" => {
            let keyword = args
                .get("keyword")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| (v as usize).clamp(1, 20))
                .unwrap_or(10);
            let target_branch = args
                .get("branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();

            on_progress(format!("AI 正在查询 API 契约: \"{keyword}\"…"));
            let req = DistillRequest::GetApiContracts {
                repo: repo.to_string(),
                branch: target_branch,
                keyword: keyword.clone(),
                limit: Some(limit),
            };
            match p2p.call(req).await {
                Ok(DistillResponse::SearchResult(payload)) => ToolExecResult {
                    summary_label: format!("api(\"{keyword}\")→{}条", payload.hits.len()),
                    content_for_llm: format_cards_for_llm(
                        &payload.branch,
                        payload.total_cards_indexed,
                        &payload.hits,
                    ),
                    cards_count: payload.hits.len(),
                    total_indexed: Some(payload.total_cards_indexed),
                    dlp_redactions: payload.dlp_redactions,
                },
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: format!("api(\"{keyword}\")→错误"),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: format!("api(\"{keyword}\")→失败"),
                    content_for_llm: format!("RPC 调用失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: format!("api(\"{keyword}\")"),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "diff_branch_behavior" => {
            let base_branch = args
                .get("base_branch")
                .and_then(Value::as_str)
                .unwrap_or("main")
                .trim()
                .to_string();
            let target_branch = args
                .get("target_branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();

            on_progress(format!("AI 正在对比分支 {base_branch} → {target_branch}…"));
            let req = DistillRequest::DiffBranchBehavior {
                repo: repo.to_string(),
                base_branch: base_branch.clone(),
                target_branch: target_branch.clone(),
            };
            match p2p.call(req).await {
                Ok(DistillResponse::BranchDiff(diff)) => {
                    let count = diff.added.len() + diff.modified.len() + diff.removed_titles.len();
                    let mut out = format!(
                        "分支行为差异 {} ({}) -> {} ({}): 新增 {} 项, 变更 {} 项, 移除 {} 项\n",
                        diff.base_branch,
                        &diff.base_commit_sha[..diff.base_commit_sha.len().min(8)],
                        diff.target_branch,
                        &diff.target_commit_sha[..diff.target_commit_sha.len().min(8)],
                        diff.added.len(),
                        diff.modified.len(),
                        diff.removed_titles.len()
                    );
                    if !diff.added.is_empty() {
                        out.push_str("\n[新增]\n");
                        for c in diff.added.iter().take(25) {
                            out.push_str(&format!("- [{}] {}: {}\n", c.kind.as_str(), c.title, c.summary));
                        }
                    }
                    if !diff.modified.is_empty() {
                        out.push_str("\n[变更]\n");
                        for c in diff.modified.iter().take(25) {
                            out.push_str(&format!("- [{}] {}: {}\n", c.kind.as_str(), c.title, c.summary));
                        }
                    }
                    if !diff.removed_titles.is_empty() {
                        out.push_str("\n[移除]\n");
                        for t in diff.removed_titles.iter().take(20) {
                            out.push_str(&format!("- {t}\n"));
                        }
                    }
                    ToolExecResult {
                        summary_label: format!("diff({base_branch}..{target_branch})"),
                        content_for_llm: out,
                        cards_count: count,
                        total_indexed: None,
                        dlp_redactions: diff.dlp_redactions,
                    }
                }
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: format!("diff({base_branch})→错误"),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: format!("diff({base_branch})→失败"),
                    content_for_llm: format!("RPC 调用失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "diff".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "list_tree" => {
            let target_branch = args
                .get("branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();
            let path = args.get("path").and_then(Value::as_str).map(str::to_string);
            let max_depth = args.get("max_depth").and_then(Value::as_u64).map(|v| v as usize);
            let limit = args.get("limit").and_then(Value::as_u64).map(|v| v as usize);
            on_progress(format!("AI 正在浏览 `{target_branch}` 目录树…"));
            match p2p
                .call(DistillRequest::ListTree {
                    repo: repo.to_string(),
                    branch: target_branch,
                    path,
                    max_depth,
                    limit,
                })
                .await
            {
                Ok(DistillResponse::Tree { entries, .. }) => {
                    let body = entries
                        .iter()
                        .map(|e| e.path.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    ToolExecResult {
                        summary_label: format!("tree→{}项", entries.len()),
                        content_for_llm: body,
                        cards_count: entries.len(),
                        total_indexed: None,
                        dlp_redactions: 0,
                    }
                }
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: "tree→错误".into(),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: "tree→失败".into(),
                    content_for_llm: format!("RPC 失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "tree".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "grep_repo" => {
            let pattern = args
                .get("pattern")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let target_branch = args
                .get("branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();
            let path_prefix = args
                .get("path_prefix")
                .and_then(Value::as_str)
                .map(str::to_string);
            let limit = args.get("limit").and_then(Value::as_u64).map(|v| v as usize);
            on_progress(format!("AI 正在全文检索 `{pattern}`…"));
            match p2p
                .call(DistillRequest::GrepRepo {
                    repo: repo.to_string(),
                    branch: target_branch,
                    pattern: pattern.clone(),
                    path_prefix,
                    case_insensitive: true,
                    context_lines: Some(2),
                    limit,
                })
                .await
            {
                Ok(DistillResponse::GrepResult {
                    matches,
                    dlp_redactions,
                    ..
                }) => {
                    let mut out = format!("命中 {} 处：\n", matches.len());
                    for m in &matches {
                        out.push_str(&format!("{}:{}: {}\n", m.path, m.line_number, m.line));
                        for a in &m.after {
                            out.push_str(&format!("  {}\n", a));
                        }
                    }
                    ToolExecResult {
                        summary_label: format!("grep(\"{pattern}\")→{}处", matches.len()),
                        content_for_llm: out,
                        cards_count: matches.len(),
                        total_indexed: None,
                        dlp_redactions,
                    }
                }
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: format!("grep(\"{pattern}\")→错误"),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: format!("grep(\"{pattern}\")→失败"),
                    content_for_llm: format!("RPC 失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "grep".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "read_file" => {
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let target_branch = args
                .get("branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();
            let start_line = args.get("start_line").and_then(Value::as_u64).map(|v| v as usize);
            let end_line = args.get("end_line").and_then(Value::as_u64).map(|v| v as usize);
            on_progress(format!("AI 正在读取 `{path}`…"));
            match p2p
                .call(DistillRequest::ReadFile {
                    repo: repo.to_string(),
                    branch: target_branch,
                    path: path.clone(),
                    start_line,
                    end_line,
                })
                .await
            {
                Ok(DistillResponse::FileContent {
                    path,
                    start_line,
                    end_line,
                    total_lines,
                    content,
                    dlp_redactions,
                    ..
                }) => ToolExecResult {
                    summary_label: format!("read({path}:L{start_line}-{end_line})"),
                    content_for_llm: format!(
                        "`{path}` (L{start_line}-L{end_line} / {total_lines}行):\n{content}"
                    ),
                    cards_count: 1,
                    total_indexed: None,
                    dlp_redactions,
                },
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: format!("read({path})→错误"),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: format!("read({path})→失败"),
                    content_for_llm: format!("RPC 失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "read".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "get_recent_commits" => {
            let target_branch = args
                .get("branch")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(branch)
                .to_string();
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| (v as usize).clamp(1, 30))
                .unwrap_or(15);
            on_progress(format!("AI 正在获取 `{target_branch}` 最近提交记录…"));
            match p2p
                .call(DistillRequest::RecentCommits {
                    repo: repo.to_string(),
                    branch: target_branch.clone(),
                    limit: Some(limit),
                })
                .await
            {
                Ok(DistillResponse::Commits {
                    commits,
                    dlp_redactions,
                    ..
                }) => {
                    let mut out = format!("分支 `{target_branch}` 最近 {} 次提交：\n", commits.len());
                    for (i, c) in commits.iter().enumerate() {
                        let sha8 = &c.commit_sha[..c.commit_sha.len().min(8)];
                        let domains = c.changed_domains.join(", ");
                        out.push_str(&format!(
                            "{}. `{sha8}` [{}] {} — {} ({} files; {})\n",
                            i + 1,
                            c.timestamp,
                            c.author,
                            c.subject,
                            c.files_changed,
                            domains
                        ));
                    }
                    ToolExecResult {
                        summary_label: format!("commits({target_branch})→{}条", commits.len()),
                        content_for_llm: out,
                        cards_count: commits.len(),
                        total_indexed: None,
                        dlp_redactions,
                    }
                }
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: format!("commits({target_branch})→错误"),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: format!("commits({target_branch})→失败"),
                    content_for_llm: format!("RPC 调用失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "commits".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "list_branches" => {
            on_progress("AI 正在获取分支清单…".to_string());
            match p2p
                .call(DistillRequest::ListBranches {
                    repo: repo.to_string(),
                })
                .await
            {
                Ok(DistillResponse::Branches { branches, .. }) => {
                    let lines = branches
                        .iter()
                        .map(|b| {
                            format!(
                                "- {} ({}): {}",
                                b.name,
                                &b.commit_sha[..b.commit_sha.len().min(8)],
                                b.commit_summary
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    ToolExecResult {
                        summary_label: format!("branches→{}个", branches.len()),
                        content_for_llm: lines,
                        cards_count: 0,
                        total_indexed: None,
                        dlp_redactions: 0,
                    }
                }
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: "branches→错误".into(),
                    content_for_llm: format!("工具错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: "branches→失败".into(),
                    content_for_llm: format!("RPC 失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "branches".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        "query_vlogs" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or("*")
                .trim()
                .to_string();
            let range = args
                .get("range")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| (v as usize).clamp(1, 200));
            on_progress(format!("AI 正在查询 VictoriaLogs: `{query}`…"));
            match p2p
                .call(DistillRequest::QueryLogs {
                    query: query.clone(),
                    range,
                    start: args
                        .get("start")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    end: args.get("end").and_then(Value::as_str).map(str::to_string),
                    limit,
                    repo_hint: Some(repo.to_string()),
                })
                .await
            {
                Ok(DistillResponse::Logs(logs)) => ToolExecResult {
                    summary_label: format!("vlogs({})→{}条", logs.range, logs.entries.len()),
                    content_for_llm: format_logs_for_llm(&logs),
                    cards_count: logs.entries.len(),
                    total_indexed: None,
                    dlp_redactions: logs.dlp_redactions,
                },
                Ok(DistillResponse::Error { code, message }) => ToolExecResult {
                    summary_label: "vlogs→错误".into(),
                    content_for_llm: format!("日志查询错误 {code}: {message}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                Err(e) => ToolExecResult {
                    summary_label: "vlogs→失败".into(),
                    content_for_llm: format!("RPC 失败: {e:#}"),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
                _ => ToolExecResult {
                    summary_label: "vlogs".into(),
                    content_for_llm: "未返回预期结果".into(),
                    cards_count: 0,
                    total_indexed: None,
                    dlp_redactions: 0,
                },
            }
        }
        other => ToolExecResult {
            summary_label: format!("unknown({other})"),
            content_for_llm: format!("未知工具: {other}"),
            cards_count: 0,
            total_indexed: None,
            dlp_redactions: 0,
        },
    }
}

fn format_cards_for_llm(branch: &str, total_indexed: usize, hits: &[SemanticCard]) -> String {
    if hits.is_empty() {
        return format!(
            "分支 `{branch}`（总计 {total_indexed} 张语义卡）未命中任何记录。"
        );
    }
    let mut out = format!(
        "分支 `{branch}` 命中 {} / {} 张脱敏语义卡：\n",
        hits.len(),
        total_indexed
    );
    for (i, c) in hits.iter().enumerate() {
        out.push_str(&format!(
            "\n{}. [{}] {} (domain={})\n   摘要: {}\n",
            i + 1,
            c.kind.as_str(),
            c.title,
            c.domain,
            c.summary
        ));
        if !c.metadata.is_empty() {
            let meta = c
                .metadata
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("   元数据: {meta}\n"));
        }
    }
    out
}

fn build_system_prompt(repo: &str, branch: &str, commit_sha: &str) -> String {
    let sha_short = &commit_sha[..commit_sha.len().min(8)];
    format!(
        "你是 Distill 自主研发与运维分析助手，服务于产品、测试、运营与研发协作。\n\
         当前查询上下文：远端仓库 `{repo}`，分支 `{branch}`（commit `{sha_short}`），且已完整接入远端 Raw Git 全部代码访问工具与 VictoriaLogs（`http://vlogs.hs99.vip:9428`）实时运行日志。\n\
         重要规则：\n\
         1. 若用户是在打招呼、连通性测试或闲聊（如 ping、你好、在吗），请直接友好简洁回复。\n\
         2. **主动多步自主探索全仓库源码 + 按需查日志**：你拥有 `list_tree`、`read_file`、`grep_repo`、`get_recent_commits`、`diff_branch_behavior`、`search_semantics`、`query_vlogs` 等完整工具。面对宏观或开放性问题（如“看下代码实现质量”“梳理核心架构”“排查线上隐患”），不要停留在预取片段或说“看不到完整代码”——你必须主动多轮调用 `list_tree` + `read_file` 深入阅读多个关键源码文件，配合 `grep_repo` 扫描与 `query_vlogs` 核实线上报错/警告日志，拿到充足一手证据后再给结论。\n\
         3. 严禁编造不存在的文件路径、行号、接口名或日志内容；同时存在代码与日志证据时请做联合归因分析。\n\
         4. 输出格式：先给核心结论，再分点列具体代码位置（`path:line`）、日志佐证（如有）与可执行建议，使用简洁中文 Markdown，方便直接复制转发。"
    )
}

fn openai_tool_schemas() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "search_semantics",
                "description": "在当前仓库分支的脱敏语义索引中搜索接口、DTO字段、错误码、业务规则、数据表、迁移与规格文档。请使用简短精准的关键词（1~4个词）。",
                "parameters": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": { "type": "string" },
                        "kind": { "type": "string" },
                        "branch": { "type": "string" },
                        "limit": { "type": "integer" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_api_contracts",
                "description": "专项查询当前分支的 HTTP API 路由、请求方法、权限码、鉴权头与 DTO 名称。",
                "parameters": {
                    "type": "object",
                    "required": ["keyword"],
                    "properties": {
                        "keyword": { "type": "string" },
                        "branch": { "type": "string" },
                        "limit": { "type": "integer" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "diff_branch_behavior",
                "description": "对比同一仓库两个分支之间的接口、字段、错误码、规则与文档变更差异。",
                "parameters": {
                    "type": "object",
                    "required": ["base_branch"],
                    "properties": {
                        "base_branch": { "type": "string" },
                        "target_branch": { "type": "string" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_tree",
                "description": "快速列出指定分支的目录和文件树（任意语言通用）。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "可选子目录" },
                        "branch": { "type": "string" },
                        "max_depth": { "type": "integer" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "grep_repo",
                "description": "在当前分支任意文本/代码文件中高速正则或关键词全文检索（返回上下文行，自动脱敏密钥）。",
                "parameters": {
                    "type": "object",
                    "required": ["pattern"],
                    "properties": {
                        "pattern": { "type": "string", "description": "关键词或正则" },
                        "path_prefix": { "type": "string" },
                        "branch": { "type": "string" },
                        "limit": { "type": "integer" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "按相对路径和行号区间直读分支中的任意文本文件（自动脱敏密钥）。",
                "parameters": {
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string" },
                        "start_line": { "type": "integer" },
                        "end_line": { "type": "integer" },
                        "branch": { "type": "string" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_recent_commits",
                "description": "获取指定分支最近的 Git 提交历史摘要（SHA、时间、作者、提交说明与受影响模块），用于回答最近提交/改动记录。",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "branch": { "type": "string", "description": "可选分支名，默认当前选中分支" },
                        "limit": { "type": "integer", "description": "返回条数，默认 15" }
                    }
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_branches",
                "description": "列出当前仓库下可见的 Git 分支、commit SHA 与最新提交摘要。",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "query_vlogs",
                "description": "查询远端固化的 VictoriaLogs 实时运行日志（LogsQL 语法，如 `container:twosite-api error` 或 `traceId`）。自动脱敏密钥。",
                "parameters": {
                    "type": "object",
                    "required": ["query"],
                    "properties": {
                        "query": { "type": "string", "description": "LogsQL 查询表达式，如 `container:twosite-api error` 或具体 traceId" },
                        "range": { "type": "string", "description": "时间窗，如 5m / 15m / 1h / 6h / 24h，默认 15m" },
                        "limit": { "type": "integer", "description": "最大返回条数，默认 50" }
                    }
                }
            }
        }
    ])
}

fn anthropic_tool_schemas() -> Value {
    json!([
        {
            "name": "search_semantics",
            "description": "在当前仓库分支的脱敏语义索引中搜索接口、DTO字段、错误码、业务规则、数据表、迁移与规格文档。",
            "input_schema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "kind": { "type": "string" },
                    "branch": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "get_api_contracts",
            "description": "专项查询当前分支的 HTTP API 路由、方法、权限码与入参类型。",
            "input_schema": {
                "type": "object",
                "required": ["keyword"],
                "properties": {
                    "keyword": { "type": "string" },
                    "branch": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "diff_branch_behavior",
            "description": "对比两个分支之间的接口、字段、规则与文档变更差异。",
            "input_schema": {
                "type": "object",
                "required": ["base_branch"],
                "properties": {
                    "base_branch": { "type": "string" },
                    "target_branch": { "type": "string" }
                }
            }
        },
        {
            "name": "list_tree",
            "description": "快速列出指定分支的目录和文件树。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "branch": { "type": "string" },
                    "max_depth": { "type": "integer" }
                }
            }
        },
        {
            "name": "grep_repo",
            "description": "在当前分支任意文本/代码文件中高速全文检索（自动脱敏密钥）。",
            "input_schema": {
                "type": "object",
                "required": ["pattern"],
                "properties": {
                    "pattern": { "type": "string" },
                    "path_prefix": { "type": "string" },
                    "branch": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "read_file",
            "description": "按相对路径和行号区间直读分支中的任意文本文件（自动脱敏密钥）。",
            "input_schema": {
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer" },
                    "end_line": { "type": "integer" },
                    "branch": { "type": "string" }
                }
            }
        },
        {
            "name": "get_recent_commits",
            "description": "获取指定分支最近的 Git 提交历史摘要（SHA、时间、作者、提交说明与受影响模块）。",
            "input_schema": {
                "type": "object",
                "properties": {
                    "branch": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "list_branches",
            "description": "列出当前仓库下可见的 Git 分支与提交摘要。",
            "input_schema": { "type": "object", "properties": {} }
        },
        {
            "name": "query_vlogs",
            "description": "查询远端固化的 VictoriaLogs 实时运行日志（LogsQL 语法，如 `container:twosite-api error` 或 `traceId`）。",
            "input_schema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "range": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }
        }
    ])
}

fn openai_completions_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn anthropic_messages_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    if trimmed.ends_with("/v1/messages") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/messages")
    } else {
        format!("{trimmed}/v1/messages")
    }
}

fn non_empty_env(name: &str) -> std::result::Result<String, ()> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .ok_or(())
}

fn truncate_err(s: &str, max_chars: usize) -> String {
    let mut out: String = s.chars().take(max_chars).collect();
    if s.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_provider_urls() {
        assert_eq!(
            openai_completions_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            openai_completions_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            anthropic_messages_url("https://api.deepseek.com/anthropic"),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn formats_investigation_bundle_with_vlogs() {
        let bundle = InvestigationBundle {
            repo: "twosite".into(),
            effective_branch: "dev/v1".into(),
            commit_sha: "abcdef123456".into(),
            detected_intents: vec!["raw_grep".into(), "vlogs".into()],
            steps: vec!["vlogs(15m)→1条".into()],
            tree: Vec::new(),
            files: Vec::new(),
            grep_matches: Vec::new(),
            commits: Vec::new(),
            semantic_cards: Vec::new(),
            total_cards_indexed: None,
            branch_diff: None,
            logs: Some(LogsQueryResult {
                endpoint: "http://vlogs.hs99.vip:9428".into(),
                effective_query: "_time:15m (container:twosite-api) error".into(),
                range: "15m".into(),
                entries: vec![distill_proto::LogEntry {
                    timestamp: "2026-11-24T14:25:00Z".into(),
                    cluster: "ack-starlink-dev".into(),
                    namespace: "starlink-blockblast-ant".into(),
                    container: "twosite-api".into(),
                    pod: "twosite-api-0".into(),
                    stream: "stdout".into(),
                    message: "HTTP 500 timeout traceId=abc123".into(),
                    extra: Default::default(),
                }],
                truncated: false,
                dlp_redactions: 0,
                note: String::new(),
            }),
            dlp_redactions: 0,
        };
        let rendered = format_bundle_for_llm(&bundle);
        assert!(rendered.contains("VictoriaLogs 运行日志"));
        assert!(rendered.contains("starlink-blockblast-ant/twosite-api"));
        assert!(rendered.contains("HTTP 500 timeout"));
        assert_eq!(bundle_evidence_count(&bundle), 1);
    }
}
