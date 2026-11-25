use anyhow::Result;
use distill_proto::{DistillRequest, DistillResponse, SemanticKind};
use serde_json::{Value, json};
use std::future::Future;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Lightweight Pure Rust MCP (Model Context Protocol) JSON-RPC 2.0 server over stdio.
/// Allows any local AI (Antigravity, Claude Code, Cursor, Codex, Ollama) to query the
/// remote Distill server over iroh P2P QUIC without ever seeing raw source code.
pub async fn run_mcp_stdio<F, Fut>(call_rpc: F) -> Result<()>
where
    F: Fn(DistillRequest) -> Fut,
    Fut: Future<Output = Result<DistillResponse>>,
{
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                write_json_line(&mut stdout, &err).await?;
                continue;
            }
        };

        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

        // Notifications have no id and expect no reply.
        if id.is_none() {
            continue;
        }
        let id_val = id.unwrap();

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "distill",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            "ping" => json!({ "jsonrpc": "2.0", "id": id_val, "result": {} }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": { "tools": tool_definitions() }
            }),
            "tools/call" => {
                let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let result = handle_tool_call(tool_name, &args, &call_rpc).await;
                match result {
                    Ok(payload) => json!({
                        "jsonrpc": "2.0",
                        "id": id_val,
                        "result": {
                            "content": [
                                {
                                    "type": "text",
                                    "text": serde_json::to_string_pretty(&payload).unwrap_or_default()
                                }
                            ]
                        }
                    }),
                    Err(err_msg) => json!({
                        "jsonrpc": "2.0",
                        "id": id_val,
                        "result": {
                            "isError": true,
                            "content": [{ "type": "text", "text": err_msg }]
                        }
                    }),
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            }),
        };

        write_json_line(&mut stdout, &response).await?;
    }

    Ok(())
}

async fn write_json_line(stdout: &mut tokio::io::Stdout, val: &Value) -> Result<()> {
    let mut s = serde_json::to_string(val)?;
    s.push('\n');
    stdout.write_all(s.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}

fn tool_definitions() -> Value {
    let ro = json!({
        "readOnlyHint": true,
        "destructiveHint": false,
        "idempotentHint": true,
        "openWorldHint": false
    });
    let mut defs = json!([
        {
            "name": "server_status",
            "description": "查询远端 Distill 服务在线状态、运行时间、缓存索引数、活跃 worktree 数与仓库同步状态。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "server_metrics",
            "description": "查询远端 Distill 服务 /metrics：各仓库硬盘占用、活跃 worktree 与对应分支、分支索引缓存、LRU/TTL 清理统计与配置阈值。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_repos",
            "description": "列出当前用户有权查询的远端 Git 仓库白名单（只读，无源码权限）。",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "list_branches",
            "description": "列出指定仓库下允许查询的 Git 分支及其最新 commit SHA、提交摘要与索引状态。",
            "inputSchema": {
                "type": "object",
                "required": ["repo"],
                "properties": {
                    "repo": { "type": "string", "description": "仓库名，如 twosite" }
                }
            }
        },
        {
            "name": "fetch_repo",
            "description": "触发远端服务对指定仓库执行 git fetch --prune 并返回最新分支列表。",
            "inputSchema": {
                "type": "object",
                "required": ["repo"],
                "properties": {
                    "repo": { "type": "string", "description": "仓库名，如 twosite" }
                }
            }
        },
        {
            "name": "search_semantics",
            "description": "在指定仓库和分支的脱敏语义索引中搜索业务实现、字段定义、接口契约、错误码、表结构与规格文档（不返回源码原文）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch", "query"],
                "properties": {
                    "repo": { "type": "string", "description": "仓库名，如 twosite" },
                    "branch": { "type": "string", "description": "分支名，如 main 或 dev/v1" },
                    "query": { "type": "string", "description": "业务问题或关键词，如 '价值点 集测1轮'、'方案号'、'更新工作项'" },
                    "kind": {
                        "type": "string",
                        "description": "可选过滤类型：api_endpoint / dto_schema / error_contract / entity_schema / migration_fact / doc_section / constant_catalog"
                    },
                    "limit": { "type": "integer", "description": "返回条数上限（默认 10，最大 50）" }
                }
            }
        },
        {
            "name": "get_api_contracts",
            "description": "按关键词查询指定分支的 HTTP API 路由、请求方法、权限码、鉴权头与 DTO 名称（不返回实现代码）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch", "keyword"],
                "properties": {
                    "repo": { "type": "string" },
                    "branch": { "type": "string" },
                    "keyword": { "type": "string", "description": "路径、中文摘要或模块关键词" },
                    "limit": { "type": "integer" }
                }
            }
        },
        {
            "name": "list_tree",
            "description": "快速列出指定仓库和分支的目录与文件树（全语言通用，无需检出工作区）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch"],
                "properties": {
                    "repo": { "type": "string" },
                    "branch": { "type": "string" },
                    "path": { "type": "string", "description": "可选子目录路径" },
                    "max_depth": { "type": "integer", "description": "最大递归深度（默认 3，最大 8）" },
                    "limit": { "type": "integer", "description": "返回条目上限（默认 200）" }
                }
            }
        },
        {
            "name": "grep_repo",
            "description": "在指定仓库和分支的任意文本/代码文件中执行高速正则或关键词全文检索（自动脱敏密钥，支持上下文行）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch", "pattern"],
                "properties": {
                    "repo": { "type": "string" },
                    "branch": { "type": "string" },
                    "pattern": { "type": "string", "description": "正则或关键词" },
                    "path_prefix": { "type": "string", "description": "可选限定子目录" },
                    "case_insensitive": { "type": "boolean", "description": "是否忽略大小写，默认 true" },
                    "context_lines": { "type": "integer", "description": "前后上下文行数（默认 2，最大 5）" },
                    "limit": { "type": "integer", "description": "最大命中数（默认 40）" }
                }
            }
        },
        {
            "name": "read_file",
            "description": "按路径和行号区间直读指定分支的任意文本文件内容（自动脱敏密钥/Token，禁止读取 .env/.key/.pem）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch", "path"],
                "properties": {
                    "repo": { "type": "string" },
                    "branch": { "type": "string" },
                    "path": { "type": "string", "description": "仓库内相对文件路径" },
                    "start_line": { "type": "integer", "description": "起始行号（1-based，默认 1）" },
                    "end_line": { "type": "integer", "description": "结束行号（默认最多 300 行）" }
                }
            }
        },
        {
            "name": "get_recent_commits",
            "description": "查看指定仓库和分支最近的 Git 提交历史摘要（仅含脱敏提交说明、作者、时间与受影响模块名，不含源码 diff）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch"],
                "properties": {
                    "repo": { "type": "string" },
                    "branch": { "type": "string" },
                    "limit": { "type": "integer", "description": "返回条数（默认 15，最大 50）" }
                }
            }
        },
        {
            "name": "diff_branch_behavior",
            "description": "对比同一仓库两个分支之间的业务能力、接口、字段、错误码与文档变更摘要（不返回源码 diff）。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "base_branch", "target_branch"],
                "properties": {
                    "repo": { "type": "string" },
                    "base_branch": { "type": "string", "description": "基准分支，如 main" },
                    "target_branch": { "type": "string", "description": "目标分支，如 dev/v1" }
                }
            }
        },
        {
            "name": "query_vlogs",
            "description": "通过服务端查询固化的 VictoriaLogs (vlogs.hs99.vip:9428) 实时运行日志，支持 LogsQL 语法（如 `container:twosite-api error` 或 `traceId`），返回自动脱敏后的日志条目。",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string", "description": "LogsQL 表达式，例如 `container:twosite-api error` 或 traceId" },
                    "range": { "type": "string", "description": "时间窗，如 5m / 15m / 1h / 6h / 24h，默认 15m" },
                    "start": { "type": "string", "description": "可选 RFC3339 起始时间" },
                    "end": { "type": "string", "description": "可选 RFC3339 结束时间" },
                    "limit": { "type": "integer", "description": "返回条数上限（默认 50，最大 200）" },
                    "repo_hint": { "type": "string", "description": "可选仓库提示（如 twosite），用于默认容器/命名空间过滤" }
                }
            }
        },
        {
            "name": "investigate",
            "description": "服务端一体化智能整合：针对自然语言问题在服务端并发执行 Raw Git 代码检索（tree/grep/read/commits/semantic/diff）与可选 VictoriaLogs 日志查询（自动分析日志意图或强制开启），返回合并脱敏后的 InvestigationBundle。",
            "inputSchema": {
                "type": "object",
                "required": ["repo", "branch", "question"],
                "properties": {
                    "repo": { "type": "string" },
                    "branch": { "type": "string" },
                    "question": { "type": "string", "description": "自然语言问题或排查诉求" },
                    "include_logs": { "type": "boolean", "description": "可选：true 强制联合查 VictoriaLogs，false 仅查代码，省略则由服务端自动判断意图" },
                    "log_range": { "type": "string", "description": "可选 VictoriaLogs 时间窗（如 5m / 15m / 1h）" }
                }
            }
        }
    ]);
    if let Some(arr) = defs.as_array_mut() {
        for item in arr {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("annotations".to_string(), ro.clone());
            }
        }
    }
    defs
}

async fn handle_tool_call<F, Fut>(
    tool: &str,
    args: &Value,
    call_rpc: &F,
) -> std::result::Result<DistillResponse, String>
where
    F: Fn(DistillRequest) -> Fut,
    Fut: Future<Output = Result<DistillResponse>>,
{
    let req = match tool {
        "server_status" => DistillRequest::ServerStatus,
        "server_metrics" => DistillRequest::Metrics,
        "list_repos" => DistillRequest::ListRepos,
        "list_branches" => DistillRequest::ListBranches {
            repo: required_str(args, "repo")?,
        },
        "fetch_repo" => DistillRequest::FetchRepo {
            repo: required_str(args, "repo")?,
        },
        "search_semantics" => DistillRequest::SearchSemantics {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            query: required_str(args, "query")?,
            kind: args
                .get("kind")
                .and_then(Value::as_str)
                .and_then(SemanticKind::from_str_opt),
            limit: args.get("limit").and_then(Value::as_u64).map(|v| v as usize),
        },
        "get_api_contracts" => DistillRequest::GetApiContracts {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            keyword: required_str(args, "keyword")?,
            limit: args.get("limit").and_then(Value::as_u64).map(|v| v as usize),
        },
        "list_tree" => DistillRequest::ListTree {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            path: args.get("path").and_then(Value::as_str).map(str::to_string),
            max_depth: args.get("max_depth").and_then(Value::as_u64).map(|v| v as usize),
            limit: args.get("limit").and_then(Value::as_u64).map(|v| v as usize),
        },
        "grep_repo" => DistillRequest::GrepRepo {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            pattern: required_str(args, "pattern")?,
            path_prefix: args
                .get("path_prefix")
                .and_then(Value::as_str)
                .map(str::to_string),
            case_insensitive: args
                .get("case_insensitive")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            context_lines: args
                .get("context_lines")
                .and_then(Value::as_u64)
                .map(|v| v as usize),
            limit: args.get("limit").and_then(Value::as_u64).map(|v| v as usize),
        },
        "read_file" => DistillRequest::ReadFile {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            path: required_str(args, "path")?,
            start_line: args.get("start_line").and_then(Value::as_u64).map(|v| v as usize),
            end_line: args.get("end_line").and_then(Value::as_u64).map(|v| v as usize),
        },
        "get_recent_commits" => DistillRequest::RecentCommits {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            limit: args.get("limit").and_then(Value::as_u64).map(|v| v as usize),
        },
        "diff_branch_behavior" => DistillRequest::DiffBranchBehavior {
            repo: required_str(args, "repo")?,
            base_branch: required_str(args, "base_branch")?,
            target_branch: required_str(args, "target_branch")?,
        },
        "query_vlogs" => DistillRequest::QueryLogs {
            query: required_str(args, "query")?,
            range: args.get("range").and_then(Value::as_str).map(str::to_string),
            start: args.get("start").and_then(Value::as_str).map(str::to_string),
            end: args.get("end").and_then(Value::as_str).map(str::to_string),
            limit: args.get("limit").and_then(Value::as_u64).map(|v| v as usize),
            repo_hint: args
                .get("repo_hint")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "investigate" => DistillRequest::Investigate {
            repo: required_str(args, "repo")?,
            branch: required_str(args, "branch")?,
            question: required_str(args, "question")?,
            include_logs: args.get("include_logs").and_then(Value::as_bool),
            log_range: args
                .get("log_range")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        other => return Err(format!("unknown tool: {other}")),
    };

    call_rpc(req).await.map_err(|e| format!("RPC error: {e:#}"))
}

fn required_str(args: &Value, field: &str) -> std::result::Result<String, String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing required string argument: {field}"))
}
