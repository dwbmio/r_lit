use crate::config::{ClientAcl, ServerConfig};
use crate::vlogs::VictoriaLogsClient;
use anyhow::Result;
use distill_crypto::DlpGate;
use distill_index::IndexService;
use distill_proto::{FileExcerpt, InvestigationBundle, LogsQueryResult};
use std::collections::HashSet;

/// Service-side unified investigation:
/// 1. Analyze natural-language intent (raw git code + optional VictoriaLogs intent)
/// 2. Execute raw Git operations (`tree` / `read` / `grep` / `log` / `diff` / `semantic`) AND
///    optional VictoriaLogs LogsQL queries IN PARALLEL on the server
/// 3. Secret-redact everything and return a single `InvestigationBundle` to the client's LLM.
pub async fn run_investigation(
    cfg: &ServerConfig,
    index: &IndexService,
    vlogs: &VictoriaLogsClient,
    acl: &ClientAcl,
    repo: &str,
    branch: &str,
    question: &str,
    include_logs: Option<bool>,
    log_range: Option<&str>,
) -> Result<InvestigationBundle> {
    let git = index.git();

    // 1. Resolve target branch (auto-switch if user explicitly names another authorized branch)
    let is_commit_q = looks_like_commit_question(question);
    let is_diff_q = looks_like_diff_question(question);
    let is_tree_q = looks_like_tree_question(question);
    let is_review_q = looks_like_review_or_overview_question(question);
    let explicit_files = extract_file_paths(question);

    let mut effective_branch = branch.to_string();
    let mut known_branches = Vec::new();
    if is_commit_q || is_diff_q || question.contains('/') || question.contains("main") {
        if let Ok(branches) = git.list_branches(repo).await {
            let allowed: Vec<_> = branches
                .into_iter()
                .filter(|b| acl.allows_branch(&b.name))
                .collect();
            if let Some(mentioned) = allowed
                .iter()
                .find(|b| b.name != branch && question.contains(&b.name))
                && !is_diff_q
            {
                effective_branch = mentioned.name.clone();
            }
            known_branches = allowed;
        }
    }

    let commit_sha = git
        .resolve_branch(repo, &effective_branch)
        .await
        .unwrap_or_default();

    // 2. Analyze optional VictoriaLogs intent on the service side
    let wants_logs = match include_logs {
        Some(true) => vlogs.is_enabled(),
        Some(false) => false,
        None => vlogs.is_enabled() && (detect_log_intent(question) || is_review_q),
    };

    let repo_vlogs_filter = cfg
        .repos
        .iter()
        .find(|r| r.name == repo)
        .and_then(|r| r.vlogs_filter.as_deref())
        .or_else(|| default_repo_vlogs_filter(repo));

    let mut detected_intents = Vec::new();
    if is_review_q {
        detected_intents.push("code_review".to_string());
    }
    if is_tree_q || is_review_q {
        detected_intents.push("tree".to_string());
    }
    if !explicit_files.is_empty() {
        detected_intents.push("read_file".to_string());
    }
    if is_commit_q || is_review_q {
        detected_intents.push("recent_commits".to_string());
    }
    if is_diff_q {
        detected_intents.push("branch_diff".to_string());
    }
    if wants_logs {
        detected_intents.push("vlogs".to_string());
    }

    // 3. Run Raw Git Code investigation & optional VictoriaLogs query IN PARALLEL
    let eff_branch_clone = effective_branch.clone();
    let repo_str = repo.to_string();
    let question_str = question.to_string();
    let files_to_read = explicit_files.clone();
    let known_branches_clone = known_branches.clone();

    let code_fut = async move {
        let mut steps = Vec::new();
        let mut tree = Vec::new();
        let mut files = Vec::new();
        let mut grep_matches = Vec::new();
        let mut commits = Vec::new();
        let mut semantic_cards = Vec::new();
        let mut total_cards_indexed = None;
        let mut branch_diff = None;
        let mut dlp_redactions = 0usize;

        if is_tree_q || is_review_q {
            if let Ok((_, _, entries, _)) = git
                .list_tree(&repo_str, &eff_branch_clone, None, if is_review_q { 2 } else { 3 }, 180)
                .await
            {
                steps.push(format!("tree({eff_branch_clone})→{}项", entries.len()));
                tree = entries;
            }
        }

        for path_cand in &files_to_read {
            if let Ok((_, norm_path, s, e, total, content, _, r)) = git
                .read_file_lines(&repo_str, &eff_branch_clone, path_cand, Some(1), Some(260))
                .await
            {
                dlp_redactions += r;
                steps.push(format!("read({norm_path}:L{s}-{e})"));
                files.push(FileExcerpt {
                    path: norm_path,
                    start_line: s,
                    end_line: e,
                    total_lines: total,
                    content,
                });
            }
        }

        if is_commit_q || is_review_q {
            if let Ok(mut list) = git
                .recent_commits(&repo_str, &eff_branch_clone, if is_review_q { 10 } else { 15 })
                .await
            {
                for c in &mut list {
                    let (s, r1) = DlpGate::sanitize_secrets(&c.subject);
                    let (b, r2) = DlpGate::sanitize_secrets(&c.body);
                    c.subject = s;
                    c.body = b;
                    dlp_redactions += r1 + r2;
                }
                steps.push(format!("commits({eff_branch_clone})→{}条", list.len()));
                commits = list;
            }
        }

        if is_review_q && files.is_empty() {
            for overview_path in ["AGENTS.md", "package.json", "Cargo.toml", "README.md"] {
                if files.len() >= 2 {
                    break;
                }
                if let Ok((_, norm_path, s, e, total, content, _, r)) = git
                    .read_file_lines(&repo_str, &eff_branch_clone, overview_path, Some(1), Some(100))
                    .await
                {
                    dlp_redactions += r;
                    steps.push(format!("read({norm_path}:L{s}-{e})"));
                    files.push(FileExcerpt {
                        path: norm_path,
                        start_line: s,
                        end_line: e,
                        total_lines: total,
                        content,
                    });
                }
            }
        }

        let sub_queries = decompose_natural_question(&question_str);
        let pure_commits = is_commit_q && !commits.is_empty() && !wants_logs;
        let pure_tree = is_tree_q && !tree.is_empty() && sub_queries.len() <= 1 && files.is_empty() && !wants_logs;

        if !pure_commits && !pure_tree {
            let grep_terms = extract_grep_terms(&question_str, &sub_queries);
            let mut seen_grep = HashSet::new();
            for term in grep_terms.iter().take(2) {
                if let Ok((_, matches, _, r)) = git
                    .grep_repo(&repo_str, &eff_branch_clone, term, None, true, 2, 18)
                    .await
                {
                    dlp_redactions += r;
                    steps.push(format!("grep(\"{term}\")→{}", matches.len()));
                    for m in matches {
                        if seen_grep.insert((m.path.clone(), m.line_number)) {
                            grep_matches.push(m);
                        }
                    }
                }
            }
            grep_matches.truncate(24);

            // Best-effort semantic enrichment if index is already warm (or fast build)
            let mut seen_cards = HashSet::new();
            for q in sub_queries.iter().take(2) {
                if let Ok(payload) = index
                    .search_branch(&repo_str, &eff_branch_clone, q, None, 6)
                    .await
                {
                    total_cards_indexed = Some(payload.total_cards_indexed);
                    dlp_redactions += payload.dlp_redactions;
                    steps.push(format!("semantic(\"{q}\")→{}", payload.hits.len()));
                    for c in payload.hits {
                        if seen_cards.insert(c.card_id.clone()) {
                            semantic_cards.push(c);
                        }
                    }
                }
            }
            semantic_cards.truncate(12);
        }

        if is_diff_q {
            if let Some(other) = known_branches_clone
                .iter()
                .find(|b| b.name != eff_branch_clone && question_str.contains(&b.name))
                .or_else(|| {
                    known_branches_clone
                        .iter()
                        .find(|b| b.name == "main" && b.name != eff_branch_clone)
                })
            {
                if let Ok(d) = index
                    .diff_branch_behavior(&repo_str, &other.name, &eff_branch_clone)
                    .await
                {
                    dlp_redactions += d.dlp_redactions;
                    steps.push(format!("diff({}..{eff_branch_clone})", other.name));
                    branch_diff = Some(d);
                }
            }
        }

        // Never return a completely empty bundle for non-trivial questions:
        // provide repo directory overview + recent commits as baseline context.
        if tree.is_empty() && files.is_empty() && grep_matches.is_empty() && commits.is_empty() {
            if let Ok((_, _, entries, _)) = git
                .list_tree(&repo_str, &eff_branch_clone, None, 2, 140)
                .await
            {
                steps.push(format!("tree({eff_branch_clone})→{}项", entries.len()));
                tree = entries;
            }
            if let Ok(mut list) = git.recent_commits(&repo_str, &eff_branch_clone, 8).await {
                for c in &mut list {
                    let (s, r1) = DlpGate::sanitize_secrets(&c.subject);
                    c.subject = s;
                    dlp_redactions += r1;
                }
                steps.push(format!("commits({eff_branch_clone})→{}条", list.len()));
                commits = list;
            }
        }

        (
            steps,
            tree,
            files,
            grep_matches,
            commits,
            semantic_cards,
            total_cards_indexed,
            branch_diff,
            dlp_redactions,
        )
    };

    let logs_fut = async move {
        if !wants_logs {
            return (None, None, 0usize);
        }
        let range = log_range
            .map(str::to_string)
            .or_else(|| extract_time_range(question))
            .unwrap_or_else(|| vlogs.default_range().to_string());
        let mut logsql = build_logsql_from_question(question);
        if is_review_q && logsql.is_empty() {
            logsql = "(error OR warn OR exception OR 500)".to_string();
        }
        let log_cap = if is_review_q && !detect_log_intent(question) {
            20
        } else {
            45
        };

        // First query scoped to repo default filter (unless question specifies container/namespace)
        match vlogs
            .query(&logsql, Some(&range), None, None, Some(log_cap), repo_vlogs_filter)
            .await
        {
            Ok(mut res) => {
                // If scoped query returned 0 entries and we had a specific token (like traceId or keyword),
                // retry once globally without repo filter so cross-service logs are not missed.
                if res.entries.is_empty() && !logsql.is_empty() && repo_vlogs_filter.is_some() {
                    if let Ok(global_res) = vlogs
                        .query(&logsql, Some(&range), None, None, Some(log_cap), None)
                        .await
                        && !global_res.entries.is_empty()
                    {
                        res = global_res;
                    }
                }
                let red = res.dlp_redactions;
                let step = format!("vlogs({range})→{}条", res.entries.len());
                (Some(res), Some(step), red)
            }
            Err(e) => {
                let err_res = LogsQueryResult {
                    endpoint: vlogs.endpoint().unwrap_or_default(),
                    effective_query: logsql,
                    range,
                    entries: Vec::new(),
                    truncated: false,
                    dlp_redactions: 0,
                    note: format!("VictoriaLogs 查询异常: {e:#}"),
                };
                (Some(err_res), Some("vlogs→失败".to_string()), 0)
            }
        }
    };

    let (
        (
            mut steps,
            tree,
            files,
            grep_matches,
            commits,
            semantic_cards,
            total_cards_indexed,
            branch_diff,
            code_redactions,
        ),
        (logs_opt, log_step, log_redactions),
    ) = tokio::join!(code_fut, logs_fut);

    if let Some(ls) = log_step {
        steps.push(ls);
    }
    if !grep_matches.is_empty() {
        detected_intents.push("raw_grep".to_string());
    }
    if !semantic_cards.is_empty() {
        detected_intents.push("semantic".to_string());
    }

    Ok(InvestigationBundle {
        repo: repo.to_string(),
        effective_branch,
        commit_sha,
        detected_intents,
        steps,
        tree,
        files,
        grep_matches,
        commits,
        semantic_cards,
        total_cards_indexed,
        branch_diff,
        logs: logs_opt,
        dlp_redactions: code_redactions + log_redactions,
    })
}

fn default_repo_vlogs_filter(repo: &str) -> Option<&'static str> {
    match repo.to_ascii_lowercase().as_str() {
        "twosite" | "rigel" => Some("container:twosite-api OR container:rigel"),
        _ => None,
    }
}

/// Detect whether the user's question implies looking up live runtime logs in VictoriaLogs.
pub fn detect_log_intent(q: &str) -> bool {
    let lower = q.to_ascii_lowercase();
    let signals = [
        "日志",
        "vlogs",
        "victorialogs",
        "报错",
        "异常",
        "错误日志",
        "故障",
        "排查",
        "线上",
        "现网",
        "挂了",
        "崩溃",
        "超时",
        "timeout",
        "死锁",
        "deadlock",
        "oom",
        "panic",
        "500",
        "502",
        "503",
        "504",
        "traceid",
        "trace_id",
        "actionrunid",
        "刚刚",
        "刚才",
        "最近几分钟",
        "为什么报",
        "失败原因",
        "container:",
        "namespace:",
        "twosite-api",
        "starflow",
        "pod",
    ];
    if signals.iter().any(|s| lower.contains(s)) {
        return true;
    }
    // Check for 32-char hex traceId or UUID in question
    extract_trace_or_uuid(q).is_some()
}

fn extract_time_range(q: &str) -> Option<String> {
    let bytes = q.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num_str = &q[start..i];
            let rest = q[i..].trim_start();
            let rest_lower = rest.to_ascii_lowercase();
            let suffix = if rest.starts_with("分钟")
                || rest.starts_with("分")
                || rest_lower.starts_with("min")
                || (rest_lower.starts_with('m')
                    && !rest_lower[1..].starts_with(|c: char| c.is_ascii_alphabetic()))
            {
                Some("m")
            } else if rest.starts_with("小时")
                || rest_lower.starts_with("hr")
                || rest_lower.starts_with("hour")
                || (rest_lower.starts_with('h')
                    && !rest_lower[1..].starts_with(|c: char| c.is_ascii_alphabetic()))
            {
                Some("h")
            } else if rest.starts_with("天")
                || rest_lower.starts_with("day")
                || (rest_lower.starts_with('d')
                    && !rest_lower[1..].starts_with(|c: char| c.is_ascii_alphabetic()))
            {
                Some("d")
            } else {
                None
            };
            if let Some(unit) = suffix
                && let Ok(n) = num_str.parse::<u32>()
                && (1..=365).contains(&n)
            {
                return Some(format!("{n}{unit}"));
            }
        } else {
            i += 1;
        }
    }
    let lower = q.to_ascii_lowercase();
    for (pat, range) in [
        ("半小时", "30m"),
        ("一小时", "1h"),
        ("两小时", "2h"),
        ("今天", "24h"),
        ("一天", "24h"),
    ] {
        if lower.contains(pat) {
            return Some(range.to_string());
        }
    }
    None
}

fn build_logsql_from_question(q: &str) -> String {
    let mut clauses = Vec::new();

    // 1. Preserve explicit container: / namespace: / pod: filters if user wrote them
    for tok in q.split_whitespace() {
        if tok.starts_with("container:")
            || tok.starts_with("namespace:")
            || tok.starts_with("pod:")
            || tok.starts_with("cluster:")
            || tok.starts_with("_time:")
        {
            clauses.push(tok.to_string());
        }
    }

    // 2. Auto-detect well-known container names mentioned directly in natural language
    if !clauses.iter().any(|c| c.starts_with("container:")) {
        let lower = q.to_ascii_lowercase();
        for cname in [
            "twosite-api",
            "rigel",
            "starflow",
            "openresty",
            "kong",
            "worker",
            "gql",
            "bff",
            "bbc",
            "ding",
        ] {
            if lower.contains(cname) {
                clauses.push(format!("container:{cname}"));
                break;
            }
        }
    }

    // 3. Exact traceId / UUID match takes highest precision priority
    if let Some(tid) = extract_trace_or_uuid(q) {
        clauses.push(format!("\"{tid}\""));
        return clauses.join(" ");
    }

    // 4. HTTP error codes or explicit error/timeout/deadlock signals
    let lower = q.to_ascii_lowercase();
    let mut err_terms = Vec::new();
    for code in ["500", "502", "503", "504", "400", "401", "403", "404"] {
        if lower.contains(code) {
            err_terms.push(code.to_string());
        }
    }
    if lower.contains("超时") || lower.contains("timeout") {
        err_terms.push("timeout".to_string());
    }
    if lower.contains("死锁") || lower.contains("deadlock") {
        err_terms.push("deadlock".to_string());
    }
    if err_terms.is_empty()
        && (lower.contains("报错")
            || lower.contains("异常")
            || lower.contains("错误")
            || lower.contains("失败")
            || lower.contains("故障")
            || lower.contains("error")
            || lower.contains("exception"))
    {
        clauses.push("(error OR ERROR OR Exception OR failed OR statusCode:5)".to_string());
    } else if !err_terms.is_empty() {
        clauses.push(format!("({})", err_terms.join(" OR ")));
    }

    clauses.join(" ")
}

fn extract_trace_or_uuid(q: &str) -> Option<String> {
    for raw in q.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
        // 32-hex traceId (like 1167325c8eb2fc2d276e322a60dd438a)
        if raw.len() == 32 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(raw.to_string());
        }
        // Standard UUID v4 (36 chars with 4 dashes)
        if raw.len() == 36
            && raw.chars().filter(|&c| c == '-').count() == 4
            && raw.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        {
            return Some(raw.to_string());
        }
    }
    None
}

fn looks_like_review_or_overview_question(q: &str) -> bool {
    let lower = q.to_ascii_lowercase();
    [
        "代码质量",
        "实现质量",
        "质量",
        "review",
        "审查",
        "评估",
        "代码怎么样",
        "写的怎么样",
        "写得怎么样",
        "整体架构",
        "系统架构",
        "技术债",
        "隐患",
        "重构",
        "稳定性",
        "通篇",
        "全面看看",
        "全面检查",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
}

fn looks_like_tree_question(q: &str) -> bool {
    let lower = q.to_ascii_lowercase();
    ["目录", "结构", "架构", "有哪些文件", "有哪些模块", "项目介绍", "仓库介绍", "tree", "布局"]
        .iter()
        .any(|kw| lower.contains(kw))
}

fn looks_like_commit_question(q: &str) -> bool {
    let lower = q.to_ascii_lowercase();
    [
        "提交", "commit", "最近", "近期", "git log", "历史", "改了什么", "改了啥", "更新了什么",
        "变更记录", "改动记录",
    ]
    .iter()
    .any(|kw| lower.contains(kw))
        && !lower.contains("最近几分钟")
        && !lower.contains("分钟")
        && !lower.contains("小时")
        && !lower.contains("日志")
}

fn looks_like_diff_question(q: &str) -> bool {
    let lower = q.to_ascii_lowercase();
    ["对比", "差异", "区别", "变化", "相比", "diff", "改动", "新增了什么"]
        .iter()
        .any(|kw| lower.contains(kw))
}

fn extract_file_paths(question: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in question.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '`' | '\'' | '"' | '（' | '）' | '(' | ')' | '，' | ',' | '。' | '？' | '?'
            )
    }) {
        let tok = raw.trim_matches(|c| c == ':' || c == ';');
        if tok.len() < 3 || tok.starts_with("http://") || tok.starts_with("https://") {
            continue;
        }
        let looks_like_file = tok.contains('/')
            || matches!(
                tok,
                "Cargo.toml"
                    | "package.json"
                    | "Jenkinsfile"
                    | "Dockerfile"
                    | "README.md"
                    | "AGENTS.md"
                    | "CLAUDE.md"
            )
            || [
                ".ts", ".tsx", ".rs", ".go", ".py", ".java", ".json", ".toml", ".yaml", ".yml",
                ".sql", ".md", ".sh", ".ps1", ".proto", ".graphql",
            ]
            .iter()
            .any(|ext| tok.ends_with(ext));
        if looks_like_file && !tok.contains("..") && !out.contains(&tok.to_string()) {
            out.push(tok.to_string());
        }
    }
    out.truncate(3);
    out
}

fn decompose_natural_question(question: &str) -> Vec<String> {
    let mut queries = Vec::new();
    let mut seen = HashSet::new();
    let normalized = question
        .replace("顺便", "|")
        .replace("以及", "|")
        .replace("并且", "|")
        .replace("同时", "|")
        .replace("还有", "|")
        .replace("结合代码", "|")
        .replace(['？', '?', '，', ',', '。', ';', '；', '\n'], "|");

    for raw_clause in normalized.split('|') {
        let mut s = raw_clause.trim().to_string();
        for sw in [
            "请帮我查一下",
            "帮我查一下",
            "帮我查下",
            "帮我看看",
            "告诉我",
            "请问",
            "帮我",
            "怎么查",
            "怎么",
            "如何",
            "是什么",
            "有哪些",
            "有什么",
            "对应的",
            "通过",
            "一下",
            "看原因",
            "报错日志",
            "日志",
            "吗",
            "呢",
        ] {
            s = s.replace(sw, " ");
        }
        let cleaned = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if cleaned.chars().count() >= 2 && seen.insert(cleaned.clone()) {
            queries.push(cleaned);
        }
    }
    queries.truncate(4);
    queries
}

fn extract_grep_terms(question: &str, sub_queries: &[String]) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();
    for tok in question.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '/') {
        if tok.len() >= 3
            && tok.chars().any(|c| c.is_ascii_alphabetic())
            && !(tok.len() == 32 && tok.chars().all(|c| c.is_ascii_hexdigit()))
            && !matches!(
                tok.to_ascii_lowercase().as_str(),
                "main" | "dev" | "api" | "key" | "get" | "post" | "put" | "vlogs" | "log" | "logs" | "error"
            )
            && seen.insert(tok.to_string())
        {
            terms.push(tok.to_string());
        }
    }
    for sq in sub_queries {
        for word in sq.split_whitespace() {
            let w = word.trim_matches(|c| matches!(c, '（' | '）' | '(' | ')' | '「' | '」'));
            if w.chars().count() >= 2
                && w.chars().count() <= 16
                && seen.insert(w.to_string())
            {
                terms.push(w.to_string());
            }
        }
    }
    terms.truncate(3);
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_log_intent_accurately() {
        assert!(detect_log_intent("最近15分钟 twosite-api 有什么报错日志，结合代码看原因"));
        assert!(detect_log_intent("查下 traceId 1167325c8eb2fc2d276e322a60dd438a 为啥失败"));
        assert!(!detect_log_intent("价值点（集测1轮）字段的内部 key 是什么？"));
        assert!(!detect_log_intent("dev/v1 最近的几次提交都有什么"));
    }

    #[test]
    fn extracts_time_range_and_logsql() {
        assert_eq!(extract_time_range("查最近5分钟报错"), Some("5m".into()));
        assert_eq!(extract_time_range("最近15分钟 twosite-api 有什么日志"), Some("15m".into()));
        assert_eq!(extract_time_range("过去1小时超时日志"), Some("1h".into()));
        let q = build_logsql_from_question("查 twosite-api 最近5分钟 500 报错");
        assert!(q.contains("container:twosite-api"));
        assert!(q.contains("500"));
    }
}
