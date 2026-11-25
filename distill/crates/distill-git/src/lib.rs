mod snapshot;

pub use snapshot::{CommitRawSnapshot, RawBlob, RawSnapshotCache, SnapshotPoolMetrics};

use anyhow::{Context, Result, anyhow};
use dashmap::DashMap;
use distill_crypto::DlpGate;
use gix::bstr::ByteSlice;
use lru::LruCache;
use distill_proto::{
    BranchInfo, CommitLogEntry, GrepMatch, RepoStatusInfo, TreeEntry, WorktreeMetric,
};
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{debug, info, warn};

const MAX_INDEXABLE_BLOB_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone)]
pub struct RepoConfigSpec {
    pub name: String,
    pub description: String,
    pub remote_url: Option<String>,
    pub local_path: Option<PathBuf>,
    pub default_branch: String,
    pub allowed_branches: Vec<String>,
    pub fetch_interval_secs: u64,
}

#[derive(Debug, Clone)]
pub struct TreeBlobEntry {
    pub path: String,
    pub blob_oid: String,
}

#[derive(Debug)]
pub struct WorktreeLease {
    pub path: PathBuf,
    pub repo_name: String,
    pub commit_sha: String,
}

struct WorktreeEntry {
    lease: Arc<WorktreeLease>,
    branch: String,
    git_dir: PathBuf,
    last_used_at: Instant,
}

/// Manages detached read-only git worktrees with reference-counted leases,
/// activity tracking, and periodic LRU / idle-TTL eviction.
pub struct WorktreePool {
    root: PathBuf,
    max_entries: AtomicUsize,
    entries: Mutex<LruCache<(String, String), WorktreeEntry>>,
}

impl WorktreePool {
    pub fn new(root: PathBuf, max_entries: usize) -> Self {
        let cap = NonZeroUsize::new(max_entries.max(1)).unwrap();
        Self {
            root,
            max_entries: AtomicUsize::new(cap.get()),
            entries: Mutex::new(LruCache::new(cap)),
        }
    }

    pub fn set_max_entries(&self, max_entries: usize) {
        let cap = NonZeroUsize::new(max_entries.max(1)).unwrap();
        self.max_entries.store(cap.get(), Ordering::Relaxed);
        let mut guard = self.entries.lock().unwrap();
        if guard.cap().get() < cap.get() {
            guard.resize(cap);
        }
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries.load(Ordering::Relaxed)
    }

    /// Acquire a read-only worktree for `(repo_name, commit_sha)`.
    /// Concurrent callers for the same commit share the same Arc<WorktreeLease>.
    pub fn acquire(
        &self,
        repo_name: &str,
        branch: &str,
        git_dir: &Path,
        commit_sha: &str,
    ) -> Result<Arc<WorktreeLease>> {
        let key = (repo_name.to_string(), commit_sha.to_string());
        {
            let mut guard = self.entries.lock().unwrap();
            if let Some(entry) = guard.get_mut(&key)
                && entry.lease.path.exists()
            {
                entry.last_used_at = Instant::now();
                if !branch.is_empty() {
                    entry.branch = branch.to_string();
                }
                return Ok(Arc::clone(&entry.lease));
            }
        }

        let short_sha = &commit_sha[..commit_sha.len().min(12)];
        let wt_dir = self.root.join(repo_name).join(short_sha);
        if let Some(parent) = wt_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !wt_dir.exists() {
            let status = Command::new("git")
                .arg("--git-dir")
                .arg(git_dir)
                .args(["worktree", "add", "--detach", "--force"])
                .arg(&wt_dir)
                .arg(commit_sha)
                .status()
                .with_context(|| format!("failed to invoke git worktree add for {repo_name}@{short_sha}"))?;
            if !status.success() {
                return Err(anyhow!(
                    "git worktree add failed for {repo_name}@{short_sha} with status {status}"
                ));
            }
        }

        let lease = Arc::new(WorktreeLease {
            path: wt_dir,
            repo_name: repo_name.to_string(),
            commit_sha: commit_sha.to_string(),
        });

        let max_cap = self.max_entries.load(Ordering::Relaxed);
        let mut evicted_paths = Vec::new();
        {
            let mut guard = self.entries.lock().unwrap();
            while guard.len() >= max_cap {
                if let Some((evict_key, evict_entry)) = guard.peek_lru() {
                    if Arc::strong_count(&evict_entry.lease) == 1 {
                        let (_, removed) = guard.pop_lru().unwrap();
                        evicted_paths.push((
                            removed.lease.repo_name.clone(),
                            removed.git_dir.clone(),
                            removed.lease.path.clone(),
                        ));
                        continue;
                    } else {
                        debug!(
                            "skipping busy worktree {:?} (active leases={})",
                            evict_key,
                            Arc::strong_count(&evict_entry.lease) - 1
                        );
                        break;
                    }
                } else {
                    break;
                }
            }
            guard.put(
                key,
                WorktreeEntry {
                    lease: Arc::clone(&lease),
                    branch: branch.to_string(),
                    git_dir: git_dir.to_path_buf(),
                    last_used_at: Instant::now(),
                },
            );
        }

        for (ev_repo, ev_git_dir, ev_path) in evicted_paths {
            remove_worktree_dir(&ev_git_dir, &ev_path);
            info!("evicted idle worktree for {ev_repo}: {}", ev_path.display());
        }

        Ok(lease)
    }

    /// Periodic LRU & idle-TTL garbage collection for worktrees + orphan disk dirs.
    pub fn gc(&self, max_keep: usize, idle_ttl_secs: u64) -> usize {
        let keep_cap = max_keep.max(1);
        self.set_max_entries(keep_cap);

        let mut evicted: Vec<(String, PathBuf, PathBuf)> = Vec::new();
        let active_paths: HashSet<PathBuf>;

        {
            let mut guard = self.entries.lock().unwrap();

            // 1. Collect entries that exceeded idle TTL (if configured)
            if idle_ttl_secs > 0 {
                let ttl = Duration::from_secs(idle_ttl_secs);
                let expired_keys: Vec<(String, String)> = guard
                    .iter()
                    .filter(|(_, e)| {
                        Arc::strong_count(&e.lease) == 1 && e.last_used_at.elapsed() >= ttl
                    })
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in expired_keys {
                    if let Some(removed) = guard.pop(&k) {
                        evicted.push((
                            removed.lease.repo_name.clone(),
                            removed.git_dir.clone(),
                            removed.lease.path.clone(),
                        ));
                    }
                }
            }

            // 2. Enforce LRU max_keep threshold
            while guard.len() > keep_cap {
                if let Some((_, lru_entry)) = guard.peek_lru() {
                    if Arc::strong_count(&lru_entry.lease) == 1 {
                        let (_, removed) = guard.pop_lru().unwrap();
                        evicted.push((
                            removed.lease.repo_name.clone(),
                            removed.git_dir.clone(),
                            removed.lease.path.clone(),
                        ));
                        continue;
                    }
                }
                break;
            }

            active_paths = guard.iter().map(|(_, e)| e.lease.path.clone()).collect();
        }

        let mut removed_count = 0usize;
        for (repo, git_dir, path) in evicted {
            remove_worktree_dir(&git_dir, &path);
            info!("gc evicted worktree for {repo}: {}", path.display());
            removed_count += 1;
        }

        // 3. Clean orphaned worktree dirs on disk (e.g. from prior process restart)
        if self.root.is_dir()
            && let Ok(repo_dirs) = std::fs::read_dir(&self.root)
        {
            for repo_entry in repo_dirs.flatten() {
                let repo_dir = repo_entry.path();
                if !repo_dir.is_dir() {
                    continue;
                }
                if let Ok(wt_dirs) = std::fs::read_dir(&repo_dir) {
                    for wt_entry in wt_dirs.flatten() {
                        let wt_path = wt_entry.path();
                        if wt_path.is_dir() && !active_paths.contains(&wt_path) {
                            let _ = std::fs::remove_dir_all(&wt_path);
                            info!("gc pruned orphan worktree dir: {}", wt_path.display());
                            removed_count += 1;
                        }
                    }
                }
            }
        }

        removed_count
    }

    pub fn snapshot_metrics(&self) -> Vec<WorktreeMetric> {
        let items: Vec<(String, String, String, PathBuf, u64)> = {
            let guard = self.entries.lock().unwrap();
            guard
                .iter()
                .map(|(_, e)| {
                    (
                        e.lease.repo_name.clone(),
                        e.branch.clone(),
                        e.lease.commit_sha.clone(),
                        e.lease.path.clone(),
                        e.last_used_at.elapsed().as_secs(),
                    )
                })
                .collect()
        };
        items
            .into_iter()
            .map(|(repo, branch, commit_sha, path, idle_secs)| WorktreeMetric {
                repo,
                branch,
                commit_sha,
                disk_bytes: dir_size_bytes(&path),
                idle_secs,
            })
            .collect()
    }
}

fn remove_worktree_dir(git_dir: &Path, wt_path: &Path) {
    let _ = Command::new("git")
        .arg("--git-dir")
        .arg(git_dir)
        .args(["worktree", "remove", "--force"])
        .arg(wt_path)
        .status();
    let _ = std::fs::remove_dir_all(wt_path);
}

/// Recursively compute disk bytes for a file or directory without following symlinks.
pub fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            let Ok(m) = std::fs::symlink_metadata(&p) else {
                continue;
            };
            if m.is_file() {
                total = total.saturating_add(m.len());
            } else if m.is_dir() {
                stack.push(p);
            }
        }
    }
    total
}

struct FetchState {
    last_fetch: Option<Instant>,
    last_ok: bool,
    last_note: String,
}

/// Short TTL for in-memory `(repo, branch) -> (commit_sha, repo_path)` resolution cache.
/// Immediately invalidated on `force_fetch` and `reload_repos`.
const REF_CACHE_TTL_AUTO: Duration = Duration::from_secs(5);
const REF_CACHE_TTL_MANUAL: Duration = Duration::from_secs(30);

pub struct GitRepoManager {
    repos_root: PathBuf,
    configs: DashMap<String, RepoConfigSpec>,
    fetch_locks: DashMap<String, Arc<AsyncMutex<FetchState>>>,
    ref_cache: DashMap<String, DashMap<String, (Arc<str>, Arc<PathBuf>, Instant)>>,
    worktrees: WorktreePool,
    snapshots: Arc<RawSnapshotCache>,
}

impl GitRepoManager {
    pub fn new(app_home: &Path, specs: Vec<RepoConfigSpec>, max_worktrees: usize) -> Self {
        Self::new_with_limits(app_home, specs, max_worktrees, 10, 512, 0)
    }

    pub fn new_with_limits(
        app_home: &Path,
        specs: Vec<RepoConfigSpec>,
        max_worktrees: usize,
        max_memory_snapshots: usize,
        max_snapshot_memory_mb: u64,
        rayon_threads: usize,
    ) -> Self {
        let configs = DashMap::new();
        let fetch_locks = DashMap::new();
        for spec in specs {
            let init_note = if spec.fetch_interval_secs == 0 {
                if spec.local_path.is_some() {
                    "手动 Fetch 模式（本地裸仓）".to_string()
                } else {
                    "手动 Fetch 模式（仅客户端触发）".to_string()
                }
            } else if spec.local_path.is_some() {
                "本地镜像模式".to_string()
            } else {
                "等待首次同步".to_string()
            };
            fetch_locks.insert(
                spec.name.clone(),
                Arc::new(AsyncMutex::new(FetchState {
                    last_fetch: None,
                    last_ok: true,
                    last_note: init_note,
                })),
            );
            configs.insert(spec.name.clone(), spec);
        }
        Self {
            repos_root: app_home.join("repos"),
            configs,
            fetch_locks,
            ref_cache: DashMap::new(),
            worktrees: WorktreePool::new(app_home.join("worktrees"), max_worktrees),
            snapshots: Arc::new(RawSnapshotCache::new(
                max_memory_snapshots,
                max_snapshot_memory_mb,
                rayon_threads,
            )),
        }
    }

    pub fn set_snapshot_limits(&self, max_snapshots: usize, max_memory_mb: u64) {
        self.snapshots.set_limits(max_snapshots, max_memory_mb);
    }

    pub fn gc_snapshots(&self, max_keep: usize, max_memory_mb: u64, idle_ttl_secs: u64) -> usize {
        self.snapshots.gc(max_keep, max_memory_mb, idle_ttl_secs)
    }

    pub fn snapshot_metrics(&self) -> SnapshotPoolMetrics {
        self.snapshots.snapshot_metrics()
    }

    pub fn rayon_pool(&self) -> &Arc<rayon::ThreadPool> {
        self.snapshots.rayon_pool()
    }

    pub fn active_worktree_count(&self) -> usize {
        self.worktrees.entries.lock().unwrap().len()
    }

    pub fn set_max_worktrees(&self, max_worktrees: usize) {
        self.worktrees.set_max_entries(max_worktrees);
    }

    pub fn gc_worktrees(&self, max_keep: usize, idle_ttl_secs: u64) -> usize {
        self.worktrees.gc(max_keep, idle_ttl_secs)
    }

    pub fn worktree_metrics(&self) -> Vec<WorktreeMetric> {
        self.worktrees.snapshot_metrics()
    }

    pub fn repo_disk_path(&self, repo: &str) -> Option<PathBuf> {
        let spec = self.configs.get(repo)?;
        if let Some(ref local) = spec.local_path {
            Some(local.clone())
        } else {
            Some(self.repos_root.join(format!("{}.git", spec.name)))
        }
    }

    pub fn invalidate_ref_cache(&self, repo: Option<&str>) {
        match repo {
            Some(r) => {
                self.ref_cache.remove(r);
            }
            None => self.ref_cache.clear(),
        }
    }

    pub fn reload_repos(&self, specs: Vec<RepoConfigSpec>) {
        let keep_names: HashSet<String> = specs.iter().map(|s| s.name.clone()).collect();
        self.configs.retain(|k, _| keep_names.contains(k));
        self.fetch_locks.retain(|k, _| keep_names.contains(k));
        self.ref_cache.clear();
        for spec in specs {
            let init_note = if spec.fetch_interval_secs == 0 {
                if spec.local_path.is_some() {
                    "手动 Fetch 模式（本地裸仓）".to_string()
                } else {
                    "手动 Fetch 模式（仅客户端触发）".to_string()
                }
            } else if spec.local_path.is_some() {
                "本地镜像模式".to_string()
            } else {
                "等待首次同步".to_string()
            };
            self.fetch_locks.entry(spec.name.clone()).or_insert_with(|| {
                Arc::new(AsyncMutex::new(FetchState {
                    last_fetch: None,
                    last_ok: true,
                    last_note: init_note,
                }))
            });
            self.configs.insert(spec.name.clone(), spec);
        }
    }

    pub async fn repo_statuses(&self) -> Vec<RepoStatusInfo> {
        let specs = self.list_repo_specs();
        let mut out = Vec::with_capacity(specs.len());
        for spec in specs {
            let mode = if spec.local_path.is_some() {
                "local_mirror".to_string()
            } else {
                "remote_bare".to_string()
            };
            let (secs_ago, ok, note) = if let Some(lock) = self.fetch_locks.get(&spec.name) {
                let st = lock.lock().await;
                (
                    st.last_fetch.map(|t| t.elapsed().as_secs()),
                    st.last_ok,
                    st.last_note.clone(),
                )
            } else {
                (None, true, String::new())
            };
            out.push(RepoStatusInfo {
                name: spec.name,
                mode,
                default_branch: spec.default_branch,
                last_fetch_secs_ago: secs_ago,
                last_fetch_ok: ok,
                last_fetch_note: note,
            });
        }
        out
    }

    /// Force an immediate `git fetch --prune` for a repository (triggered by client Fetch action).
    pub async fn force_fetch(&self, repo: &str) -> Result<(bool, String)> {
        let repo_path = self.ensure_ready(repo).await?;
        let is_bare = !repo_path.join(".git").is_dir();
        let git_dir = if is_bare {
            repo_path
        } else {
            repo_path.join(".git")
        };

        let lock = self
            .fetch_locks
            .get(repo)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| anyhow!("missing fetch lock for {repo}"))?;
        let mut state = lock.lock().await;

        let dir = git_dir.clone();
        let output = tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new("git");
            cmd.arg("--git-dir")
                .arg(&dir)
                .env("GIT_TERMINAL_PROMPT", "0");
            if is_bare {
                cmd.args(["fetch", "--prune", "origin", "+refs/heads/*:refs/heads/*"]);
            } else {
                cmd.args(["fetch", "--prune", "origin"]);
            }
            cmd.output()
        })
        .await??;

        state.last_fetch = Some(Instant::now());
        if output.status.success() {
            self.invalidate_ref_cache(Some(repo));
            state.last_ok = true;
            state.last_note = "Fetch 成功，分支已同步".into();
            Ok((true, state.last_note.clone()))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let first_line = stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("远端未配置上游 Git 凭据")
                .trim();
            let msg = if first_line.contains("could not read Username")
                || first_line.contains("Permission denied")
                || first_line.contains("Authentication failed")
            {
                "远端未配置 Git 凭据（当前使用本地裸仓镜像；配好只读凭据后可自动拉取上游）".to_string()
            } else {
                format!("Fetch 跳过/失败：{first_line}")
            };
            state.last_ok = false;
            state.last_note = msg.clone();
            Ok((false, msg))
        }
    }

    pub fn list_repo_specs(&self) -> Vec<RepoConfigSpec> {
        let mut out: Vec<_> = self.configs.iter().map(|e| e.value().clone()).collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn get_spec(&self, repo: &str) -> Result<RepoConfigSpec> {
        self.configs
            .get(repo)
            .map(|r| r.value().clone())
            .ok_or_else(|| anyhow!("repository '{repo}' is not registered"))
    }

    pub fn branch_allowed(&self, repo: &str, branch: &str) -> Result<bool> {
        let spec = self
            .configs
            .get(repo)
            .ok_or_else(|| anyhow!("repository '{repo}' is not registered"))?;
        if spec.allowed_branches.is_empty() {
            return Ok(true);
        }
        Ok(spec
            .allowed_branches
            .iter()
            .any(|pattern| wildcard_match(pattern, branch)))
    }

    /// Ensure the git repo exists locally (cloning bare mirror if remote_url is configured)
    /// and fetch updates if TTL expired. Returns the path suitable for `gix::open`.
    pub async fn ensure_ready(&self, repo: &str) -> Result<PathBuf> {
        let spec = self.get_spec(repo)?;
        if let Some(local) = &spec.local_path {
            if !local.exists() {
                return Err(anyhow!("local repo path does not exist: {}", local.display()));
            }
            return Ok(local.clone());
        }

        let remote_url = spec
            .remote_url
            .as_deref()
            .ok_or_else(|| anyhow!("repo '{}' has neither local_path nor remote_url", spec.name))?;

        let bare_dir = self.repos_root.join(format!("{}.git", spec.name));
        let lock = self
            .fetch_locks
            .get(repo)
            .map(|e| Arc::clone(e.value()))
            .ok_or_else(|| anyhow!("missing fetch lock for {repo}"))?;

        let mut fetch_state = lock.lock().await;
        let auto_fetch_enabled = spec.fetch_interval_secs > 0;
        let ttl = Duration::from_secs(spec.fetch_interval_secs.max(5));

        if !bare_dir.exists() {
            std::fs::create_dir_all(&self.repos_root)?;
            info!("cloning bare mirror for repo '{}'", spec.name);
            let url = remote_url.to_string();
            let dir = bare_dir.clone();
            let status = tokio::task::spawn_blocking(move || {
                Command::new("git")
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .args(["clone", "--bare"])
                    .arg(&url)
                    .arg(&dir)
                    .status()
            })
            .await??;
            if !status.success() {
                return Err(anyhow!("git clone --bare failed for '{}' with {status}", spec.name));
            }
            fetch_state.last_fetch = Some(Instant::now());
            fetch_state.last_ok = true;
            fetch_state.last_note = if auto_fetch_enabled {
                "已初始化裸仓".into()
            } else {
                "已初始化裸仓（手动 Fetch 模式）".into()
            };
        } else if auto_fetch_enabled
            && fetch_state
                .last_fetch
                .map(|t| t.elapsed() >= ttl)
                .unwrap_or(true)
        {
            let dir = bare_dir.clone();
            let name = spec.name.clone();
            let res = tokio::task::spawn_blocking(move || {
                Command::new("git")
                    .arg("--git-dir")
                    .arg(&dir)
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .args(["fetch", "--prune", "origin", "+refs/heads/*:refs/heads/*"])
                    .status()
            })
            .await?;
            match res {
                Ok(s) if s.success() => {
                    fetch_state.last_fetch = Some(Instant::now());
                    fetch_state.last_ok = true;
                    fetch_state.last_note = "自动同步正常".into();
                }
                Ok(s) => {
                    fetch_state.last_ok = false;
                    fetch_state.last_note = format!("自动 fetch 返回 {s}");
                    warn!("git fetch for '{name}' returned {s}; serving cached refs");
                }
                Err(e) => {
                    fetch_state.last_ok = false;
                    fetch_state.last_note = format!("自动 fetch 异常: {e}");
                    warn!("git fetch for '{name}' errored: {e}; serving cached refs");
                }
            }
        }

        Ok(bare_dir)
    }

    /// List files and directories directly from the in-memory commit snapshot (microsecond latency).
    pub async fn list_tree(
        &self,
        repo: &str,
        branch: &str,
        subpath: Option<&str>,
        max_depth: usize,
        limit: usize,
    ) -> Result<(String, String, Vec<TreeEntry>, bool)> {
        let (commit_sha, repo_path) = self.resolve_branch_with_path(repo, branch).await?;
        let norm_sub = normalize_rel_path(subpath.unwrap_or(""))?;
        let snap = self
            .snapshots
            .get_or_build(repo, branch, &commit_sha, repo_path.as_path())
            .await?;
        let (entries, truncated) = snap.list_tree(&norm_sub, max_depth, limit)?;
        Ok((commit_sha.to_string(), norm_sub, entries, truncated))
    }

    /// Ultra-fast trigram-pruned + SIMD/Rayon in-memory grep over pre-sanitized blobs.
    /// Returns `(commit_sha, matches, truncated, dlp_redactions)` — matches are ALREADY secret-redacted.
    pub async fn grep_repo(
        &self,
        repo: &str,
        branch: &str,
        pattern: &str,
        path_prefix: Option<&str>,
        case_insensitive: bool,
        context_lines: usize,
        limit: usize,
    ) -> Result<(String, Vec<GrepMatch>, bool, usize)> {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("grep pattern must not be empty"));
        }
        let (commit_sha, repo_path) = self.resolve_branch_with_path(repo, branch).await?;
        let norm_prefix = normalize_rel_path(path_prefix.unwrap_or(""))?;
        let snap = self
            .snapshots
            .get_or_build(repo, branch, &commit_sha, repo_path.as_path())
            .await?;

        // Single-pass adaptive execution:
        // - Trigram miss or sequential head probe (<= 24 files) completes inline with ZERO thread wake-up overhead.
        // - Only sparse queries that exhaust the head probe fan out across the dedicated 12-thread Rayon pool!
        let (matches, truncated, dlp) = snap.grep(
            self.snapshots.rayon_pool(),
            trimmed,
            &norm_prefix,
            case_insensitive,
            context_lines,
            limit,
        )?;
        Ok((commit_sha.to_string(), matches, truncated, dlp))
    }

    /// Read any text file (or line range) in O(1) from the in-memory commit snapshot,
    /// falling back to direct `gix` blob read if the path was outside the searchable snapshot.
    /// Returns `(commit_sha, norm_path, start, end, total, sanitized_content, truncated, dlp_redactions)`.
    pub async fn read_file_lines(
        &self,
        repo: &str,
        branch: &str,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<(String, String, usize, usize, usize, String, bool, usize)> {
        let norm_path = normalize_rel_path(path)?;
        if norm_path.is_empty() {
            return Err(anyhow!("file path must not be empty"));
        }
        if is_secret_file_path(&norm_path) {
            return Err(anyhow!("敏感密钥/凭据文件 `{norm_path}` 受安全策略保护禁止直读"));
        }
        let (commit_sha, repo_path) = self.resolve_branch_with_path(repo, branch).await?;

        if let Ok(snap) = self
            .snapshots
            .get_or_build(repo, branch, &commit_sha, repo_path.as_path())
            .await
            && let Some((s, e, total, content, truncated, dlp)) =
                snap.read_lines(&norm_path, start_line, end_line)
        {
            return Ok((commit_sha.to_string(), norm_path, s, e, total, content, truncated, dlp));
        }

        let sha_clone = commit_sha.to_string();
        let path_clone = norm_path.clone();
        let repo_path_bg = (*repo_path).clone();

        let (actual_start, actual_end, total_lines, raw_content, truncated) =
            tokio::task::spawn_blocking(move || -> Result<(usize, usize, usize, String, bool)> {
                let git_repo = gix::open(&repo_path_bg)?;
                let id = gix::ObjectId::from_hex(sha_clone.as_bytes())?;
                let commit = git_repo.find_object(id)?.try_into_commit()?;
                let tree = commit.tree()?;
                let entry = tree
                    .lookup_entry_by_path(&path_clone)?
                    .ok_or_else(|| anyhow!("file not found in branch: {path_clone}"))?;
                if entry.mode().is_tree() {
                    return Err(anyhow!("`{path_clone}` is a directory, use `tree` instead"));
                }
                let obj = git_repo.find_object(entry.oid())?;
                if obj.data.len() > 1024 * 1024 {
                    return Err(anyhow!(
                        "file `{path_clone}` is too large ({} bytes > 1 MiB)",
                        obj.data.len()
                    ));
                }
                if obj.data.contains(&0) {
                    return Err(anyhow!("`{path_clone}` appears to be a binary file"));
                }
                let full_text = String::from_utf8_lossy(&obj.data);
                let all_lines: Vec<&str> = full_text.lines().collect();
                let total = all_lines.len();
                if total == 0 {
                    return Ok((1, 0, 0, String::new(), false));
                }
                let s = start_line.unwrap_or(1).clamp(1, total);
                let requested_end = end_line.unwrap_or(s + 299).clamp(s, total);
                let capped_end = requested_end.min(s + 599).min(total);
                let mut out = String::new();
                let mut truncated = capped_end < total && end_line.is_none();
                let mut final_end = s.saturating_sub(1);
                for (offset, line) in all_lines[(s - 1)..capped_end].iter().enumerate() {
                    let ln = s + offset;
                    let formatted = format!("{ln:>5} | {line}\n");
                    if out.len() + formatted.len() > 64 * 1024 {
                        truncated = true;
                        break;
                    }
                    out.push_str(&formatted);
                    final_end = ln;
                }
                Ok((s, final_end, total, out, truncated))
            })
            .await??;

        let (content, dlp) = DlpGate::sanitize_secrets(&raw_content);
        Ok((
            commit_sha.to_string(),
            norm_path,
            actual_start,
            actual_end,
            total_lines,
            content,
            truncated,
            dlp,
        ))
    }

    pub async fn recent_commits(
        &self,
        repo: &str,
        branch: &str,
        limit: usize,
    ) -> Result<Vec<CommitLogEntry>> {
        let spec = self.get_spec(repo)?;
        if !spec.allowed_branches.is_empty()
            && !spec
                .allowed_branches
                .iter()
                .any(|p| wildcard_match(p, branch))
        {
            return Err(anyhow!("branch '{branch}' is not allowed for repo '{repo}'"));
        }
        let commit_sha = self.resolve_branch(repo, branch).await?;
        let repo_path = self.ensure_ready(repo).await?;
        let git_dir = if repo_path.join(".git").is_dir() {
            repo_path.join(".git")
        } else {
            repo_path
        };
        let capped = limit.clamp(1, 50);

        tokio::task::spawn_blocking(move || {
            let output = Command::new("git")
                .arg("--git-dir")
                .arg(&git_dir)
                .args([
                    "log",
                    "-n",
                    &capped.to_string(),
                    "--date=iso-strict",
                    "--name-only",
                    "--format=%x1e%H%x1f%an%x1f%ad%x1f%s%x1f%b%x1f",
                    &commit_sha,
                ])
                .output()
                .context("failed to run git log for recent_commits")?;

            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!("git log failed: {}", err.trim()));
            }

            let raw = String::from_utf8_lossy(&output.stdout);
            let mut commits = Vec::new();
            for rec in raw.split('\x1e') {
                let rec = rec.trim();
                if rec.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = rec.split('\x1f').collect();
                if parts.len() < 5 {
                    continue;
                }
                let sha = parts[0].trim().to_string();
                let author = parts[1].trim().to_string();
                let timestamp = parts[2].trim().to_string();
                let subject = parts[3].trim().to_string();
                let body = parts[4].trim().chars().take(400).collect::<String>();
                let files_raw = parts.get(5).copied().unwrap_or("");

                let mut files_changed = 0usize;
                let mut domains = std::collections::BTreeSet::new();
                for line in files_raw.lines() {
                    let f = line.trim();
                    if f.is_empty() {
                        continue;
                    }
                    files_changed += 1;
                    domains.insert(path_to_high_level_domain(f));
                }

                commits.push(CommitLogEntry {
                    commit_sha: sha,
                    author,
                    timestamp,
                    subject,
                    body,
                    changed_domains: domains.into_iter().take(12).collect(),
                    files_changed,
                });
            }
            Ok(commits)
        })
        .await?
    }

    pub async fn list_branches(&self, repo: &str) -> Result<Vec<BranchInfo>> {
        let spec = self.get_spec(repo)?;
        let repo_path = self.ensure_ready(repo).await?;
        let allowed = spec.allowed_branches.clone();

        tokio::task::spawn_blocking(move || {
            let git_repo = gix::open(&repo_path)
                .with_context(|| format!("gix failed to open {}", repo_path.display()))?;
            let refs = git_repo.references()?;
            let mut seen = std::collections::BTreeMap::<String, (String, String)>::new();

            for reference in refs.local_branches()?.flatten() {
                let name = reference.name().shorten().to_str_lossy().to_string();
                if !allowed.is_empty() && !allowed.iter().any(|p| wildcard_match(p, &name)) {
                    continue;
                }
                if let Ok(id) = reference.into_fully_peeled_id() {
                    let sha = id.to_hex().to_string();
                    let summary = commit_first_line(&git_repo, id.detach());
                    seen.insert(name, (sha, summary));
                }
            }

            // Also inspect origin/* remote tracking branches for non-bare checkouts
            if let Ok(remote_refs) = refs.remote_branches() {
                for reference in remote_refs.flatten() {
                    let short = reference.name().shorten().to_str_lossy().to_string();
                    if let Some(name) = short.strip_prefix("origin/") {
                        if name == "HEAD" || seen.contains_key(name) {
                            continue;
                        }
                        if !allowed.is_empty() && !allowed.iter().any(|p| wildcard_match(p, name)) {
                            continue;
                        }
                        if let Ok(id) = reference.into_fully_peeled_id() {
                            let sha = id.to_hex().to_string();
                            let summary = commit_first_line(&git_repo, id.detach());
                            seen.insert(name.to_string(), (sha, summary));
                        }
                    }
                }
            }

            Ok(seen
                .into_iter()
                .map(|(name, (commit_sha, commit_summary))| BranchInfo {
                    name,
                    commit_sha,
                    commit_summary,
                    indexed_cards: None,
                })
                .collect())
        })
        .await?
    }

    pub async fn resolve_branch(&self, repo: &str, branch: &str) -> Result<String> {
        let (sha, _) = self.resolve_branch_with_path(repo, branch).await?;
        Ok(sha.to_string())
    }

    /// Resolve `(repo, branch) -> (commit_sha, repo_path)` with ZERO heap allocations on hot cache hit,
    /// plus direct loose-ref / `packed-refs` fast reader (~15µs) before falling back to `gix::open`.
    async fn resolve_branch_with_path(
        &self,
        repo: &str,
        branch: &str,
    ) -> Result<(Arc<str>, Arc<PathBuf>)> {
        let ttl = {
            let spec = self
                .configs
                .get(repo)
                .ok_or_else(|| anyhow!("repository '{repo}' is not registered"))?;
            if !spec.allowed_branches.is_empty()
                && !spec
                    .allowed_branches
                    .iter()
                    .any(|pattern| wildcard_match(pattern, branch))
            {
                return Err(anyhow!("branch '{branch}' is not permitted for repo '{repo}'"));
            }
            if spec.fetch_interval_secs == 0 {
                REF_CACHE_TTL_MANUAL
            } else {
                REF_CACHE_TTL_AUTO
            }
        };

        // Zero-allocation borrowed `&str` lookup on hot cache hit
        if let Some(repo_map) = self.ref_cache.get(repo)
            && let Some(entry) = repo_map.get(branch)
        {
            let (sha, path, cached_at) = entry.value();
            if cached_at.elapsed() < ttl {
                return Ok((Arc::clone(sha), Arc::clone(path)));
            }
        }

        let repo_path = Arc::new(self.ensure_ready(repo).await?);
        let git_dir = if repo_path.join(".git").is_dir() {
            repo_path.join(".git")
        } else {
            (*repo_path).clone()
        };

        // Fast path 1 & 2: read loose ref file OR `packed-refs` directly (~15µs) without `gix::open`
        if !branch.contains("..") && !branch.starts_with('/') {
            let ref_head = format!("refs/heads/{branch}");
            let ref_remote = format!("refs/remotes/origin/{branch}");
            for rel in [&ref_head, &ref_remote] {
                if let Ok(raw) = std::fs::read_to_string(git_dir.join(rel)) {
                    let trimmed = raw.trim();
                    if trimmed.len() == 40 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
                        let sha: Arc<str> = Arc::from(trimmed);
                        self.ref_cache
                            .entry(repo.to_string())
                            .or_default()
                            .insert(branch.to_string(), (Arc::clone(&sha), Arc::clone(&repo_path), Instant::now()));
                        return Ok((sha, repo_path));
                    }
                }
            }
            if let Ok(packed) = std::fs::read_to_string(git_dir.join("packed-refs")) {
                let mut found_remote: Option<&str> = None;
                for line in packed.lines() {
                    if line.starts_with('#') || line.starts_with('^') || line.len() < 42 {
                        continue;
                    }
                    if let Some((oid, rname)) = line.split_once(' ')
                        && oid.len() == 40
                        && oid.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        if rname == ref_head {
                            let sha: Arc<str> = Arc::from(oid);
                            self.ref_cache
                                .entry(repo.to_string())
                                .or_default()
                                .insert(branch.to_string(), (Arc::clone(&sha), Arc::clone(&repo_path), Instant::now()));
                            return Ok((sha, repo_path));
                        } else if rname == ref_remote {
                            found_remote = Some(oid);
                        }
                    }
                }
                if let Some(oid) = found_remote {
                    let sha: Arc<str> = Arc::from(oid);
                    self.ref_cache
                        .entry(repo.to_string())
                        .or_default()
                        .insert(branch.to_string(), (Arc::clone(&sha), Arc::clone(&repo_path), Instant::now()));
                    return Ok((sha, repo_path));
                }
            }
        }

        let repo_path_bg = (*repo_path).clone();
        let branch_owned = branch.to_string();
        let sha_str = tokio::task::spawn_blocking(move || {
            let git_repo = gix::open(&repo_path_bg)?;
            let candidates = [
                format!("refs/heads/{branch_owned}"),
                format!("refs/remotes/origin/{branch_owned}"),
                branch_owned.clone(),
            ];
            for candidate in candidates {
                if let Ok(id) = git_repo.rev_parse_single(candidate.as_str()) {
                    let commit = id.object()?.peel_to_commit()?;
                    return Ok(commit.id().to_hex().to_string());
                }
            }
            Err(anyhow!("branch '{branch_owned}' not found"))
        })
        .await??;

        let sha: Arc<str> = Arc::from(sha_str);
        self.ref_cache
            .entry(repo.to_string())
            .or_default()
            .insert(branch.to_string(), (Arc::clone(&sha), Arc::clone(&repo_path), Instant::now()));
        Ok((sha, repo_path))
    }

    /// Pure Rust zero-checkout tree walk at a specific commit SHA.
    pub async fn list_indexable_blobs(
        &self,
        repo: &str,
        commit_sha: &str,
    ) -> Result<Vec<TreeBlobEntry>> {
        let repo_path = self.ensure_ready(repo).await?;
        let sha = commit_sha.to_string();
        tokio::task::spawn_blocking(move || {
            let git_repo = gix::open(&repo_path)?;
            let oid = gix::ObjectId::from_hex(sha.as_bytes())?;
            let commit = git_repo.find_commit(oid)?;
            let tree = commit.tree()?;
            let mut entries = Vec::new();
            walk_tree(&git_repo, &tree, "", &mut entries)?;
            Ok(entries)
        })
        .await?
    }

    /// Pure Rust zero-checkout blob read directly from git object database.
    pub async fn read_blob_utf8(&self, repo: &str, blob_oid: &str) -> Result<Option<String>> {
        let repo_path = self.ensure_ready(repo).await?;
        let oid_str = blob_oid.to_string();
        tokio::task::spawn_blocking(move || {
            let git_repo = gix::open(&repo_path)?;
            let oid = gix::ObjectId::from_hex(oid_str.as_bytes())?;
            let blob = git_repo.find_blob(oid)?;
            if blob.data.len() > MAX_INDEXABLE_BLOB_BYTES {
                return Ok(None);
            }
            if blob.data.contains(&0u8) {
                return Ok(None);
            }
            Ok(std::str::from_utf8(&blob.data).ok().map(|s| s.to_string()))
        })
        .await?
    }

    /// Acquire a read-only detached worktree lease when physical filesystem view is needed.
    pub async fn acquire_worktree(
        &self,
        repo: &str,
        branch: &str,
        commit_sha: &str,
    ) -> Result<Arc<WorktreeLease>> {
        let repo_path = self.ensure_ready(repo).await?;
        let git_dir = if repo_path.join(".git").is_dir() {
            repo_path.join(".git")
        } else {
            repo_path
        };
        let repo_name = repo.to_string();
        let sha = commit_sha.to_string();
        let pool = &self.worktrees;
        pool.acquire(&repo_name, branch, &git_dir, &sha)
    }
}

fn commit_first_line(repo: &gix::Repository, oid: gix::ObjectId) -> String {
    repo.find_commit(oid)
        .ok()
        .and_then(|c| c.message_raw().ok().map(|m| m.to_str_lossy().to_string()))
        .and_then(|m| m.lines().next().map(|l| l.trim().chars().take(60).collect()))
        .unwrap_or_default()
}

fn walk_tree(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: &str,
    out: &mut Vec<TreeBlobEntry>,
) -> Result<()> {
    for entry_res in tree.iter() {
        let entry = entry_res?;
        let name = entry.filename().to_str_lossy();
        if should_skip_dir_or_file(&name) {
            continue;
        }
        let rel_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode().is_tree() {
            if should_skip_path(&rel_path) {
                continue;
            }
            let subtree = repo.find_tree(entry.oid())?;
            walk_tree(repo, &subtree, &rel_path, out)?;
        } else if entry.mode().is_blob() && is_indexable_path(&rel_path) {
            out.push(TreeBlobEntry {
                path: rel_path,
                blob_oid: entry.oid().to_hex().to_string(),
            });
        }
    }
    Ok(())
}

pub(crate) fn should_skip_tree_name(name: &str) -> bool {
    should_skip_dir_or_file(name)
}

fn should_skip_dir_or_file(name: &str) -> bool {
    matches!(
        name,
        "node_modules"
            | "dist"
            | "coverage"
            | ".git"
            | ".turbo"
            | "_ai"
            | "target"
            | "pnpm-lock.yaml"
            | "package-lock.json"
            | "Cargo.lock"
    )
}

fn should_skip_path(rel_path: &str) -> bool {
    rel_path.starts_with("outputs/")
        || rel_path.starts_with("deploy/sql/prod-init")
        || rel_path.contains("/test/")
        || rel_path.contains("/fixtures/")
}

fn is_indexable_path(path: &str) -> bool {
    if path.ends_with(".spec.ts")
        || path.ends_with(".spec.tsx")
        || path.ends_with(".test.ts")
        || path.ends_with(".test.mjs")
        || path.ends_with(".d.ts")
    {
        return false;
    }
    path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".md")
        || path.ends_with(".json")
        || path.ends_with(".sql")
}

fn normalize_rel_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(String::new());
    }
    for seg in trimmed.split('/') {
        if seg == ".." {
            return Err(anyhow!("invalid path traversal segment `..`"));
        }
    }
    Ok(trimmed.to_string())
}

pub(crate) fn is_secret_file_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if base == ".env.example" || base == ".env.sample" || base == ".env.template" {
        return false;
    }
    base == ".env"
        || base.starts_with(".env.")
        || base.ends_with(".pem")
        || base.ends_with(".key")
        || base.ends_with(".p12")
        || base.ends_with(".pfx")
        || base == "id_rsa"
        || base == "id_ed25519"
}

pub(crate) fn should_skip_grep_path(path: &str) -> bool {
    path.contains("pnpm-lock.yaml")
        || path.contains("package-lock.json")
        || path.contains("Cargo.lock")
        || path.starts_with("outputs/")
}

fn path_to_high_level_domain(rel_path: &str) -> String {
    let segs: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();
    match segs.as_slice() {
        ["apps", app, "src", module, ..] => format!("{app}/{module}"),
        ["apps", app, ..] => (*app).to_string(),
        ["packages", pkg, ..] => format!("packages/{pkg}"),
        ["docs", sub, ..] => format!("docs/{sub}"),
        [top, ..] => (*top).to_string(),
        [] => "root".to_string(),
    }
}

pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return text.starts_with(prefix);
    }
    pattern == text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_patterns_work() {
        assert!(wildcard_match("*", "dev/v1"));
        assert!(wildcard_match("dev/*", "dev/v1"));
        assert!(!wildcard_match("dev/*", "main"));
        assert!(wildcard_match("main", "main"));
    }
}
