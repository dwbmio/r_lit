use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use thiserror::Error;

pub const APP_NAME: &str = "distill";
pub const CLIENT_APP_NAME: &str = "distill-client";
pub const DISTILL_ALPN: &[u8] = b"distill/p2p/1";
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Resolve the root directory for an application:
/// 1. Explicit env override (e.g. `DISTILL_HOME`)
/// 2. Default `$HOME/.<app_name>`
pub fn resolve_app_home(app_name: &str, env_var: &str) -> PathBuf {
    if let Ok(explicit) = std::env::var(env_var) {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(format!(".{app_name}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    ApiEndpoint,
    DtoSchema,
    ErrorContract,
    DomainRule,
    EntitySchema,
    MigrationFact,
    DocSection,
    ConstantCatalog,
}

impl SemanticKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiEndpoint => "api_endpoint",
            Self::DtoSchema => "dto_schema",
            Self::ErrorContract => "error_contract",
            Self::DomainRule => "domain_rule",
            Self::EntitySchema => "entity_schema",
            Self::MigrationFact => "migration_fact",
            Self::DocSection => "doc_section",
            Self::ConstantCatalog => "constant_catalog",
        }
    }

    pub fn from_str_opt(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "api_endpoint" | "api" | "endpoint" => Some(Self::ApiEndpoint),
            "dto_schema" | "dto" => Some(Self::DtoSchema),
            "error_contract" | "error" => Some(Self::ErrorContract),
            "domain_rule" | "rule" => Some(Self::DomainRule),
            "entity_schema" | "entity" => Some(Self::EntitySchema),
            "migration_fact" | "migration" => Some(Self::MigrationFact),
            "doc_section" | "doc" => Some(Self::DocSection),
            "constant_catalog" | "constant" => Some(Self::ConstantCatalog),
            _ => None,
        }
    }
}

/// Sanitized semantic card returned to clients. Never contains raw source code or host paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticCard {
    pub card_id: String,
    pub kind: SemanticKind,
    pub domain: String,
    pub title: String,
    pub summary: String,
    pub metadata: BTreeMap<String, String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSummary {
    pub name: String,
    pub description: String,
    pub default_branch: String,
    pub allowed_branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub commit_sha: String,
    #[serde(default)]
    pub commit_summary: String,
    #[serde(default)]
    pub indexed_cards: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoStatusInfo {
    pub name: String,
    pub mode: String,
    pub default_branch: String,
    pub last_fetch_secs_ago: Option<u64>,
    pub last_fetch_ok: bool,
    pub last_fetch_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerStatusInfo {
    pub server_version: String,
    pub protocol: String,
    pub uptime_secs: u64,
    pub peer_label: String,
    pub cached_branch_indexes: usize,
    pub active_worktrees: usize,
    pub total_requests: u64,
    pub total_dlp_redactions: u64,
    pub repos: Vec<RepoStatusInfo>,
    #[serde(default)]
    pub vlogs_endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRepoResult {
    pub repo: String,
    pub fetched: bool,
    pub message: String,
    pub branches: Vec<BranchInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultPayload {
    pub repo: String,
    pub branch: String,
    pub commit_sha: String,
    pub total_cards_indexed: usize,
    pub dlp_redactions: usize,
    pub hits: Vec<SemanticCard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchBehaviorDiff {
    pub repo: String,
    pub base_branch: String,
    pub base_commit_sha: String,
    pub target_branch: String,
    pub target_commit_sha: String,
    pub added: Vec<SemanticCard>,
    pub modified: Vec<SemanticCard>,
    pub removed_titles: Vec<String>,
    pub dlp_redactions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitLogEntry {
    pub commit_sha: String,
    pub author: String,
    pub timestamp: String,
    pub subject: String,
    pub body: String,
    pub changed_domains: Vec<String>,
    pub files_changed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntry {
    pub path: String,
    pub is_dir: bool,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: usize,
    pub line: String,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub cluster: String,
    pub namespace: String,
    pub container: String,
    pub pod: String,
    pub stream: String,
    pub message: String,
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogsQueryResult {
    pub endpoint: String,
    pub effective_query: String,
    pub range: String,
    pub entries: Vec<LogEntry>,
    pub truncated: bool,
    pub dlp_redactions: usize,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileExcerpt {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub content: String,
}

/// Service-side consolidated investigation bundle combining raw Git code evidence
/// and optional VictoriaLogs runtime log evidence in a single encrypted RPC round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvestigationBundle {
    pub repo: String,
    pub effective_branch: String,
    pub commit_sha: String,
    pub detected_intents: Vec<String>,
    pub steps: Vec<String>,
    pub tree: Vec<TreeEntry>,
    pub files: Vec<FileExcerpt>,
    pub grep_matches: Vec<GrepMatch>,
    pub commits: Vec<CommitLogEntry>,
    pub semantic_cards: Vec<SemanticCard>,
    pub total_cards_indexed: Option<usize>,
    pub branch_diff: Option<BranchBehaviorDiff>,
    pub logs: Option<LogsQueryResult>,
    pub dlp_redactions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeMetric {
    pub repo: String,
    pub branch: String,
    pub commit_sha: String,
    pub disk_bytes: u64,
    pub idle_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchIndexMetric {
    pub repo: String,
    pub branch: String,
    pub commit_sha: String,
    pub total_cards: usize,
    pub disk_bytes: u64,
    pub idle_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoDiskMetric {
    pub name: String,
    pub mode: String,
    pub repo_disk_bytes: u64,
    pub worktrees_count: usize,
    pub worktrees_disk_bytes: u64,
    pub cached_indexes_count: usize,
    pub indexes_disk_bytes: u64,
    pub branches_count: usize,
    pub active_branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySnapshotMetric {
    pub repo: String,
    pub branch: String,
    pub commit_sha: String,
    pub files_count: usize,
    pub tree_entries_count: usize,
    pub idle_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerMetrics {
    pub server_version: String,
    pub uptime_secs: u64,
    pub metrics_listen: Option<String>,
    pub total_requests: u64,
    pub total_dlp_redactions: u64,
    pub config_reloads_total: u64,
    pub gc_evictions_total: u64,
    pub max_worktrees: usize,
    pub max_cached_indexes: usize,
    pub max_memory_snapshots: usize,
    pub max_snapshot_memory_mb: u64,
    pub rayon_threads: usize,
    pub worktree_idle_ttl_secs: u64,
    pub gc_interval_secs: u64,
    pub disk_warn_threshold_mb: u64,
    pub config_reload_interval_secs: u64,
    pub repos_disk_bytes: u64,
    pub worktrees_disk_bytes: u64,
    pub indexes_disk_bytes: u64,
    pub blob_cache_disk_bytes: u64,
    pub audit_log_bytes: u64,
    pub total_disk_bytes: u64,
    pub disk_over_threshold: bool,
    pub snapshot_memory_bytes: u64,
    pub snapshot_blobs_cached: usize,
    pub repos: Vec<RepoDiskMetric>,
    pub worktrees: Vec<WorktreeMetric>,
    pub cached_indexes: Vec<BranchIndexMetric>,
    pub memory_snapshots: Vec<MemorySnapshotMetric>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistillRequest {
    Ping,
    ServerStatus,
    ListRepos,
    ListBranches {
        repo: String,
    },
    FetchRepo {
        repo: String,
    },
    SearchSemantics {
        repo: String,
        branch: String,
        query: String,
        kind: Option<SemanticKind>,
        limit: Option<usize>,
    },
    GetApiContracts {
        repo: String,
        branch: String,
        keyword: String,
        limit: Option<usize>,
    },
    GetDomainRules {
        repo: String,
        branch: String,
        topic: String,
        limit: Option<usize>,
    },
    DiffBranchBehavior {
        repo: String,
        base_branch: String,
        target_branch: String,
    },
    RecentCommits {
        repo: String,
        branch: String,
        limit: Option<usize>,
    },
    ListTree {
        repo: String,
        branch: String,
        path: Option<String>,
        max_depth: Option<usize>,
        limit: Option<usize>,
    },
    GrepRepo {
        repo: String,
        branch: String,
        pattern: String,
        path_prefix: Option<String>,
        case_insensitive: bool,
        context_lines: Option<usize>,
        limit: Option<usize>,
    },
    ReadFile {
        repo: String,
        branch: String,
        path: String,
        start_line: Option<usize>,
        end_line: Option<usize>,
    },
    QueryLogs {
        query: String,
        range: Option<String>,
        start: Option<String>,
        end: Option<String>,
        limit: Option<usize>,
        repo_hint: Option<String>,
    },
    Investigate {
        repo: String,
        branch: String,
        question: String,
        /// None = auto-detect log intent on server side; Some(true) = force query VictoriaLogs; Some(false) = skip logs
        include_logs: Option<bool>,
        log_range: Option<String>,
    },
    Metrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistillResponse {
    Pong {
        server_version: String,
        protocol: String,
    },
    Status(ServerStatusInfo),
    Repos(Vec<RepoSummary>),
    Branches {
        repo: String,
        branches: Vec<BranchInfo>,
    },
    FetchResult(FetchRepoResult),
    SearchResult(SearchResultPayload),
    BranchDiff(BranchBehaviorDiff),
    Error {
        code: String,
        message: String,
    },
    Commits {
        repo: String,
        branch: String,
        commits: Vec<CommitLogEntry>,
        dlp_redactions: usize,
    },
    Tree {
        repo: String,
        branch: String,
        commit_sha: String,
        path: String,
        entries: Vec<TreeEntry>,
        truncated: bool,
    },
    GrepResult {
        repo: String,
        branch: String,
        commit_sha: String,
        pattern: String,
        matches: Vec<GrepMatch>,
        truncated: bool,
        dlp_redactions: usize,
    },
    FileContent {
        repo: String,
        branch: String,
        commit_sha: String,
        path: String,
        start_line: usize,
        end_line: usize,
        total_lines: usize,
        content: String,
        truncated: bool,
        dlp_redactions: usize,
    },
    Logs(LogsQueryResult),
    Investigation(InvestigationBundle),
    Metrics(ServerMetrics),
}

#[derive(Debug, Error)]
pub enum FrameError {
    #[error("frame exceeds limit ({size} > {MAX_FRAME_BYTES})")]
    TooLarge { size: usize },
    #[error("serialization error: {0}")]
    Codec(#[from] postcard::Error),
}

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let payload = postcard::to_allocvec(value)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge { size: payload.len() });
    }
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

pub fn decode_payload<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, FrameError> {
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLarge { size: bytes.len() });
    }
    Ok(postcard::from_bytes(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let req = DistillRequest::SearchSemantics {
            repo: "twosite".into(),
            branch: "dev/v1".into(),
            query: "价值点 集测1轮".into(),
            kind: Some(SemanticKind::ApiEndpoint),
            limit: Some(10),
        };
        let framed = encode_frame(&req).unwrap();
        let len = u32::from_be_bytes(framed[..4].try_into().unwrap()) as usize;
        assert_eq!(len, framed.len() - 4);
        let decoded: DistillRequest = decode_payload(&framed[4..]).unwrap();
        assert_eq!(decoded, req);
    }

    #[test]
    fn resolve_home_respects_default_dot_dir() {
        let path = resolve_app_home("distill", "DISTILL_HOME_NONEXISTENT_TEST");
        assert!(path.ends_with(".distill"));
    }
}
