use crate::ai::{
    AI_PRESETS, AiAnswer, AiFileConfig, ConversationTurn, LocalAiAgent, ResolvedAiConfig,
    find_preset, preset_readiness,
};
use crate::clipboard::copy_to_clipboard;
use crate::{P2pClient, save_ai_config};
use anyhow::Result;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, MouseButton,
    MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use distill_proto::{
    BranchBehaviorDiff, BranchInfo, DistillRequest, DistillResponse, FetchRepoResult, RepoSummary,
    SearchResultPayload, SemanticCard, SemanticKind, ServerStatusInfo,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Input,
    Sidebar,
}

#[derive(Debug, Clone)]
enum ChatItem {
    User {
        repo: String,
        branch: String,
        text: String,
    },
    AiConclusion {
        answer_id: usize,
        repo: String,
        branch: String,
        answer: AiAnswer,
    },
    SearchAnswer {
        answer_id: usize,
        payload: SearchResultPayload,
        elapsed_ms: u64,
        query: String,
    },
    DiffAnswer {
        answer_id: usize,
        diff: BranchBehaviorDiff,
        elapsed_ms: u64,
    },
    RawBlock {
        answer_id: usize,
        title: String,
        content: String,
        elapsed_ms: u64,
    },
    System {
        is_error: bool,
        text: String,
    },
}

impl ChatItem {
    fn answer_id(&self) -> Option<usize> {
        match self {
            Self::AiConclusion { answer_id, .. }
            | Self::SearchAnswer { answer_id, .. }
            | Self::DiffAnswer { answer_id, .. }
            | Self::RawBlock { answer_id, .. } => Some(*answer_id),
            _ => None,
        }
    }

    fn copyable_text(&self) -> Option<String> {
        match self {
            Self::AiConclusion { answer, .. } => Some(answer.content.clone()),
            Self::SearchAnswer { payload, .. } => {
                let mut out = format!(
                    "### {} @ {} 语义检索结果（命中 {}/{}）\n\n",
                    payload.repo,
                    payload.branch,
                    payload.hits.len(),
                    payload.total_cards_indexed
                );
                for (i, c) in payload.hits.iter().enumerate() {
                    out.push_str(&format!(
                        "{}. **[{}]** {} (`{}`)\n   - {}\n",
                        i + 1,
                        kind_label(c.kind),
                        c.title,
                        c.domain,
                        c.summary
                    ));
                    if !c.metadata.is_empty() {
                        let kv = c
                            .metadata
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(" · ");
                        out.push_str(&format!("   - {kv}\n"));
                    }
                }
                Some(out)
            }
            Self::DiffAnswer { diff, .. } => {
                let mut out = format!(
                    "### 分支差异：{} ({} → {})\n- 新增 {} 项 / 变更 {} 项 / 移除 {} 项\n",
                    diff.repo,
                    diff.base_branch,
                    diff.target_branch,
                    diff.added.len(),
                    diff.modified.len(),
                    diff.removed_titles.len()
                );
                for c in &diff.added {
                    out.push_str(&format!("- + [{}] {}\n", kind_label(c.kind), c.title));
                }
                for c in &diff.modified {
                    out.push_str(&format!("- ~ [{}] {}\n", kind_label(c.kind), c.title));
                }
                for t in &diff.removed_titles {
                    out.push_str(&format!("- - {t}\n"));
                }
                Some(out)
            }
            Self::RawBlock { content, .. } => Some(content.clone()),
            _ => None,
        }
    }
}

enum BgEvent {
    StatusLoaded {
        result: Result<(ServerStatusInfo, u64), String>,
    },
    ReposLoaded {
        result: Result<Vec<RepoSummary>, String>,
    },
    BranchesLoaded {
        repo: String,
        result: Result<Vec<BranchInfo>, String>,
    },
    FetchFinished {
        result: Result<(FetchRepoResult, u64), String>,
    },
    AiProgress {
        status: String,
    },
    AiFinished {
        repo: String,
        branch: String,
        question: String,
        result: Result<AiAnswer, String>,
    },
    SearchFinished {
        query: String,
        result: Result<(SearchResultPayload, u64), String>,
    },
    DiffFinished {
        result: Result<(BranchBehaviorDiff, u64), String>,
    },
    RawBlockFinished {
        title: String,
        result: Result<(String, u64), String>,
    },
}

#[derive(Default, Clone)]
struct ClickRegions {
    fetch_btn: Rect,
    refresh_branches_btn: Rect,
    refresh_status_btn: Rect,
    repo_header: Rect,
    branch_list_area: Rect,
    copy_latest_btn: Rect,
    select_mode_btn: Rect,
    copy_answer_btns: Vec<(Rect, usize)>,
    chat_area: Rect,
    input_area: Rect,
}

#[derive(Debug, Clone)]
enum AiModalState {
    ChooseProvider {
        idx: usize,
    },
    EditFields {
        preset_idx: usize,
        /// 0 = model, 1 = base_url, 2 = api_key
        field: usize,
        model: String,
        base_url: String,
        api_key: String,
    },
}

struct App {
    client: Arc<P2pClient>,
    ai: Option<Arc<LocalAiAgent>>,
    cfg_path: PathBuf,
    server_hint: String,
    tx: mpsc::UnboundedSender<BgEvent>,
    rx: mpsc::UnboundedReceiver<BgEvent>,

    // Remote state
    online: Option<bool>,
    last_rtt_ms: Option<u64>,
    last_error: Option<String>,
    server_status: Option<ServerStatusInfo>,
    repos: Vec<RepoSummary>,
    active_repo_idx: usize,
    branches: Vec<BranchInfo>,
    active_branch: String,
    selected_branch_idx: usize,
    branch_scroll: usize,

    // Busy / progress / toast
    fetching: bool,
    querying: bool,
    query_started_at: Option<Instant>,
    ai_progress: Option<String>,
    ai_steps: Vec<String>,
    status_loading: bool,
    tick: usize,
    last_status_poll: Instant,
    toast: Option<(String, bool, Instant)>,

    // Conversation & input
    next_answer_id: usize,
    messages: Vec<ChatItem>,
    ai_history: Vec<ConversationTurn>,
    chat_scroll_from_bottom: usize,
    input: String,
    cursor_byte: usize,
    input_history: Vec<String>,
    history_idx: Option<usize>,

    // Interaction
    focus: Focus,
    mouse_select_mode: bool,
    ai_modal: Option<AiModalState>,
    regions: ClickRegions,
    should_quit: bool,
}

impl App {
    fn new(
        client: Arc<P2pClient>,
        ai: Option<Arc<LocalAiAgent>>,
        cfg_path: PathBuf,
        server_hint: String,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let welcome = if let Some(ref agent) = ai {
            format!(
                "欢迎使用 Distill AI 语义问答终端（本地 AI: `{}` · 远端 iroh 端到端加密 · 零源码直出）。\n\
                 • 直接在下方用自然语言提问，本地 AI 会自动检索当前分支语义索引并输出中文结论。\n\
                 • 默认已开启「自由选字模式」：可直接鼠标拖选复制任意文本；按 `Ctrl+Y` 一键复制最新结论；按 `Ctrl+S` 可切换鼠标点击模式。\n\
                 • 切换分支/拉取：按 `Tab` 切到左侧栏 `↑/↓` + `Enter` 切换，`Ctrl+F` 拉取；输入 `/ai` 或按 `Ctrl+P` 随时切换 AI Provider（类似 pi `/model`）。",
                agent.config().display_badge()
            )
        } else {
            "欢迎使用 Distill 语义问答终端（当前未配置本地 AI，输入 `/ai` 或按 `Ctrl+P` 一键选择 OpenAI / Anthropic / DeepSeek / Gemini / Codex / Ollama 等 Provider）。\n\
             • 默认已开启「自由选字模式」：可直接鼠标划选复制；按 `Ctrl+Y` 一键复制最新结果；按 `Ctrl+S` 切换鼠标点击模式。"
                .to_string()
        };

        let mut app = Self {
            client,
            ai,
            cfg_path,
            server_hint,
            tx,
            rx,
            online: None,
            last_rtt_ms: None,
            last_error: None,
            server_status: None,
            repos: Vec::new(),
            active_repo_idx: 0,
            branches: Vec::new(),
            active_branch: String::new(),
            selected_branch_idx: 0,
            branch_scroll: 0,
            fetching: false,
            querying: false,
            query_started_at: None,
            ai_progress: None,
            ai_steps: Vec::new(),
            status_loading: false,
            tick: 0,
            last_status_poll: Instant::now(),
            toast: None,
            next_answer_id: 1,
            messages: vec![ChatItem::System {
                is_error: false,
                text: welcome,
            }],
            ai_history: Vec::new(),
            chat_scroll_from_bottom: 0,
            input: String::new(),
            cursor_byte: 0,
            input_history: Vec::new(),
            history_idx: None,
            focus: Focus::Input,
            mouse_select_mode: true,
            ai_modal: None,
            regions: ClickRegions::default(),
            should_quit: false,
        };
        app.trigger_refresh_status();
        app.trigger_load_repos();
        app
    }

    fn alloc_answer_id(&mut self) -> usize {
        let id = self.next_answer_id;
        self.next_answer_id += 1;
        id
    }

    fn set_toast(&mut self, msg: impl Into<String>, is_error: bool) {
        self.toast = Some((msg.into(), is_error, Instant::now()));
    }

    fn active_repo_name(&self) -> Option<&str> {
        self.repos.get(self.active_repo_idx).map(|r| r.name.as_str())
    }

    fn active_branch_info(&self) -> Option<&BranchInfo> {
        self.branches.iter().find(|b| b.name == self.active_branch)
    }

    fn push_system(&mut self, is_error: bool, text: impl Into<String>) {
        self.messages.push(ChatItem::System {
            is_error,
            text: text.into(),
        });
        self.chat_scroll_from_bottom = 0;
    }

    fn copy_answer(&mut self, target_id: Option<usize>) {
        let found = match target_id {
            Some(id) => self
                .messages
                .iter()
                .rev()
                .find(|m| m.answer_id() == Some(id))
                .and_then(ChatItem::copyable_text),
            None => self
                .messages
                .iter()
                .rev()
                .find_map(ChatItem::copyable_text),
        };
        let Some(text) = found else {
            self.set_toast("暂无可复制的结论", true);
            return;
        };
        let char_count = text.chars().count();
        match copy_to_clipboard(&text) {
            Ok(dest) => {
                self.set_toast(format!("✅ 已复制到{dest} ({char_count} 字)"), false);
            }
            Err(e) => {
                self.set_toast(format!("复制失败: {e}"), true);
            }
        }
    }

    fn toggle_mouse_select_mode(&mut self) {
        self.mouse_select_mode = !self.mouse_select_mode;
        let mut stdout = io::stdout();
        if self.mouse_select_mode {
            let _ = execute!(stdout, DisableMouseCapture);
            self.set_toast(
                "🖱 已开启选字模式（可直接鼠标划选复制；按 Ctrl+S 恢复按钮点击）",
                false,
            );
        } else {
            let _ = execute!(stdout, EnableMouseCapture);
            self.set_toast("🖱 已恢复交互模式（可点击分支 / Fetch / 复制按钮）", false);
        }
    }

    fn trigger_refresh_status(&mut self) {
        if self.status_loading {
            return;
        }
        self.status_loading = true;
        self.last_status_poll = Instant::now();
        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client.call(DistillRequest::ServerStatus).await {
                Ok(DistillResponse::Status(st)) => Ok((st, t0.elapsed().as_millis() as u64)),
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(other) => Err(format!("意外响应: {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(BgEvent::StatusLoaded { result: res });
        });
    }

    fn trigger_load_repos(&mut self) {
        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let res = match client.call(DistillRequest::ListRepos).await {
                Ok(DistillResponse::Repos(repos)) => Ok(repos),
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(other) => Err(format!("意外响应: {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(BgEvent::ReposLoaded { result: res });
        });
    }

    fn trigger_load_branches(&mut self, repo: String) {
        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let res = match client
                .call(DistillRequest::ListBranches { repo: repo.clone() })
                .await
            {
                Ok(DistillResponse::Branches { branches, .. }) => Ok(branches),
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(other) => Err(format!("意外响应: {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(BgEvent::BranchesLoaded { repo, result: res });
        });
    }

    fn trigger_fetch(&mut self) {
        if self.fetching {
            return;
        }
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "当前未加载到可用仓库，请先等待连接或按 Ctrl+R 刷新状态。");
            return;
        };
        self.fetching = true;
        self.push_system(false, format!("⟳ 正在通知远端拉取仓库 `{repo}` 最新分支..."));
        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client.call(DistillRequest::FetchRepo { repo }).await {
                Ok(DistillResponse::FetchResult(r)) => Ok((r, t0.elapsed().as_millis() as u64)),
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(other) => Err(format!("意外响应: {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(BgEvent::FetchFinished { result: res });
        });
    }

    fn select_branch_by_idx(&mut self, idx: usize) {
        if let Some(b) = self.branches.get(idx).cloned() {
            self.selected_branch_idx = idx;
            if self.active_branch != b.name {
                self.active_branch = b.name.clone();
                // Reset branch-scoped AI multi-turn history to avoid cross-branch confusion
                self.ai_history.clear();
                let sha = short_sha(&b.commit_sha);
                let repo = self.active_repo_name().unwrap_or("unknown").to_string();
                let idx_badge = match b.indexed_cards {
                    Some(n) => format!("已索引 {n} 张语义卡"),
                    None => "首次提问时将自动构建索引".to_string(),
                };
                self.push_system(
                    false,
                    format!("🔀 已切换对话分支：{repo} @ {} ({sha}) · {idx_badge}", b.name),
                );
            }
        }
    }

    fn submit_input(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input_history.push(text.clone());
        self.history_idx = None;
        self.input.clear();
        self.cursor_byte = 0;

        if text.starts_with('/') {
            self.handle_slash_command(&text);
            return;
        }

        if self.ai.is_some() {
            self.dispatch_ai_question(text, None, None);
        } else {
            self.dispatch_raw_search(text, None, 8);
        }
    }

    fn handle_slash_command(&mut self, raw: &str) {
        let mut parts = raw.splitn(2, char::is_whitespace);
        let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
        let arg = parts.next().unwrap_or("").trim();

        match cmd.as_str() {
            "/help" | "/?" => {
                self.push_system(
                    false,
                    "可用命令：\n\
                     • 直接输入自然语言问题      服务端自动整合 Raw Git 代码 + VictoriaLogs 日志意图，再由本地 AI 回复结论\n\
                     • /ask+log <问题>           强制联合查询 Raw Git 代码 + VictoriaLogs 日志并生成诊断结论（别名 /diagnose）\n\
                     • /vlogs [时间窗] [LogsQL]  直查 VictoriaLogs 实时日志（别名 /logs），如 /vlogs 5m container:twosite-api error\n\
                     • /ai 或 /model             打开 AI Provider 交互式设置（支持 codex/claude/openai/anthropic/deepseek/gemini/openrouter/qwen/moonshot/zhipu/ollama/custom，快捷键 Ctrl+P）\n\
                     • /ai <preset> [model] [key] 快速切换 AI Provider，例如 /ai codex 或 /ai deepseek deepseek-chat sk-xxx\n\
                     • /copy [编号]              复制最新（或指定编号 #N）的结论到剪贴板（快捷键 Ctrl+Y）\n\
                     • /select                   切换「鼠标自由选字模式(默认开)」与「鼠标按钮点击模式」（快捷键 Ctrl+S）\n\
                     • /tree [子目录]            快速查看当前分支目录树（毫秒级，全语言通用）\n\
                     • /grep <关键词/正则>       高速全文检索当前分支任意文件（自动脱敏密钥）\n\
                     • /read <路径> [起行] [止行] 直读当前分支任意文本文件（自动脱敏密钥）\n\
                     • /log [条数]               查看当前分支最近提交记录（默认 15 条）\n\
                     • /raw <关键词>             跳过 AI，直接查看原始脱敏语义卡片\n\
                     • /api <关键词>             仅检索 HTTP API 接口契约卡片\n\
                     • /kind <类型> <关键词>     按卡片类型过滤（api/dto/error/rule/entity/migration/doc/constant）\n\
                     • /diff <基准分支>          对比 <基准分支> 与当前分支的行为差异\n\
                     • /branch <分支名>          切换当前问答分支\n\
                     • /fetch                    触发远端 git fetch 并刷新分支列表\n\
                     • /status                   刷新远端服务状态\n\
                     • /metrics                  查看远端磁盘占用、Worktree/分支索引明细、阈值与 GC 统计\n\
                     • /clear                    清空对话与上下文记忆\n\
                     • /quit                     退出 TUI",
                );
            }
            "/ai" | "/model" | "/provider" => {
                self.handle_ai_command(arg);
            }
            "/copy" | "/cp" => {
                let id = if arg.is_empty() {
                    None
                } else {
                    arg.trim_start_matches('#').parse::<usize>().ok()
                };
                self.copy_answer(id);
            }
            "/select" => {
                self.toggle_mouse_select_mode();
            }
            "/clear" => {
                self.messages.clear();
                self.ai_history.clear();
                self.chat_scroll_from_bottom = 0;
                self.push_system(false, "已清空对话记录与上下文记忆。");
            }
            "/quit" | "/exit" | "/q" => {
                self.should_quit = true;
            }
            "/status" => {
                self.trigger_refresh_status();
                if let Some(repo) = self.active_repo_name().map(str::to_string) {
                    self.trigger_load_branches(repo);
                }
            }
            "/metrics" => {
                self.dispatch_metrics();
            }
            "/fetch" => {
                self.trigger_fetch();
            }
            "/branch" | "/b" => {
                if arg.is_empty() {
                    let list = self
                        .branches
                        .iter()
                        .map(|b| format!("{} ({})", b.name, short_sha(&b.commit_sha)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.push_system(false, format!("当前分支列表：{list}"));
                } else if let Some(idx) = self.branches.iter().position(|b| b.name == arg) {
                    self.select_branch_by_idx(idx);
                } else {
                    self.push_system(true, format!("未在当前仓库找到分支 `{arg}`。"));
                }
            }
            "/tree" | "/ls" => {
                let sub = (!arg.is_empty()).then(|| arg.to_string());
                self.dispatch_raw_tree(sub);
            }
            "/grep" | "/g" => {
                if arg.is_empty() {
                    self.push_system(true, "用法：/grep <关键词或正则>");
                } else {
                    self.dispatch_raw_grep(arg.to_string());
                }
            }
            "/read" | "/cat" => {
                let toks: Vec<&str> = arg.split_whitespace().collect();
                if toks.is_empty() {
                    self.push_system(true, "用法：/read <相对路径> [起始行] [结束行]");
                } else {
                    let path = toks[0].to_string();
                    let start = toks.get(1).and_then(|s| s.parse::<usize>().ok());
                    let end = toks.get(2).and_then(|s| s.parse::<usize>().ok());
                    self.dispatch_raw_read(path, start, end);
                }
            }
            "/log" => {
                let limit = arg.parse::<usize>().ok().unwrap_or(15).clamp(1, 50);
                self.dispatch_raw_log(limit);
            }
            "/vlogs" | "/logs" => {
                let (range, query) = parse_vlogs_slash_args(arg);
                self.dispatch_vlogs(query, range);
            }
            "/ask+log" | "/diagnose" | "/diag" => {
                if arg.is_empty() {
                    self.push_system(
                        true,
                        "用法：/ask+log <问题>，例如 /ask+log 最近15分钟 twosite-api 有什么500报错，对应哪段代码？",
                    );
                } else {
                    self.dispatch_ai_question(arg.to_string(), Some(true), None);
                }
            }
            "/raw" | "/search" => {
                if arg.is_empty() {
                    self.push_system(true, "用法：/raw <关键词>");
                } else {
                    self.dispatch_raw_search(arg.to_string(), None, 8);
                }
            }
            "/api" => {
                if arg.is_empty() {
                    self.push_system(true, "用法：/api <路由或模块关键词>");
                } else {
                    self.dispatch_raw_search(arg.to_string(), Some(SemanticKind::ApiEndpoint), 10);
                }
            }
            "/kind" => {
                let mut sub = arg.splitn(2, char::is_whitespace);
                let kind_str = sub.next().unwrap_or("");
                let kw = sub.next().unwrap_or("").trim();
                match (SemanticKind::from_str_opt(kind_str), kw.is_empty()) {
                    (Some(k), false) => self.dispatch_raw_search(kw.to_string(), Some(k), 10),
                    _ => self.push_system(
                        true,
                        "用法：/kind <api|dto|error|rule|entity|migration|doc|constant> <关键词>",
                    ),
                }
            }
            "/diff" => {
                if arg.is_empty() {
                    self.push_system(true, "用法：/diff <基准分支名>，例如 /diff main");
                } else {
                    self.dispatch_diff(arg.to_string());
                }
            }
            _ => {
                self.push_system(true, format!("未知命令 `{cmd}`，输入 /help 查看支持的命令。"));
            }
        }
    }

    fn dispatch_ai_question(
        &mut self,
        question: String,
        include_logs: Option<bool>,
        log_range: Option<String>,
    ) {
        if self.querying {
            self.push_system(true, "上一条提问仍在处理中，请稍候...");
            return;
        }
        let Some(ai) = self.ai.as_ref().map(Arc::clone) else {
            self.dispatch_raw_search(question, None, 8);
            return;
        };
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库，请稍候或按 Ctrl+R 刷新状态。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择分支，请在左侧栏点击一个分支。");
            return;
        }

        let branch = self.active_branch.clone();
        let commit_sha = self
            .active_branch_info()
            .map(|b| b.commit_sha.clone())
            .unwrap_or_default();

        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: branch.clone(),
            text: question.clone(),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some("正在发送问题到远端服务并分析意图…".to_string());

        let client = Arc::clone(&self.client);
        let history = self.ai_history.clone();
        let tx = self.tx.clone();
        let tx_prog = self.tx.clone();

        tokio::spawn(async move {
            let res = ai
                .ask(
                    &client,
                    &repo,
                    &branch,
                    &commit_sha,
                    &question,
                    include_logs,
                    log_range,
                    &history,
                    move |status| {
                        let _ = tx_prog.send(BgEvent::AiProgress { status });
                    },
                )
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(BgEvent::AiFinished {
                repo,
                branch,
                question,
                result: res,
            });
        });
    }

    fn dispatch_metrics(&mut self) {
        if self.querying {
            self.push_system(true, "上一条请求仍在执行中，请稍候...");
            return;
        }
        let repo_label = self.active_repo_name().unwrap_or("server").to_string();
        let branch_label = if self.active_branch.is_empty() {
            "metrics".to_string()
        } else {
            self.active_branch.clone()
        };
        self.messages.push(ChatItem::User {
            repo: repo_label,
            branch: branch_label,
            text: "/metrics".to_string(),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some("正在拉取远端 /metrics 磁盘与 Worktree 指标…".to_string());

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let title = "📊 远端服务 /metrics（磁盘 · Worktree · 分支索引 · GC 阈值）".to_string();
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client.call(DistillRequest::Metrics).await {
                Ok(DistillResponse::Metrics(m)) => {
                    Ok((crate::format_metrics_text(&m), t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Err(e) => Err(format!("{e:#}")),
                _ => Err("意外响应".into()),
            };
            let _ = tx.send(BgEvent::RawBlockFinished { title, result: res });
        });
    }

    fn dispatch_vlogs(&mut self, query: String, range: Option<String>) {
        if self.querying {
            self.push_system(true, "上一条请求仍在执行中，请稍候...");
            return;
        }
        let repo_hint = self.active_repo_name().map(str::to_string);
        let repo_label = repo_hint.clone().unwrap_or_else(|| "vlogs".to_string());
        let branch_label = if self.active_branch.is_empty() {
            "logs".to_string()
        } else {
            self.active_branch.clone()
        };
        let range_badge = range.as_deref().unwrap_or("15m").to_string();
        self.messages.push(ChatItem::User {
            repo: repo_label,
            branch: branch_label,
            text: format!("/vlogs {range_badge} {query}"),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some(format!(
            "服务端正在查询 VictoriaLogs ({range_badge}): `{query}`…"
        ));

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let title = format!("📜 VictoriaLogs ({range_badge}) `{query}`");
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client
                .call(DistillRequest::QueryLogs {
                    query,
                    range,
                    start: None,
                    end: None,
                    limit: Some(50),
                    repo_hint,
                })
                .await
            {
                Ok(DistillResponse::Logs(logs)) => {
                    let mut out = format!(
                        "端点: {} | 时间窗: {} | 查询: `{}`\n命中 {} 条{} · 密钥脱敏 {} 处\n",
                        logs.endpoint,
                        logs.range,
                        logs.effective_query,
                        logs.entries.len(),
                        if logs.truncated { "（已截断）" } else { "" },
                        logs.dlp_redactions
                    );
                    if !logs.note.trim().is_empty() {
                        out.push_str(&format!("备注: {}\n", logs.note.trim()));
                    }
                    if logs.entries.is_empty() {
                        out.push_str("\n（该时间窗内未命中日志，可扩大时间窗如 `/vlogs 1h ...`）");
                    } else {
                        out.push('\n');
                        for e in &logs.entries {
                            let scope = match (e.namespace.is_empty(), e.container.is_empty()) {
                                (false, false) => format!("{}/{}", e.namespace, e.container),
                                (true, false) => e.container.clone(),
                                (false, true) => e.namespace.clone(),
                                (true, true) => {
                                    if !e.stream.is_empty() {
                                        e.stream.clone()
                                    } else {
                                        "-".to_string()
                                    }
                                }
                            };
                            let ts = e.timestamp.get(..23).unwrap_or(&e.timestamp);
                            out.push_str(&format!("[{ts}] ({scope}) {}\n", e.message));
                        }
                        if out.ends_with('\n') {
                            out.pop();
                        }
                    }
                    Ok((out, t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Err(e) => Err(format!("{e:#}")),
                _ => Err("意外响应".into()),
            };
            let _ = tx.send(BgEvent::RawBlockFinished { title, result: res });
        });
    }

    fn dispatch_raw_tree(&mut self, subpath: Option<String>) {
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择分支。");
            return;
        }
        let branch = self.active_branch.clone();
        let label = format!(
            "/tree {}",
            subpath.as_deref().unwrap_or(".")
        );
        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: branch.clone(),
            text: label.clone(),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some(format!("正在读取 `{branch}` 目录树…"));

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let title = format!("📁 目录树 {repo}@{branch} {}", subpath.as_deref().unwrap_or("/"));
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client
                .call(DistillRequest::ListTree {
                    repo,
                    branch,
                    path: subpath,
                    max_depth: Some(3),
                    limit: Some(200),
                })
                .await
            {
                Ok(DistillResponse::Tree {
                    entries, truncated, ..
                }) => {
                    let mut out = entries
                        .iter()
                        .map(|e| e.path.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if truncated {
                        out.push_str("\n…（已截断，可传子目录如 `/tree apps/api` 细看）");
                    }
                    Ok((out, t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Err(e) => Err(format!("{e:#}")),
                _ => Err("意外响应".into()),
            };
            let _ = tx.send(BgEvent::RawBlockFinished { title, result: res });
        });
    }

    fn dispatch_raw_grep(&mut self, pattern: String) {
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择分支。");
            return;
        }
        let branch = self.active_branch.clone();
        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: branch.clone(),
            text: format!("/grep {pattern}"),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some(format!("正在全文检索 `{pattern}`…"));

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let title = format!("🔎 全文检索 {repo}@{branch} `{pattern}`");
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client
                .call(DistillRequest::GrepRepo {
                    repo,
                    branch,
                    pattern,
                    path_prefix: None,
                    case_insensitive: true,
                    context_lines: Some(1),
                    limit: Some(40),
                })
                .await
            {
                Ok(DistillResponse::GrepResult {
                    matches,
                    truncated,
                    dlp_redactions,
                    ..
                }) => {
                    if matches.is_empty() {
                        Ok(("未命中任何文件行。".to_string(), t0.elapsed().as_millis() as u64))
                    } else {
                        let mut out = format!(
                            "命中 {} 处{}（密钥脱敏 {} 处）：\n\n",
                            matches.len(),
                            if truncated { "（已截断）" } else { "" },
                            dlp_redactions
                        );
                        for m in &matches {
                            out.push_str(&format!("{}:{}: {}\n", m.path, m.line_number, m.line));
                        }
                        Ok((out, t0.elapsed().as_millis() as u64))
                    }
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Err(e) => Err(format!("{e:#}")),
                _ => Err("意外响应".into()),
            };
            let _ = tx.send(BgEvent::RawBlockFinished { title, result: res });
        });
    }

    fn dispatch_raw_read(&mut self, path: String, start: Option<usize>, end: Option<usize>) {
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择分支。");
            return;
        }
        let branch = self.active_branch.clone();
        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: branch.clone(),
            text: format!("/read {path}"),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some(format!("正在直读 `{path}`…"));

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let title = format!("📄 文件 {repo}@{branch} `{path}`");
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client
                .call(DistillRequest::ReadFile {
                    repo,
                    branch,
                    path,
                    start_line: start,
                    end_line: end,
                })
                .await
            {
                Ok(DistillResponse::FileContent {
                    start_line,
                    end_line,
                    total_lines,
                    content,
                    truncated,
                    dlp_redactions,
                    ..
                }) => {
                    let mut out = format!(
                        "行范围 L{start_line}-L{end_line} / 共 {total_lines} 行{}（密钥脱敏 {dlp_redactions} 处）：\n\n{content}",
                        if truncated { "（已截断，可用 `/read <路径> <起始行> <结束行>` 续看）" } else { "" }
                    );
                    if out.ends_with('\n') {
                        out.pop();
                    }
                    Ok((out, t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Err(e) => Err(format!("{e:#}")),
                _ => Err("意外响应".into()),
            };
            let _ = tx.send(BgEvent::RawBlockFinished { title, result: res });
        });
    }

    fn dispatch_raw_log(&mut self, limit: usize) {
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择分支。");
            return;
        }
        let branch = self.active_branch.clone();
        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: branch.clone(),
            text: format!("/log {limit}"),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some(format!("正在获取 `{branch}` 最近 {limit} 条提交…"));

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let title = format!("🧾 最近提交 {repo}@{branch}");
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client
                .call(DistillRequest::RecentCommits {
                    repo,
                    branch,
                    limit: Some(limit),
                })
                .await
            {
                Ok(DistillResponse::Commits { commits, .. }) => {
                    let out = commits
                        .iter()
                        .map(|c| {
                            let sha8 = &c.commit_sha[..c.commit_sha.len().min(8)];
                            let ts = format_commit_timestamp(&c.timestamp);
                            format!("{sha8}  {ts}  {}  {}", c.author, c.subject)
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok((out, t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Err(e) => Err(format!("{e:#}")),
                _ => Err("意外响应".into()),
            };
            let _ = tx.send(BgEvent::RawBlockFinished { title, result: res });
        });
    }

    fn dispatch_raw_search(&mut self, query: String, kind: Option<SemanticKind>, limit: usize) {
        if self.querying {
            self.push_system(true, "上一条请求仍在执行中，请稍候...");
            return;
        }
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库，无法查询。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择分支，请在左侧栏点击一个分支。");
            return;
        }
        let branch = self.active_branch.clone();
        let display_q = match kind {
            Some(k) => format!("/kind {} {}", k.as_str(), query),
            None => format!("/raw {query}"),
        };
        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: branch.clone(),
            text: display_q,
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some("正在检索远端语义卡片…".to_string());

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        let req = match kind {
            Some(SemanticKind::ApiEndpoint) => DistillRequest::GetApiContracts {
                repo,
                branch,
                keyword: query.clone(),
                limit: Some(limit),
            },
            _ => DistillRequest::SearchSemantics {
                repo,
                branch,
                query: query.clone(),
                kind,
                limit: Some(limit),
            },
        };

        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client.call(req).await {
                Ok(DistillResponse::SearchResult(payload)) => {
                    Ok((payload, t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(other) => Err(format!("意外响应: {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(BgEvent::SearchFinished { query, result: res });
        });
    }

    fn dispatch_diff(&mut self, base_branch: String) {
        if self.querying {
            self.push_system(true, "上一条请求仍在执行中，请稍候...");
            return;
        }
        let Some(repo) = self.active_repo_name().map(str::to_string) else {
            self.push_system(true, "尚未连接到远端仓库。");
            return;
        };
        if self.active_branch.is_empty() {
            self.push_system(true, "尚未选择目标分支。");
            return;
        }
        let target_branch = self.active_branch.clone();
        self.messages.push(ChatItem::User {
            repo: repo.clone(),
            branch: target_branch.clone(),
            text: format!("/diff {base_branch} -> {target_branch}"),
        });
        self.chat_scroll_from_bottom = 0;
        self.querying = true;
        self.query_started_at = Some(Instant::now());
        self.ai_steps.clear();
        self.ai_progress = Some(format!("正在对比 {base_branch} → {target_branch}…"));

        let client = Arc::clone(&self.client);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let t0 = Instant::now();
            let res = match client
                .call(DistillRequest::DiffBranchBehavior {
                    repo,
                    base_branch,
                    target_branch,
                })
                .await
            {
                Ok(DistillResponse::BranchDiff(diff)) => {
                    Ok((diff, t0.elapsed().as_millis() as u64))
                }
                Ok(DistillResponse::Error { code, message }) => Err(format!("{code}: {message}")),
                Ok(other) => Err(format!("意外响应: {other:?}")),
                Err(e) => Err(format!("{e:#}")),
            };
            let _ = tx.send(BgEvent::DiffFinished { result: res });
        });
    }

    fn process_bg_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                BgEvent::StatusLoaded { result } => {
                    self.status_loading = false;
                    match result {
                        Ok((st, rtt)) => {
                            self.online = Some(true);
                            self.last_rtt_ms = Some(rtt);
                            self.last_error = None;
                            self.server_status = Some(st);
                        }
                        Err(e) => {
                            self.online = Some(false);
                            self.last_error = Some(e);
                        }
                    }
                }
                BgEvent::ReposLoaded { result } => match result {
                    Ok(repos) => {
                        self.online = Some(true);
                        self.repos = repos;
                        if self.active_repo_idx >= self.repos.len() {
                            self.active_repo_idx = 0;
                        }
                        if let Some(repo) = self.active_repo_name().map(str::to_string) {
                            self.trigger_load_branches(repo);
                        }
                    }
                    Err(e) => {
                        self.online = Some(false);
                        self.last_error = Some(e.clone());
                        self.push_system(true, format!("获取仓库列表失败：{e}"));
                    }
                },
                BgEvent::BranchesLoaded { repo, result } => {
                    if self.active_repo_name() != Some(repo.as_str()) {
                        continue;
                    }
                    match result {
                        Ok(branches) => {
                            self.branches = branches;
                            if self.active_branch.is_empty()
                                || !self.branches.iter().any(|b| b.name == self.active_branch)
                            {
                                let default_b = self
                                    .repos
                                    .get(self.active_repo_idx)
                                    .map(|r| r.default_branch.as_str())
                                    .unwrap_or("");
                                if let Some(pos) = self
                                    .branches
                                    .iter()
                                    .position(|b| b.name == default_b)
                                    .or_else(|| (!self.branches.is_empty()).then_some(0))
                                {
                                    self.selected_branch_idx = pos;
                                    self.active_branch = self.branches[pos].name.clone();
                                }
                            } else if let Some(pos) =
                                self.branches.iter().position(|b| b.name == self.active_branch)
                            {
                                self.selected_branch_idx = pos;
                            }
                        }
                        Err(e) => {
                            self.push_system(true, format!("加载分支列表失败：{e}"));
                        }
                    }
                }
                BgEvent::FetchFinished { result } => {
                    self.fetching = false;
                    match result {
                        Ok((res, ms)) => {
                            self.online = Some(true);
                            self.last_rtt_ms = Some(ms);
                            if self.active_repo_name() == Some(res.repo.as_str()) {
                                self.branches = res.branches;
                                if let Some(pos) =
                                    self.branches.iter().position(|b| b.name == self.active_branch)
                                {
                                    self.selected_branch_idx = pos;
                                }
                            }
                            let icon = if res.fetched { "✅" } else { "ℹ️" };
                            self.push_system(
                                false,
                                format!(
                                    "{icon} Fetch `{}` ({ms}ms)：{}（当前可见 {} 个分支）",
                                    res.repo,
                                    res.message,
                                    self.branches.len()
                                ),
                            );
                            self.trigger_refresh_status();
                        }
                        Err(e) => {
                            self.push_system(true, format!("Fetch 失败：{e}"));
                        }
                    }
                }
                BgEvent::AiProgress { status } => {
                    if let Some(prev) = self.ai_progress.replace(status)
                        && self.ai_steps.last().map(String::as_str) != Some(prev.as_str())
                    {
                        self.ai_steps.push(prev);
                        if self.ai_steps.len() > 8 {
                            self.ai_steps.remove(0);
                        }
                    }
                    self.chat_scroll_from_bottom = 0;
                }
                BgEvent::AiFinished {
                    repo,
                    branch,
                    question,
                    result,
                } => {
                    self.querying = false;
                    self.query_started_at = None;
                    self.ai_progress = None;
                    self.ai_steps.clear();
                    match result {
                        Ok(answer) => {
                            self.online = Some(true);
                            if let Some(total) = answer.total_cards_indexed
                                && let Some(b) = self.branches.iter_mut().find(|b| b.name == branch)
                            {
                                b.indexed_cards = Some(total);
                            }
                            self.ai_history.push(ConversationTurn {
                                question,
                                answer: answer.content.clone(),
                            });
                            if self.ai_history.len() > 8 {
                                self.ai_history.remove(0);
                            }
                            let answer_id = self.alloc_answer_id();
                            self.messages.push(ChatItem::AiConclusion {
                                answer_id,
                                repo,
                                branch,
                                answer,
                            });
                            self.chat_scroll_from_bottom = 0;
                            self.trigger_refresh_status();
                        }
                        Err(e) => {
                            self.push_system(
                                true,
                                format!(
                                    "AI 问答失败：{e}\n提示：可使用 `/raw <关键词>` 直接查远端语义卡片，或检查 `distill configure-ai` / 环境变量配置。"
                                ),
                            );
                        }
                    }
                }
                BgEvent::SearchFinished { query, result } => {
                    self.querying = false;
                    self.query_started_at = None;
                    self.ai_progress = None;
                    self.ai_steps.clear();
                    match result {
                        Ok((payload, ms)) => {
                            self.online = Some(true);
                            self.last_rtt_ms = Some(ms);
                            if let Some(b) = self
                                .branches
                                .iter_mut()
                                .find(|b| b.name == payload.branch && b.commit_sha == payload.commit_sha)
                            {
                                b.indexed_cards = Some(payload.total_cards_indexed);
                            }
                            let answer_id = self.alloc_answer_id();
                            self.messages.push(ChatItem::SearchAnswer {
                                answer_id,
                                payload,
                                elapsed_ms: ms,
                                query,
                            });
                            self.chat_scroll_from_bottom = 0;
                            self.trigger_refresh_status();
                        }
                        Err(e) => {
                            self.push_system(true, format!("检索失败：{e}"));
                        }
                    }
                }
                BgEvent::DiffFinished { result } => {
                    self.querying = false;
                    self.query_started_at = None;
                    self.ai_progress = None;
                    self.ai_steps.clear();
                    match result {
                        Ok((diff, ms)) => {
                            self.online = Some(true);
                            self.last_rtt_ms = Some(ms);
                            let answer_id = self.alloc_answer_id();
                            self.messages.push(ChatItem::DiffAnswer {
                                answer_id,
                                diff,
                                elapsed_ms: ms,
                            });
                            self.chat_scroll_from_bottom = 0;
                            self.trigger_refresh_status();
                        }
                        Err(e) => {
                            self.push_system(true, format!("分支对比失败：{e}"));
                        }
                    }
                }
                BgEvent::RawBlockFinished { title, result } => {
                    self.querying = false;
                    self.query_started_at = None;
                    self.ai_progress = None;
                    self.ai_steps.clear();
                    match result {
                        Ok((content, ms)) => {
                            self.online = Some(true);
                            self.last_rtt_ms = Some(ms);
                            let answer_id = self.alloc_answer_id();
                            self.messages.push(ChatItem::RawBlock {
                                answer_id,
                                title,
                                content,
                                elapsed_ms: ms,
                            });
                            self.chat_scroll_from_bottom = 0;
                            self.trigger_refresh_status();
                        }
                        Err(e) => {
                            self.push_system(true, format!("执行失败：{e}"));
                        }
                    }
                }
            }
        }

        if let Some((_, _, t)) = &self.toast
            && t.elapsed() >= Duration::from_secs(4)
        {
            self.toast = None;
        }
    }

    fn open_ai_modal(&mut self) {
        let initial_idx = self
            .ai
            .as_ref()
            .and_then(|a| {
                let src = a.config().source_label.to_ascii_lowercase();
                AI_PRESETS.iter().position(|p| p.id == src)
            })
            .unwrap_or(0);
        self.ai_modal = Some(AiModalState::ChooseProvider { idx: initial_idx });
    }

    fn handle_ai_command(&mut self, arg: &str) {
        if arg.is_empty() {
            self.open_ai_modal();
            return;
        }
        let tokens: Vec<&str> = arg.split_whitespace().collect();
        if tokens[0].eq_ignore_ascii_case("list") {
            let mut lines = vec!["内置 AI Provider 预设（输入 `/ai` 打开交互选择器，或 `/ai <id> [model] [key]` 直接切换）：".to_string()];
            for p in AI_PRESETS {
                let (st, ok) = preset_readiness(p);
                lines.push(format!(
                    "• {:<10} {} — 默认模型 `{}` [{} {}]",
                    p.id,
                    p.name,
                    p.default_model,
                    if ok { "✓" } else { "·" },
                    st
                ));
            }
            self.push_system(false, lines.join("\n"));
            return;
        }
        let provider_id = tokens[0];
        let Some(preset) = find_preset(provider_id) else {
            self.push_system(
                true,
                format!("未知 AI Provider `{provider_id}`。输入 `/ai` 打开选择器，或 `/ai list` 查看全部预设。"),
            );
            return;
        };
        let model = tokens
            .get(1)
            .map(|s| s.to_string())
            .unwrap_or_else(|| preset.default_model.to_string());
        let api_key = tokens.get(2).map(|s| s.to_string());
        let base_url = (!preset.default_base_url.is_empty()).then(|| preset.default_base_url.to_string());
        self.apply_ai_config(preset.id, model, base_url, api_key, preset.default_key_env);
    }

    fn apply_ai_config(
        &mut self,
        provider_id: &str,
        model: String,
        base_url: Option<String>,
        api_key: Option<String>,
        default_key_env: &str,
    ) {
        let ai_cfg = AiFileConfig {
            provider: Some(provider_id.to_string()),
            base_url: base_url.filter(|s| !s.trim().is_empty()),
            model: (!model.trim().is_empty()).then_some(model.trim().to_string()),
            api_key: api_key.filter(|s| !s.trim().is_empty()),
            api_key_env: (!default_key_env.is_empty()).then(|| default_key_env.to_string()),
            cli_path: None,
        };
        if let Err(e) = save_ai_config(&self.cfg_path, ai_cfg.clone()) {
            self.set_toast(format!("保存 AI 配置失败: {e}"), true);
            return;
        }
        match ResolvedAiConfig::discover(Some(&ai_cfg))
            .and_then(|c| LocalAiAgent::new(c).ok())
            .map(Arc::new)
        {
            Some(agent) => {
                let badge = agent.config().display_badge();
                self.ai = Some(agent);
                self.ai_history.clear();
                self.set_toast(format!("✅ 已切换本地 AI: {badge}"), false);
                self.push_system(
                    false,
                    format!("🤖 已保存并切换到本地 AI：`{badge}`（写入 `{}`）", self.cfg_path.display()),
                );
            }
            None => {
                self.ai = None;
                self.set_toast(
                    format!("已保存 `{provider_id}`，但缺少有效 Key 或本机 CLI"),
                    true,
                );
                self.push_system(
                    true,
                    format!(
                        "已保存预设 `{provider_id}`，但尚未检测到可用的 API Key 或本机 CLI。请按 `/ai` 填入 API Key 或设置 `${default_key_env}`。"
                    ),
                );
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.ai_modal.is_some() {
            self.handle_ai_modal_key(key);
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') | KeyCode::Char('q') => {
                    self.should_quit = true;
                    return;
                }
                KeyCode::Char('p') => {
                    self.open_ai_modal();
                    return;
                }
                KeyCode::Char('y') => {
                    self.copy_answer(None);
                    return;
                }
                KeyCode::Char('s') => {
                    self.toggle_mouse_select_mode();
                    return;
                }
                KeyCode::Char('f') => {
                    self.trigger_fetch();
                    return;
                }
                KeyCode::Char('r') => {
                    self.trigger_refresh_status();
                    if let Some(repo) = self.active_repo_name().map(str::to_string) {
                        self.trigger_load_branches(repo);
                    }
                    return;
                }
                KeyCode::Char('l') => {
                    self.messages.clear();
                    self.ai_history.clear();
                    self.chat_scroll_from_bottom = 0;
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Esc => {
                if self.focus == Focus::Sidebar {
                    self.focus = Focus::Input;
                } else if !self.input.is_empty() {
                    self.input.clear();
                    self.cursor_byte = 0;
                } else {
                    self.should_quit = true;
                }
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Input => Focus::Sidebar,
                    Focus::Sidebar => Focus::Input,
                };
            }
            KeyCode::PageUp => {
                self.chat_scroll_from_bottom = self.chat_scroll_from_bottom.saturating_add(8);
            }
            KeyCode::PageDown => {
                self.chat_scroll_from_bottom = self.chat_scroll_from_bottom.saturating_sub(8);
            }
            _ => match self.focus {
                Focus::Sidebar => self.handle_sidebar_key(key),
                Focus::Input => self.handle_input_key(key),
            },
        }
    }

    fn handle_ai_modal_key(&mut self, key: KeyEvent) {
        let Some(mut modal) = self.ai_modal.take() else {
            return;
        };
        let is_ctrl_or_super = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::SUPER | KeyModifiers::META);

        if is_ctrl_or_super && matches!(key.code, KeyCode::Char('c' | 'q')) {
            self.should_quit = true;
            return;
        }

        match &mut modal {
            AiModalState::ChooseProvider { idx } => match key.code {
                KeyCode::Esc => {
                    return;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *idx = idx.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *idx + 1 < AI_PRESETS.len() {
                        *idx += 1;
                    }
                }
                KeyCode::Enter => {
                    let p = &AI_PRESETS[*idx];
                    modal = AiModalState::EditFields {
                        preset_idx: *idx,
                        field: 0,
                        model: p.default_model.to_string(),
                        base_url: p.default_base_url.to_string(),
                        api_key: String::new(),
                    };
                }
                _ => {}
            },
            AiModalState::EditFields {
                preset_idx,
                field,
                model,
                base_url,
                api_key,
            } => {
                let p = &AI_PRESETS[*preset_idx];
                let is_cli = p.wire == "codex" || p.wire == "claude";
                let max_field = if is_cli {
                    0
                } else if p.needs_api_key {
                    2
                } else {
                    1
                };

                if is_ctrl_or_super
                    && matches!(key.code, KeyCode::Char('u' | 'U') | KeyCode::Backspace)
                {
                    match *field {
                        0 => model.clear(),
                        1 => base_url.clear(),
                        _ => api_key.clear(),
                    }
                    self.ai_modal = Some(modal);
                    return;
                }

                match key.code {
                    KeyCode::Esc => {
                        modal = AiModalState::ChooseProvider { idx: *preset_idx };
                    }
                    KeyCode::Tab | KeyCode::Down => {
                        *field = if *field >= max_field { 0 } else { *field + 1 };
                    }
                    KeyCode::BackTab | KeyCode::Up => {
                        *field = if *field == 0 { max_field } else { *field - 1 };
                    }
                    KeyCode::Enter => {
                        if *field < max_field {
                            *field += 1;
                        } else {
                            let m = model.clone();
                            let u = (!is_cli).then(|| base_url.clone());
                            let k = (p.needs_api_key && !api_key.trim().is_empty())
                                .then(|| api_key.clone());
                            self.apply_ai_config(p.id, m, u, k, p.default_key_env);
                            return;
                        }
                    }
                    KeyCode::Backspace => match *field {
                        0 => {
                            model.pop();
                        }
                        1 => {
                            base_url.pop();
                        }
                        _ => {
                            api_key.pop();
                        }
                    },
                    KeyCode::Char(c) if !is_ctrl_or_super => match *field {
                        0 => model.push(c),
                        1 => base_url.push(c),
                        _ => api_key.push(c),
                    },
                    _ => {}
                }
            }
        }
        self.ai_modal = Some(modal);
    }

    fn handle_sidebar_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.branches.is_empty() {
                    self.selected_branch_idx = self.selected_branch_idx.saturating_sub(1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.branches.is_empty() {
                    self.selected_branch_idx =
                        (self.selected_branch_idx + 1).min(self.branches.len() - 1);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                self.select_branch_by_idx(self.selected_branch_idx);
                self.focus = Focus::Input;
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.trigger_fetch();
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.trigger_refresh_status();
                if let Some(repo) = self.active_repo_name().map(str::to_string) {
                    self.trigger_load_branches(repo);
                }
            }
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    fn handle_input_key(&mut self, key: KeyEvent) {
        let is_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let is_super = key.modifiers.contains(KeyModifiers::SUPER)
            || key.modifiers.contains(KeyModifiers::META);
        let is_alt = key.modifiers.contains(KeyModifiers::ALT);

        // Readline & macOS Cmd/Ctrl line-editing shortcuts
        if is_ctrl || is_super {
            match key.code {
                // Cmd+U / Ctrl+U / Cmd+Backspace: clear input to cursor (or entire line if at start)
                KeyCode::Char('u' | 'U') | KeyCode::Backspace => {
                    if self.cursor_byte > 0 {
                        self.input.drain(..self.cursor_byte);
                        self.cursor_byte = 0;
                    } else {
                        self.input.clear();
                    }
                    self.history_idx = None;
                    return;
                }
                // Ctrl+K: delete from cursor to end
                KeyCode::Char('k' | 'K') => {
                    self.input.truncate(self.cursor_byte);
                    return;
                }
                // Ctrl+W: delete previous word
                KeyCode::Char('w' | 'W') => {
                    self.delete_prev_word();
                    return;
                }
                // Ctrl+A / Cmd+Left: move cursor to beginning
                KeyCode::Char('a' | 'A') | KeyCode::Left => {
                    self.cursor_byte = 0;
                    return;
                }
                // Ctrl+E / Cmd+Right: move cursor to end
                KeyCode::Char('e' | 'E') | KeyCode::Right => {
                    self.cursor_byte = self.input.len();
                    return;
                }
                _ => return,
            }
        }

        if is_alt && key.code == KeyCode::Backspace {
            self.delete_prev_word();
            return;
        }

        match key.code {
            KeyCode::Enter => self.submit_input(),
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_byte, c);
                self.cursor_byte += c.len_utf8();
            }
            KeyCode::Backspace => {
                if self.cursor_byte > 0 {
                    let prev = prev_char_boundary(&self.input, self.cursor_byte);
                    self.input.drain(prev..self.cursor_byte);
                    self.cursor_byte = prev;
                }
            }
            KeyCode::Delete => {
                if self.cursor_byte < self.input.len() {
                    let next = next_char_boundary(&self.input, self.cursor_byte);
                    self.input.drain(self.cursor_byte..next);
                }
            }
            KeyCode::Left => {
                if self.cursor_byte > 0 {
                    self.cursor_byte = prev_char_boundary(&self.input, self.cursor_byte);
                }
            }
            KeyCode::Right => {
                if self.cursor_byte < self.input.len() {
                    self.cursor_byte = next_char_boundary(&self.input, self.cursor_byte);
                }
            }
            KeyCode::Home => self.cursor_byte = 0,
            KeyCode::End => self.cursor_byte = self.input.len(),
            KeyCode::Up => {
                if !self.input_history.is_empty() {
                    let next_idx = match self.history_idx {
                        None => self.input_history.len() - 1,
                        Some(i) => i.saturating_sub(1),
                    };
                    self.history_idx = Some(next_idx);
                    self.input = self.input_history[next_idx].clone();
                    self.cursor_byte = self.input.len();
                }
            }
            KeyCode::Down => {
                if let Some(i) = self.history_idx {
                    if i + 1 < self.input_history.len() {
                        let next_idx = i + 1;
                        self.history_idx = Some(next_idx);
                        self.input = self.input_history[next_idx].clone();
                        self.cursor_byte = self.input.len();
                    } else {
                        self.history_idx = None;
                        self.input.clear();
                        self.cursor_byte = 0;
                    }
                }
            }
            _ => {}
        }
    }

    fn delete_prev_word(&mut self) {
        if self.cursor_byte == 0 {
            return;
        }
        let mut idx = self.cursor_byte;
        // Skip trailing whitespace before cursor
        while idx > 0 {
            let prev = prev_char_boundary(&self.input, idx);
            if self.input[prev..idx].chars().all(char::is_whitespace) {
                idx = prev;
            } else {
                break;
            }
        }
        // Delete non-whitespace word characters
        while idx > 0 {
            let prev = prev_char_boundary(&self.input, idx);
            if !self.input[prev..idx].chars().all(char::is_whitespace) {
                idx = prev;
            } else {
                break;
            }
        }
        self.input.drain(idx..self.cursor_byte);
        self.cursor_byte = idx;
    }

    fn handle_paste(&mut self, text: String) {
        let single_line = text.replace(['\r', '\n'], " ").trim().to_string();
        if let Some(AiModalState::EditFields {
            field,
            model,
            base_url,
            api_key,
            ..
        }) = &mut self.ai_modal
        {
            match *field {
                0 => model.push_str(&single_line),
                1 => base_url.push_str(&single_line),
                _ => api_key.push_str(&single_line),
            }
            return;
        }
        self.input.insert_str(self.cursor_byte, &single_line);
        self.cursor_byte += single_line.len();
        self.focus = Focus::Input;
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        let col = mouse.column;
        let row = mouse.row;

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // 1. Per-answer [📋 复制] buttons inside chat pane
                if let Some((_, ans_id)) = self
                    .regions
                    .copy_answer_btns
                    .iter()
                    .find(|(rect, _)| rect_contains(*rect, col, row))
                    .copied()
                {
                    self.copy_answer(Some(ans_id));
                    return;
                }

                if rect_contains(self.regions.copy_latest_btn, col, row) {
                    self.copy_answer(None);
                } else if rect_contains(self.regions.select_mode_btn, col, row) {
                    self.toggle_mouse_select_mode();
                } else if rect_contains(self.regions.fetch_btn, col, row) {
                    self.trigger_fetch();
                } else if rect_contains(self.regions.refresh_branches_btn, col, row) {
                    if let Some(repo) = self.active_repo_name().map(str::to_string) {
                        self.trigger_load_branches(repo);
                    }
                } else if rect_contains(self.regions.refresh_status_btn, col, row) {
                    self.trigger_refresh_status();
                } else if rect_contains(self.regions.repo_header, col, row) && self.repos.len() > 1 {
                    self.active_repo_idx = (self.active_repo_idx + 1) % self.repos.len();
                    self.active_branch.clear();
                    self.branches.clear();
                    self.ai_history.clear();
                    if let Some(repo) = self.active_repo_name().map(str::to_string) {
                        self.trigger_load_branches(repo);
                    }
                } else if rect_contains(self.regions.branch_list_area, col, row) {
                    let rel_y = (row - self.regions.branch_list_area.y) as usize;
                    let idx = self.branch_scroll + (rel_y / 2);
                    if idx < self.branches.len() {
                        self.select_branch_by_idx(idx);
                        self.focus = Focus::Input;
                    }
                } else if rect_contains(self.regions.input_area, col, row) {
                    self.focus = Focus::Input;
                }
            }
            MouseEventKind::ScrollUp => {
                if rect_contains(self.regions.branch_list_area, col, row) {
                    self.branch_scroll = self.branch_scroll.saturating_sub(1);
                } else if rect_contains(self.regions.chat_area, col, row) {
                    self.chat_scroll_from_bottom = self.chat_scroll_from_bottom.saturating_add(3);
                }
            }
            MouseEventKind::ScrollDown => {
                if rect_contains(self.regions.branch_list_area, col, row) {
                    if self.branch_scroll + 1 < self.branches.len() {
                        self.branch_scroll += 1;
                    }
                } else if rect_contains(self.regions.chat_area, col, row) {
                    self.chat_scroll_from_bottom = self.chat_scroll_from_bottom.saturating_sub(3);
                }
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let size = frame.area();
        let sidebar_w = if size.width >= 112 {
            38
        } else {
            (size.width * 34 / 100).max(28)
        };

        let root = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(sidebar_w), Constraint::Min(32)])
            .split(size);

        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(14),
                Constraint::Min(10),
                Constraint::Length(5),
            ])
            .split(root[0]);

        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(4),
            ])
            .split(root[1]);

        self.draw_remote_status(frame, left[0]);
        self.draw_branches_panel(frame, left[1]);
        self.draw_shortcuts_panel(frame, left[2]);
        self.draw_chat_header(frame, right[0]);
        self.draw_chat_history(frame, right[1]);
        self.draw_input_box(frame, right[2]);
        if self.ai_modal.is_some() {
            self.draw_ai_modal(frame, size);
        }
    }

    fn draw_ai_modal(&self, frame: &mut Frame, area: Rect) {
        let Some(ref modal) = self.ai_modal else {
            return;
        };
        let modal_w = area.width.saturating_sub(8).clamp(52, 78);
        let modal_h = area.height.saturating_sub(4).clamp(16, 22);
        let rect = Rect {
            x: area.x + area.width.saturating_sub(modal_w) / 2,
            y: area.y + area.height.saturating_sub(modal_h) / 2,
            width: modal_w,
            height: modal_h,
        };
        frame.render_widget(Clear, rect);

        let block = Block::default()
            .title(Span::styled(
                " 🤖 本地 AI Provider 设置 (类似 pi /model) ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);

        let mut lines = Vec::new();
        match modal {
            AiModalState::ChooseProvider { idx } => {
                lines.push(Line::from(Span::styled(
                    "选择 AI Provider（↑/↓ 移动 · Enter 下一步 · Esc 关闭）：",
                    Style::default().fg(Color::Reset),
                )));
                lines.push(Line::raw(""));
                for (i, p) in AI_PRESETS.iter().enumerate() {
                    let selected = i == *idx;
                    let (st, ok) = preset_readiness(p);
                    let row_mod = if selected {
                        Modifier::BOLD | Modifier::REVERSED
                    } else {
                        Modifier::empty()
                    };
                    let prefix = if selected { "▶ " } else { "  " };
                    let id_pad = format!("{:<11}", p.id);
                    let name_pad = format!("{:<22}", truncate_width(p.name, 22));
                    let st_color = if ok { Color::Green } else { Color::Yellow };
                    lines.push(Line::from(vec![
                        Span::styled(
                            prefix,
                            Style::default().fg(Color::Blue).add_modifier(row_mod | Modifier::BOLD),
                        ),
                        Span::styled(
                            id_pad,
                            Style::default()
                                .fg(if selected { Color::Blue } else { Color::Reset })
                                .add_modifier(row_mod | Modifier::BOLD),
                        ),
                        Span::styled(name_pad, Style::default().fg(Color::Reset).add_modifier(row_mod)),
                        Span::styled(" ", Style::default().add_modifier(row_mod)),
                        Span::styled(
                            format!("{} {}", if ok { "✓" } else { "·" }, st),
                            Style::default().fg(st_color).add_modifier(row_mod),
                        ),
                    ]));
                }
                if let Some(p) = AI_PRESETS.get(*idx) {
                    lines.push(Line::raw(""));
                    lines.push(Line::from(vec![
                        Span::styled("说明: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{} · 默认模型 {}", p.desc, p.default_model),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));
                }
            }
            AiModalState::EditFields {
                preset_idx,
                field,
                model,
                base_url,
                api_key,
            } => {
                let p = &AI_PRESETS[*preset_idx];
                let is_cli = p.wire == "codex" || p.wire == "claude";
                lines.push(Line::from(vec![
                    Span::styled("Provider: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{} ({})", p.name, p.id),
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
                lines.push(Line::from(Span::styled(
                    p.desc,
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::raw(""));

                let render_field = |idx: usize, label: &str, val: &str, hint: &str| {
                    let active = *field == idx;
                    let fg = if active { Color::Blue } else { Color::Reset };
                    let val_mod = if active {
                        Modifier::UNDERLINED | Modifier::BOLD
                    } else {
                        Modifier::empty()
                    };
                    let shown = if val.is_empty() {
                        Span::styled(hint.to_string(), Style::default().fg(Color::DarkGray))
                    } else {
                        Span::styled(
                            val.to_string(),
                            Style::default().fg(Color::Reset).add_modifier(val_mod),
                        )
                    };
                    Line::from(vec![
                        Span::styled(
                            if active { "▶ " } else { "  " },
                            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{label:<10}: "),
                            Style::default().fg(fg).add_modifier(Modifier::BOLD),
                        ),
                        shown,
                    ])
                };

                lines.push(render_field(
                    0,
                    "Model",
                    model,
                    p.default_model,
                ));
                if !is_cli {
                    lines.push(Line::raw(""));
                    lines.push(render_field(
                        1,
                        "Base URL",
                        base_url,
                        p.default_base_url,
                    ));
                    if p.needs_api_key {
                        lines.push(Line::raw(""));
                        let masked = if api_key.is_empty() {
                            String::new()
                        } else {
                            "•".repeat(api_key.chars().count().min(32))
                        };
                        let env_hint = if p.default_key_env.is_empty() {
                            "粘贴 API Key（Cmd+V）".to_string()
                        } else {
                            format!("留空则自动读 ${}，或直接 Cmd+V 粘贴", p.default_key_env)
                        };
                        lines.push(render_field(2, "API Key", &masked, &env_hint));
                    }
                }

                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    "Tab / ↑↓ 切换字段 · Cmd/Ctrl+U 清空当前栏 · Enter 下一步/保存 · Esc 返回列表",
                    Style::default().fg(Color::Yellow),
                )));
                lines.push(Line::from(Span::styled(
                    format!("配置将加密权限(0600)写入：{}", self.cfg_path.display()),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    }

    fn draw_remote_status(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(
                " 📡 远端与 AI 状态 ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (dot_str, dot_color) = match self.online {
            Some(true) => ("● 在线", Color::Green),
            Some(false) => ("● 离线", Color::Red),
            None => ("◌ 连接中", Color::Yellow),
        };
        let rtt_str = self
            .last_rtt_ms
            .map(|ms| format!(" {ms}ms"))
            .unwrap_or_default();

        let btn_label = if self.status_loading {
            format!("[{}]", SPINNER[self.tick % SPINNER.len()])
        } else {
            "[↻刷新]".to_string()
        };
        let btn_w = btn_label.width() as u16;
        if inner.width > btn_w + 2 && inner.height > 0 {
            self.regions.refresh_status_btn = Rect {
                x: inner.x + inner.width.saturating_sub(btn_w),
                y: inner.y,
                width: btn_w,
                height: 1,
            };
        }

        let max_w = inner.width as usize;
        let mut lines = Vec::new();
        let left_status = format!("{dot_str}{rtt_str}");
        let pad_len = inner
            .width
            .saturating_sub(left_status.width() as u16 + btn_w) as usize;
        lines.push(Line::from(vec![
            Span::styled(dot_str, Style::default().fg(dot_color).add_modifier(Modifier::BOLD)),
            Span::styled(rtt_str, Style::default().fg(Color::DarkGray)),
            Span::raw(" ".repeat(pad_len)),
            Span::styled(btn_label, Style::default().fg(Color::Cyan)),
        ]));

        // Local AI status line
        if let Some(ref agent) = self.ai {
            lines.push(Line::from(vec![
                Span::styled("AI  : ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    truncate_width(&agent.config().display_badge(), max_w.saturating_sub(6)),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("AI  : ", Style::default().fg(Color::DarkGray)),
                Span::styled("未配置 (仅原始检索)", Style::default().fg(Color::Yellow)),
            ]));
        }

        lines.push(Line::from(vec![
            Span::styled("节点: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_width(&self.server_hint, max_w.saturating_sub(6)),
                Style::default().fg(Color::Reset),
            ),
        ]));

        if let Some(st) = &self.server_status {
            lines.push(Line::from(vec![
                Span::styled("身份: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} · v{}", st.peer_label, st.server_version),
                    Style::default().fg(Color::Reset),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("运行: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format_uptime(st.uptime_secs), Style::default().fg(Color::Green)),
                Span::styled("  QUIC ", Style::default().fg(Color::DarkGray)),
                Span::styled("iroh", Style::default().fg(Color::Cyan)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("索引: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} 分支", st.cached_branch_indexes),
                    Style::default().fg(Color::Blue),
                ),
                Span::styled("  WT: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", st.active_worktrees),
                    Style::default().fg(Color::Reset),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("请求: ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}", st.total_requests), Style::default().fg(Color::Reset)),
                Span::styled("  DLP脱敏: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{}", st.total_dlp_redactions),
                    Style::default().fg(Color::Magenta),
                ),
            ]));
            if let Some(ep) = &st.vlogs_endpoint {
                let host = ep
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .split('/')
                    .next()
                    .unwrap_or(ep);
                lines.push(Line::from(vec![
                    Span::styled("日志: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        truncate_width(&format!("{host} ✓"), max_w.saturating_sub(6)),
                        Style::default().fg(Color::Green),
                    ),
                ]));
            }

            if let Some(repo_name) = self.active_repo_name()
                && let Some(rs) = st.repos.iter().find(|r| r.name == repo_name)
            {
                let mode_cn = if rs.mode == "local_mirror" {
                    "镜像"
                } else {
                    "远端"
                };
                let sync_age = rs
                    .last_fetch_secs_ago
                    .map(|s| format!(" · {}前", format_uptime(s)))
                    .unwrap_or_default();
                let sync_color = if rs.last_fetch_ok {
                    Color::Green
                } else {
                    Color::Yellow
                };
                lines.push(Line::from(vec![
                    Span::styled("同步: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{mode_cn}{sync_age}"), Style::default().fg(sync_color)),
                ]));
                lines.push(Line::from(Span::styled(
                    truncate_width(&rs.last_fetch_note, max_w),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        } else if let Some(err) = &self.last_error {
            lines.push(Line::from(Span::styled(
                truncate_width(&format!("错误: {err}"), max_w),
                Style::default().fg(Color::Red),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "正在握手并拉取服务状态…",
                Style::default().fg(Color::DarkGray),
            )));
        }

        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_branches_panel(&mut self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focus == Focus::Sidebar {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        let block = Block::default()
            .title(Span::styled(
                " 📦 仓库与分支 ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.height < 4 || inner.width < 10 {
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(2),
            ])
            .split(inner);

        self.regions.repo_header = chunks[0];
        let repo_label = self.active_repo_name().unwrap_or("(加载中)");
        let repo_hint = if self.repos.len() > 1 {
            format!(" ({}/{}) ▾", self.active_repo_idx + 1, self.repos.len())
        } else {
            format!(" ({} 分支)", self.branches.len())
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("仓库: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    repo_label,
                    Style::default()
                        .fg(Color::Reset)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(repo_hint, Style::default().fg(Color::DarkGray)),
            ])),
            chunks[0],
        );

        let fetch_label = if self.fetching {
            format!(" {} Fetching… ", SPINNER[self.tick % SPINNER.len()])
        } else {
            " ⟳ Fetch ".to_string()
        };
        let refresh_label = " ↻ 刷新 ".to_string();
        let fetch_w = fetch_label.width() as u16;
        let refresh_w = refresh_label.width() as u16;

        self.regions.fetch_btn = Rect {
            x: chunks[1].x,
            y: chunks[1].y,
            width: fetch_w.min(chunks[1].width),
            height: 1,
        };
        self.regions.refresh_branches_btn = Rect {
            x: chunks[1].x + fetch_w + 1,
            y: chunks[1].y,
            width: refresh_w.min(chunks[1].width.saturating_sub(fetch_w + 1)),
            height: 1,
        };

        let fetch_style = if self.fetching {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        let refresh_style = Style::default().fg(Color::White).bg(Color::DarkGray);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(fetch_label, fetch_style),
                Span::raw(" "),
                Span::styled(refresh_label, refresh_style),
            ])),
            chunks[1],
        );

        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "─".repeat(chunks[2].width as usize),
                Style::default().fg(Color::DarkGray),
            ))),
            chunks[2],
        );

        self.regions.branch_list_area = chunks[3];
        let visible_slots = (chunks[3].height as usize) / 2;
        if visible_slots > 0 && self.selected_branch_idx >= self.branch_scroll + visible_slots {
            self.branch_scroll = self.selected_branch_idx + 1 - visible_slots;
        }
        if self.selected_branch_idx < self.branch_scroll {
            self.branch_scroll = self.selected_branch_idx;
        }

        let max_w = chunks[3].width as usize;
        let mut list_lines = Vec::new();
        if self.branches.is_empty() {
            list_lines.push(Line::from(Span::styled(
                "  (暂无可见分支)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (idx, b) in self
                .branches
                .iter()
                .enumerate()
                .skip(self.branch_scroll)
                .take(visible_slots.max(1))
            {
                let is_active = b.name == self.active_branch;
                let is_selected = idx == self.selected_branch_idx && self.focus == Focus::Sidebar;
                let prefix = if is_active { "▶ " } else { "  " };
                let sha = short_sha(&b.commit_sha);
                let badge = match b.indexed_cards {
                    Some(n) => format!("[{n}卡]"),
                    None => "[未索引]".to_string(),
                };
                let badge_color = if b.indexed_cards.is_some() {
                    Color::Green
                } else {
                    Color::DarkGray
                };

                let used = prefix.width() + 1 + sha.width() + 1 + badge.width();
                let name_budget = max_w.saturating_sub(used).max(6);
                let name_disp = truncate_width(&b.name, name_budget);
                let name_style = if is_selected {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                } else if is_active {
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Reset)
                };

                list_lines.push(Line::from(vec![
                    Span::styled(prefix, name_style),
                    Span::styled(name_disp, name_style),
                    Span::raw(" "),
                    Span::styled(sha, Style::default().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(badge, Style::default().fg(badge_color)),
                ]));

                let summary = if b.commit_summary.is_empty() {
                    "(无提交说明)"
                } else {
                    &b.commit_summary
                };
                list_lines.push(Line::from(Span::styled(
                    format!("  {}", truncate_width(summary, max_w.saturating_sub(2))),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        frame.render_widget(Paragraph::new(list_lines), chunks[3]);
    }

    fn draw_shortcuts_panel(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(Span::styled(
                " ⌨ 快捷键 ",
                Style::default().fg(Color::DarkGray),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let lines = vec![
            Line::from(Span::styled(
                "Ctrl+Y 复制 · Ctrl+S 切点击/选字",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                "Ctrl+P 配AI · Ctrl+F Fetch",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                "Tab 切栏 · Cmd/Ctrl+U 清行 · Esc退",
                Style::default().fg(Color::DarkGray),
            )),
        ];
        frame.render_widget(Paragraph::new(lines), inner);
    }

    fn draw_chat_header(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let repo = self.active_repo_name().unwrap_or("…").to_string();
        let branch = if self.active_branch.is_empty() {
            "未选择".to_string()
        } else {
            self.active_branch.clone()
        };
        let sha = self
            .active_branch_info()
            .map(|b| format!(" ({})", short_sha(&b.commit_sha)))
            .unwrap_or_default();

        let copy_label = " 📋 复制最新 ".to_string();
        let select_label = if self.mouse_select_mode {
            " 🖱 选字:开 ".to_string()
        } else {
            " 🖱 选字:关 ".to_string()
        };
        let copy_w = copy_label.width() as u16;
        let select_w = select_label.width() as u16;
        let right_btns_w = copy_w + 1 + select_w;

        // Place right-aligned clickable buttons
        if inner.width > right_btns_w + 12 {
            let select_x = inner.x + inner.width - select_w;
            let copy_x = select_x.saturating_sub(copy_w + 1);
            self.regions.copy_latest_btn = Rect {
                x: copy_x,
                y: inner.y,
                width: copy_w,
                height: 1,
            };
            self.regions.select_mode_btn = Rect {
                x: select_x,
                y: inner.y,
                width: select_w,
                height: 1,
            };
        } else {
            self.regions.copy_latest_btn = Rect::default();
            self.regions.select_mode_btn = Rect::default();
        }

        let mut left_spans = vec![
            Span::styled(
                " Distill AI ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{repo}@{branch}"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(sha, Style::default().fg(Color::DarkGray)),
        ];

        if let Some((msg, is_err, _)) = &self.toast {
            left_spans.push(Span::raw("  "));
            left_spans.push(Span::styled(
                msg.clone(),
                Style::default()
                    .fg(if *is_err { Color::Red } else { Color::Green })
                    .add_modifier(Modifier::BOLD),
            ));
        } else if self.fetching {
            left_spans.push(Span::raw("  "));
            left_spans.push(Span::styled(
                format!("{} 远端 Fetch 中…", SPINNER[self.tick % SPINNER.len()]),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let left_w: usize = left_spans.iter().map(|s| s.content.width()).sum();
        let budget = (inner.width as usize).saturating_sub(right_btns_w as usize + 1);
        let pad = budget.saturating_sub(left_w);

        let mut spans = left_spans;
        if inner.width > right_btns_w + 12 {
            spans.push(Span::raw(" ".repeat(pad.max(1))));
            spans.push(Span::styled(
                copy_label,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                select_label,
                if self.mouse_select_mode {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White).bg(Color::DarkGray)
                },
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    }

    fn draw_chat_history(&mut self, frame: &mut Frame, area: Rect) {
        self.regions.chat_area = area;
        self.regions.copy_answer_btns.clear();

        let title = if self.mouse_select_mode {
            " 💬 对话记录 (鼠标可直接划选复制 · Ctrl+Y 复制最新 · Ctrl+S 切点击模式) "
        } else {
            " 💬 对话记录 [点击模式：可点分支/Fetch/复制按钮 · Ctrl+S 切回划选] "
        };
        let block = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::Reset)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let wrap_w = inner.width.max(20) as usize;
        let mut lines: Vec<Line<'static>> = Vec::new();
        // Track (line_index_in_buffer, col_offset, width, answer_id) for per-answer [📋 复制] buttons
        let mut btn_anchors: Vec<(usize, u16, u16, usize)> = Vec::new();

        for item in &self.messages {
            render_chat_item(item, wrap_w, &mut lines, &mut btn_anchors);
            lines.push(Line::raw(""));
        }

        // IM-style live thinking bubble right underneath the user's message
        if self.querying {
            let spin = SPINNER[self.tick % SPINNER.len()];
            let elapsed = self
                .query_started_at
                .map(|t| format!("{:.1}s", t.elapsed().as_secs_f64()))
                .unwrap_or_else(|| "0.0s".to_string());
            let dots = match (self.tick / 4) % 4 {
                0 => "",
                1 => ".",
                2 => "..",
                _ => "...",
            };
            let ai_label = self
                .ai
                .as_ref()
                .map(|a| a.config().model.clone())
                .unwrap_or_else(|| "Distill".to_string());

            lines.push(Line::from(vec![
                Span::styled(
                    format!("◆ {ai_label} "),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("思考中{dots} ({elapsed})"),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));

            for done_step in &self.ai_steps {
                for w in wrap_text(done_step, wrap_w.saturating_sub(4)) {
                    lines.push(Line::from(vec![
                        Span::styled("  ✓ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(w, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }

            let cur_step = self
                .ai_progress
                .as_deref()
                .unwrap_or("正在检索远端语义并生成结论…");
            for (i, w) in wrap_text(cur_step, wrap_w.saturating_sub(4))
                .into_iter()
                .enumerate()
            {
                let prefix = if i == 0 {
                    format!("  {spin} ")
                } else {
                    "    ".to_string()
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        prefix,
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        w,
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            lines.push(Line::raw(""));
        }

        let total_lines = lines.len();
        let viewport_h = inner.height as usize;
        let max_scroll = total_lines.saturating_sub(viewport_h);
        if self.chat_scroll_from_bottom > max_scroll {
            self.chat_scroll_from_bottom = max_scroll;
        }
        let scroll_top = max_scroll.saturating_sub(self.chat_scroll_from_bottom);

        // Compute visible screen coordinates for each answer's [📋 复制] button
        for (line_idx, col_off, btn_w, ans_id) in btn_anchors {
            if line_idx >= scroll_top && line_idx < scroll_top + viewport_h {
                let screen_y = inner.y + (line_idx - scroll_top) as u16;
                let screen_x = inner.x + col_off;
                self.regions.copy_answer_btns.push((
                    Rect {
                        x: screen_x,
                        y: screen_y,
                        width: btn_w,
                        height: 1,
                    },
                    ans_id,
                ));
            }
        }

        let para = Paragraph::new(lines).scroll((scroll_top as u16, 0));
        frame.render_widget(para, inner);
    }

    fn draw_input_box(&mut self, frame: &mut Frame, area: Rect) {
        self.regions.input_area = area;
        let focused = self.focus == Focus::Input;
        let border_color = if focused { Color::Cyan } else { Color::DarkGray };
        let mode_tag = if self.ai.is_some() {
            "自然语言 AI 问答"
        } else {
            "语义卡片检索"
        };
        let title = if focused {
            format!(" ✎ 提问输入框 [{mode_tag}] (Enter 发送 · /ai 切换模型 · Ctrl+Y 复制 · /help) ")
        } else {
            " ✎ 提问输入框 (按 Tab 返回输入) ".to_string()
        };
        let block = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(if focused { Color::Cyan } else { Color::DarkGray })
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.input.is_empty() {
            let placeholder = if self.ai.is_some() {
                "直接问业务问题，例如：通过方案号怎么查 workItemId，价值点集测1轮字段是什么？"
            } else {
                "输入 /ai 或按 Ctrl+P 选择本地 AI Provider，或直接输入关键词检索"
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    placeholder,
                    Style::default().fg(Color::DarkGray),
                )))
                .wrap(Wrap { trim: false }),
                inner,
            );
        } else {
            frame.render_widget(
                Paragraph::new(self.input.as_str()).wrap(Wrap { trim: false }),
                inner,
            );
        }

        if focused && self.ai_modal.is_none() && inner.width > 0 && inner.height > 0 {
            let prefix = &self.input[..self.cursor_byte];
            let display_w = prefix.width() as u16;
            let cx = inner.x + (display_w % inner.width);
            let cy = inner.y + (display_w / inner.width).min(inner.height.saturating_sub(1));
            frame.set_cursor_position((cx, cy));
        }
    }
}

fn push_answer_header_with_copy(
    left_spans: Vec<Span<'static>>,
    answer_id: usize,
    max_w: usize,
    out: &mut Vec<Line<'static>>,
    btn_anchors: &mut Vec<(usize, u16, u16, usize)>,
) {
    let copy_btn = format!(" 📋 复制#{answer_id} ");
    let btn_w = copy_btn.width();
    let left_w: usize = left_spans.iter().map(|s| s.content.width()).sum();

    let mut spans = left_spans;
    let line_idx = out.len();
    if left_w + btn_w + 1 <= max_w {
        let pad = max_w - left_w - btn_w;
        let col_off = (left_w + pad) as u16;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(
            copy_btn,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        btn_anchors.push((line_idx, col_off, btn_w as u16, answer_id));
    } else {
        spans.push(Span::raw(" "));
        let col_off = (left_w + 1) as u16;
        spans.push(Span::styled(
            copy_btn,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        btn_anchors.push((line_idx, col_off, btn_w as u16, answer_id));
    }
    out.push(Line::from(spans));
}

fn render_chat_item(
    item: &ChatItem,
    max_w: usize,
    out: &mut Vec<Line<'static>>,
    btn_anchors: &mut Vec<(usize, u16, u16, usize)>,
) {
    match item {
        ChatItem::User { repo, branch, text } => {
            out.push(Line::from(vec![
                Span::styled(
                    "▸ 你 ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{repo} @ {branch}]"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            for wrapped in wrap_text(text, max_w.saturating_sub(2)) {
                out.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        wrapped,
                        Style::default()
                            .fg(Color::Reset)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
        }
        ChatItem::AiConclusion {
            answer_id,
            repo,
            branch,
            answer,
        } => {
            let meta = format!(
                "{repo}@{branch} · {} · {:.1}s",
                answer.model,
                answer.elapsed_ms as f64 / 1000.0
            );
            push_answer_header_with_copy(
                vec![
                    Span::styled(
                        "◆ AI 结论 ",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(meta, Style::default().fg(Color::DarkGray)),
                ],
                *answer_id,
                max_w,
                out,
                btn_anchors,
            );

            crate::markdown::render_markdown(&answer.content, max_w, out);

            let tools_desc = if answer.tool_calls.is_empty() {
                "直接基于上下文".to_string()
            } else {
                answer.tool_calls.join(" · ")
            };
            let footer = format!(
                "  📚 依据：探索/整合 {} 项代码与日志证据（{}）· DLP 脱敏 {} 处",
                answer.cards_consulted, tools_desc, answer.dlp_redactions
            );
            for wrapped in wrap_text(&footer, max_w) {
                out.push(Line::from(Span::styled(
                    wrapped,
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        ChatItem::SearchAnswer {
            answer_id,
            payload,
            elapsed_ms,
            query,
        } => {
            push_answer_header_with_copy(
                vec![
                    Span::styled(
                        "◆ 语义卡片 ",
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "{}@{} · 命中 {}/{} · {}ms",
                            payload.repo,
                            payload.branch,
                            payload.hits.len(),
                            payload.total_cards_indexed,
                            elapsed_ms
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ],
                *answer_id,
                max_w,
                out,
                btn_anchors,
            );
            if payload.hits.is_empty() {
                out.push(Line::from(Span::styled(
                    format!("  未检索到与 `{query}` 匹配的语义卡。"),
                    Style::default().fg(Color::Yellow),
                )));
            } else {
                for (i, card) in payload.hits.iter().enumerate() {
                    render_card(i + 1, card, max_w, out);
                }
            }
        }
        ChatItem::DiffAnswer {
            answer_id,
            diff,
            elapsed_ms,
        } => {
            push_answer_header_with_copy(
                vec![
                    Span::styled(
                        "◆ 分支语义差异 ",
                        Style::default()
                            .fg(Color::Magenta)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "{} → {} · {}ms",
                            diff.base_branch, diff.target_branch, elapsed_ms
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ],
                *answer_id,
                max_w,
                out,
                btn_anchors,
            );
            out.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("+ 新增 {}", diff.added.len()),
                    Style::default().fg(Color::Green),
                ),
                Span::raw("   "),
                Span::styled(
                    format!("~ 变更 {}", diff.modified.len()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("   "),
                Span::styled(
                    format!("- 移除 {}", diff.removed_titles.len()),
                    Style::default().fg(Color::Red),
                ),
            ]));
            for c in diff.added.iter().take(12) {
                out.push(Line::from(vec![
                    Span::styled("  + ", Style::default().fg(Color::Green)),
                    Span::styled(
                        format!("[{}] {}", kind_label(c.kind), c.title),
                        Style::default().fg(Color::Reset),
                    ),
                ]));
            }
            for c in diff.modified.iter().take(12) {
                out.push(Line::from(vec![
                    Span::styled("  ~ ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!("[{}] {}", kind_label(c.kind), c.title),
                        Style::default().fg(Color::Reset),
                    ),
                ]));
            }
            for t in diff.removed_titles.iter().take(10) {
                out.push(Line::from(vec![
                    Span::styled("  - ", Style::default().fg(Color::Red)),
                    Span::styled(t.clone(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
        ChatItem::RawBlock {
            answer_id,
            title,
            content,
            elapsed_ms,
        } => {
            push_answer_header_with_copy(
                vec![
                    Span::styled(
                        format!("◆ {title} "),
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{elapsed_ms}ms"), Style::default().fg(Color::DarkGray)),
                ],
                *answer_id,
                max_w,
                out,
                btn_anchors,
            );
            for raw_line in content.lines() {
                for wrapped in wrap_text(raw_line, max_w.saturating_sub(2)) {
                    out.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(wrapped, Style::default().fg(Color::Reset)),
                    ]));
                }
            }
        }
        ChatItem::System { is_error, text } => {
            let color = if *is_error { Color::Red } else { Color::Reset };
            for raw_line in text.lines() {
                for wrapped in wrap_text(raw_line, max_w) {
                    out.push(Line::from(Span::styled(wrapped, Style::default().fg(color))));
                }
            }
        }
    }
}

fn render_card(idx: usize, card: &SemanticCard, max_w: usize, out: &mut Vec<Line<'static>>) {
    let (badge, color) = kind_badge(card.kind);
    out.push(Line::from(vec![
        Span::styled(format!("  {idx}. "), Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("[{badge}] "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            card.title.clone(),
            Style::default()
                .fg(Color::Reset)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  #{}", card.domain),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    if !card.summary.is_empty() {
        for wrapped in wrap_text(&card.summary, max_w.saturating_sub(5)) {
            out.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(wrapped, Style::default().fg(Color::Reset)),
            ]));
        }
    }

    if !card.metadata.is_empty() {
        let kv = card
            .metadata
            .iter()
            .take(5)
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" · ");
        for wrapped in wrap_text(&kv, max_w.saturating_sub(5)) {
            out.push(Line::from(vec![
                Span::raw("     "),
                Span::styled(wrapped, Style::default().fg(Color::DarkGray)),
            ]));
        }
    }
}

fn kind_badge(kind: SemanticKind) -> (&'static str, Color) {
    match kind {
        SemanticKind::ApiEndpoint => ("API接口", Color::Magenta),
        SemanticKind::DtoSchema => ("DTO字段", Color::Blue),
        SemanticKind::ErrorContract => ("错误契约", Color::Red),
        SemanticKind::DomainRule => ("业务规则", Color::Yellow),
        SemanticKind::EntitySchema => ("数据表", Color::Cyan),
        SemanticKind::MigrationFact => ("迁移", Color::Green),
        SemanticKind::DocSection => ("规格文档", Color::DarkGray),
        SemanticKind::ConstantCatalog => ("常量枚举", Color::Blue),
    }
}

fn kind_label(kind: SemanticKind) -> &'static str {
    kind_badge(kind).0
}

fn short_sha(sha: &str) -> String {
    sha.chars().take(8).collect()
}

fn format_uptime(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    rect.width > 0
        && rect.height > 0
        && col >= rect.x
        && col < rect.x + rect.width
        && row >= rect.y
        && row < rect.y + rect.height
}

fn prev_char_boundary(s: &str, byte_idx: usize) -> usize {
    let mut i = byte_idx.saturating_sub(1);
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, byte_idx: usize) -> usize {
    let mut i = (byte_idx + 1).min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn truncate_width(s: &str, max_w: usize) -> String {
    if max_w == 0 {
        return String::new();
    }
    if s.width() <= max_w {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0usize;
    let limit = max_w.saturating_sub(1);
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > limit {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

fn wrap_text(s: &str, max_w: usize) -> Vec<String> {
    if max_w <= 4 {
        return vec![s.to_string()];
    }
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    for ch in s.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
            continue;
        }
        let cw = ch.width().unwrap_or(0);
        if cur_w + cw > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += cw;
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

struct TerminalGuard {
    kbd_enhanced: bool,
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        if self.kbd_enhanced {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste
        );
    }
}

fn format_commit_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 19 && trimmed.as_bytes().get(10) == Some(&b'T') {
        let mut s = trimmed.to_string();
        s.replace_range(10..11, " ");
        s
    } else {
        trimmed.to_string()
    }
}

fn parse_vlogs_slash_args(arg: &str) -> (Option<String>, String) {
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return (None, "*".to_string());
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    let is_duration = first.len() >= 2
        && first.len() <= 5
        && matches!(first.as_bytes().last(), Some(b's' | b'm' | b'h' | b'd'))
        && first[..first.len() - 1]
            .bytes()
            .all(|b| b.is_ascii_digit());
    if is_duration {
        let q = if rest.is_empty() {
            "*".to_string()
        } else {
            rest.to_string()
        };
        (Some(first.to_string()), q)
    } else {
        (None, trimmed.to_string())
    }
}

pub(crate) async fn run_tui(
    client: Arc<P2pClient>,
    ai: Option<Arc<LocalAiAgent>>,
    cfg_path: PathBuf,
    server_hint: String,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Default to select mode ON (do not capture mouse on startup so native drag-select works immediately;
    // user can press Ctrl+S anytime to toggle mouse click mode).
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let kbd_enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if kbd_enhanced {
        let _ = execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    let _guard = TerminalGuard { kbd_enhanced };

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(client, ai, cfg_path, server_hint);

    while !app.should_quit {
        app.process_bg_events();
        if app.last_status_poll.elapsed() >= Duration::from_secs(15) {
            app.trigger_refresh_status();
        }
        app.tick = app.tick.wrapping_add(1);

        terminal.draw(|f| app.draw(f))?;

        if event::poll(Duration::from_millis(75))? {
            match event::read()? {
                Event::Key(k) => app.handle_key(k),
                Event::Paste(text) => app.handle_paste(text),
                Event::Mouse(m) if !app.mouse_select_mode => app.handle_mouse(m),
                _ => {}
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::format_commit_timestamp;

    #[test]
    fn formats_iso_commit_timestamp_with_time_and_offset() {
        assert_eq!(
            format_commit_timestamp("2026-11-26T14:05:32+08:00"),
            "2026-11-26 14:05:32+08:00"
        );
        assert_eq!(
            format_commit_timestamp(" 2026-11-26T06:05:32Z "),
            "2026-11-26 06:05:32Z"
        );
        assert_eq!(format_commit_timestamp("2026-11-26"), "2026-11-26");
    }
}
