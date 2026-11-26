use crate::{is_secret_file_path, should_skip_grep_path};
use anyhow::{Context, Result, anyhow};
use dashmap::DashMap;
use distill_crypto::DlpGate;
use distill_proto::{GrepMatch, MemorySnapshotMetric, TreeEntry};
use gix::bstr::ByteSlice;
use lru::LruCache;
use memchr::memmem::Finder;
use rayon::prelude::*;
use regex::RegexBuilder;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;
use tokio::sync::OnceCell;
use tracing::info;

/// Max per-file size hydrated into the in-memory raw snapshot (1 MiB).
const MAX_SNAPSHOT_BLOB_BYTES: usize = 1024 * 1024;
/// Max matches emitted from a single file (aligned with `git grep --max-count 20`).
const MAX_MATCHES_PER_FILE: usize = 20;
/// Number of leading candidate files scanned sequentially before fanning out to Rayon.
/// Satisfies dense queries (`limit = 10..40`) in exact deterministic order with ZERO thread wake-up overhead.
const INLINE_HEAD_PROBE: usize = 24;

static EPOCH_ORIGIN: OnceLock<Instant> = OnceLock::new();

#[inline]
fn now_millis_since_origin() -> u64 {
    EPOCH_ORIGIN
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis() as u64
}

static REGEX_CACHE: OnceLock<Mutex<LruCache<(String, bool), Arc<regex::Regex>>>> = OnceLock::new();

fn compile_cached_regex(pattern: &str, case_insensitive: bool) -> Result<Arc<regex::Regex>> {
    let cache = REGEX_CACHE.get_or_init(|| {
        Mutex::new(LruCache::new(NonZeroUsize::new(256).unwrap()))
    });
    let key = (pattern.to_string(), case_insensitive);
    if let Ok(mut guard) = cache.lock()
        && let Some(re) = guard.get(&key)
    {
        return Ok(Arc::clone(re));
    }
    // For pure-ASCII regex patterns (e.g. `\s+`, `\w+`, `[A-Z]`), ASCII DFA mode is 3-5x faster
    // and matches `git grep -E` semantics; non-ASCII/UTF-8 patterns keep full Unicode mode.
    let use_unicode = !pattern.is_ascii();
    let built = RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .multi_line(true)
        .unicode(use_unicode)
        .build()
        .or_else(|_| {
            RegexBuilder::new(&regex::escape(pattern))
                .case_insensitive(case_insensitive)
                .unicode(use_unicode)
                .build()
        })
        .context("failed to compile grep pattern")?;
    let arc = Arc::new(built);
    if let Ok(mut guard) = cache.lock() {
        guard.put(key, Arc::clone(&arc));
    }
    Ok(arc)
}

/// Deduplicated, pre-sanitized, line-indexed text blob shared across branches/commits via `Arc`.
pub struct RawBlob {
    /// Already secret-redacted UTF-8 text (zero per-query regex DLP overhead).
    pub content: Arc<str>,
    /// Number of secret redactions applied when this blob was loaded.
    pub dlp_redactions: usize,
    /// Byte offset of each line's start (0-based line index -> byte offset in `content`).
    line_starts: Box<[u32]>,
}

impl RawBlob {
    pub fn from_raw_bytes(raw: &[u8]) -> Option<Self> {
        if raw.len() > MAX_SNAPSHOT_BLOB_BYTES || raw.contains(&0u8) {
            return None;
        }
        let text = String::from_utf8_lossy(raw);
        let (sanitized, dlp_redactions) = DlpGate::sanitize_secrets(&text);
        Some(Self::from_sanitized(sanitized, dlp_redactions))
    }

    pub fn from_sanitized(sanitized: String, dlp_redactions: usize) -> Self {
        let mut starts: Vec<u32> = Vec::with_capacity((sanitized.len() / 48).max(8));
        if !sanitized.is_empty() {
            starts.push(0);
            let bytes = sanitized.as_bytes();
            for pos in memchr::memchr_iter(b'\n', bytes) {
                if pos + 1 < bytes.len() {
                    starts.push((pos + 1) as u32);
                }
            }
        }
        Self {
            content: Arc::from(sanitized),
            dlp_redactions,
            line_starts: starts.into_boxed_slice(),
        }
    }

    pub fn total_lines(&self) -> usize {
        self.line_starts.len()
    }

    pub fn estimated_bytes(&self) -> u64 {
        (self.content.len() + self.line_starts.len() * std::mem::size_of::<u32>() + 48) as u64
    }

    /// O(1) line access (0-based), stripping trailing `\n` / `\r\n`.
    pub fn line_str(&self, idx: usize) -> &str {
        if idx >= self.line_starts.len() {
            return "";
        }
        let start = self.line_starts[idx] as usize;
        let end = if idx + 1 < self.line_starts.len() {
            self.line_starts[idx + 1] as usize
        } else {
            self.content.len()
        };
        self.content[start..end]
            .trim_end_matches('\n')
            .trim_end_matches('\r')
    }

    /// O(log L) mapping from byte offset inside `content` to 0-based line index.
    pub fn line_index_at_byte(&self, byte_pos: usize) -> usize {
        if self.line_starts.is_empty() {
            return 0;
        }
        self.line_starts
            .partition_point(|&s| (s as usize) <= byte_pos)
            .saturating_sub(1)
    }

    /// Format numbered lines `[start_line..=end_line]` (1-based) cleanly within 64 KiB cap.
    pub fn format_line_range(
        &self,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> (usize, usize, usize, String, bool, usize) {
        let total = self.total_lines();
        if total == 0 {
            return (1, 0, 0, String::new(), false, self.dlp_redactions);
        }
        let s = start_line.unwrap_or(1).clamp(1, total);
        let requested_end = end_line.unwrap_or(s + 299).clamp(s, total);
        let capped_end = requested_end.min(s + 599).min(total);
        let mut out = String::with_capacity(((capped_end - s + 1) * 64).min(64 * 1024));
        let mut truncated = capped_end < total && end_line.is_none();
        let mut final_end = s.saturating_sub(1);
        for ln in s..=capped_end {
            let line = self.line_str(ln - 1);
            // Predict formatted length (`5 digits + " | " + line + "\n"`) to avoid extra allocation
            let est_len = 9 + line.len();
            if out.len() + est_len > 64 * 1024 {
                truncated = true;
                break;
            }
            let _ = writeln!(out, "{ln:>5} | {line}");
            final_end = ln;
        }
        (s, final_end, total, out, truncated, self.dlp_redactions)
    }
}

/// Compact contiguous CSR posting store (`u16` for repos <= 65,535 files, `u32` for larger repos).
enum PostingStore {
    U16(Box<[u16]>),
    U32(Box<[u32]>),
}

impl PostingStore {
    fn slice_len(&self, _offset: u32, len: u32) -> usize {
        len as usize
    }

    fn contains(&self, offset: u32, len: u32, file_idx: u32) -> bool {
        let start = offset as usize;
        let end = start + len as usize;
        match self {
            PostingStore::U16(data) => {
                if file_idx > u16::MAX as u32 {
                    return false;
                }
                data[start..end].binary_search(&(file_idx as u16)).is_ok()
            }
            PostingStore::U32(data) => data[start..end].binary_search(&file_idx).is_ok(),
        }
    }

    fn copy_into(&self, offset: u32, len: u32, lo: u32, hi: u32, out: &mut Vec<u32>) {
        let start = offset as usize;
        let end = start + len as usize;
        match self {
            PostingStore::U16(data) => {
                let slice = &data[start..end];
                let s = if lo == 0 {
                    0
                } else {
                    slice.partition_point(|&x| (x as u32) < lo)
                };
                for &x in &slice[s..] {
                    let v = x as u32;
                    if v >= hi {
                        break;
                    }
                    out.push(v);
                }
            }
            PostingStore::U32(data) => {
                let slice = &data[start..end];
                let s = if lo == 0 {
                    0
                } else {
                    slice.partition_point(|&x| x < lo)
                };
                for &v in &slice[s..] {
                    if v >= hi {
                        break;
                    }
                    out.push(v);
                }
            }
        }
    }

    fn intersect_sorted(&self, offset: u32, len: u32, acc: &mut Vec<u32>) {
        if acc.is_empty() {
            return;
        }
        let next_len = len as usize;
        if acc.len().saturating_mul(8) < next_len {
            // Galloping binary search when current candidate set is much smaller than posting list
            acc.retain(|&id| self.contains(offset, len, id));
            return;
        }
        let start = offset as usize;
        let end = start + next_len;
        let mut i = 0usize;
        let mut j = 0usize;
        let mut write = 0usize;
        match self {
            PostingStore::U16(data) => {
                let slice = &data[start..end];
                while i < acc.len() && j < slice.len() {
                    let a = acc[i];
                    let b = slice[j] as u32;
                    if a == b {
                        acc[write] = a;
                        write += 1;
                        i += 1;
                        j += 1;
                    } else if a < b {
                        i += 1;
                    } else {
                        j += 1;
                    }
                }
            }
            PostingStore::U32(data) => {
                let slice = &data[start..end];
                while i < acc.len() && j < slice.len() {
                    let a = acc[i];
                    let b = slice[j];
                    if a == b {
                        acc[write] = a;
                        write += 1;
                        i += 1;
                        j += 1;
                    } else if a < b {
                        i += 1;
                    } else {
                        j += 1;
                    }
                }
            }
        }
        acc.truncate(write);
    }

    fn bytes_len(&self) -> usize {
        match self {
            PostingStore::U16(v) => v.len() * std::mem::size_of::<u16>(),
            PostingStore::U32(v) => v.len() * std::mem::size_of::<u32>(),
        }
    }
}

/// Compact CSR-packed 3-byte trigram inverted index over all searchable files in a commit snapshot.
/// Keys are ASCII-case-folded 3-byte sequences (`(b0 << 16) | (b1 << 8) | b2`), preserving UTF-8/CJK bytes.
pub struct TrigramIndex {
    /// trigram -> (offset, len) into `postings`
    table: HashMap<u32, (u32, u32)>,
    postings: PostingStore,
    pub estimated_bytes: u64,
}

enum TrigramPlan {
    AllFiles,
    UnionOfAnds(Vec<Vec<u32>>),
}

#[inline]
fn pack_trigram(b0: u8, b1: u8, b2: u8) -> u32 {
    ((b0.to_ascii_lowercase() as u32) << 16)
        | ((b1.to_ascii_lowercase() as u32) << 8)
        | (b2.to_ascii_lowercase() as u32)
}

fn extract_unique_trigrams_bytes(bytes: &[u8]) -> Vec<u32> {
    if bytes.len() < 3 {
        return Vec::new();
    }
    let mut tris: Vec<u32> = Vec::with_capacity(bytes.len().min(8192));
    for w in bytes.windows(3) {
        tris.push(pack_trigram(w[0], w[1], w[2]));
    }
    tris.sort_unstable();
    tris.dedup();
    tris
}

impl TrigramIndex {
    pub fn build(pool: &rayon::ThreadPool, files: &[(Arc<str>, Arc<RawBlob>)]) -> Self {
        if files.is_empty() {
            return Self {
                table: HashMap::new(),
                postings: PostingStore::U16(Box::new([])),
                estimated_bytes: 0,
            };
        }

        // Step 1: Extract deduplicated trigrams per file in parallel on dedicated Rayon pool
        let per_file: Vec<Vec<u32>> = pool.install(|| {
            files
                .par_iter()
                .map(|(_, blob)| extract_unique_trigrams_bytes(blob.content.as_bytes()))
                .collect()
        });

        // Step 2: Count frequencies per trigram to allocate CSR flat array in a single pass
        let mut counts: HashMap<u32, u32> = HashMap::with_capacity(65_536);
        let mut total_postings: usize = 0;
        for tris in &per_file {
            total_postings += tris.len();
            for &tri in tris {
                *counts.entry(tri).or_insert(0) += 1;
            }
        }

        // Step 3: Compute prefix offsets into CSR table
        let mut table: HashMap<u32, (u32, u32)> = HashMap::with_capacity(counts.len());
        let mut cursors: HashMap<u32, u32> = HashMap::with_capacity(counts.len());
        let mut offset: u32 = 0;
        for (tri, count) in counts {
            table.insert(tri, (offset, count));
            cursors.insert(tri, offset);
            offset = offset.saturating_add(count);
        }

        // Step 4: Fill flat postings in ascending file_idx order (each posting list is auto-sorted!)
        let postings = if files.len() <= u16::MAX as usize {
            let mut flat = vec![0u16; total_postings];
            for (file_idx, tris) in per_file.iter().enumerate() {
                let fid = file_idx as u16;
                for &tri in tris {
                    if let Some(pos) = cursors.get_mut(&tri) {
                        flat[*pos as usize] = fid;
                        *pos += 1;
                    }
                }
            }
            PostingStore::U16(flat.into_boxed_slice())
        } else {
            let mut flat = vec![0u32; total_postings];
            for (file_idx, tris) in per_file.iter().enumerate() {
                let fid = file_idx as u32;
                for &tri in tris {
                    if let Some(pos) = cursors.get_mut(&tri) {
                        flat[*pos as usize] = fid;
                        *pos += 1;
                    }
                }
            }
            PostingStore::U32(flat.into_boxed_slice())
        };

        let table_bytes = table.capacity() * (std::mem::size_of::<u32>() + std::mem::size_of::<(u32, u32)>() + 1);
        let estimated_bytes = (table_bytes + postings.bytes_len()) as u64;

        Self {
            table,
            postings,
            estimated_bytes,
        }
    }

    /// Return pruned sorted candidate file indices within `[lo..hi)`, or `None` if a full scan is required.
    fn query(&self, plan: &TrigramPlan, lo: usize, hi: usize) -> Option<Vec<u32>> {
        let TrigramPlan::UnionOfAnds(branches) = plan else {
            return None;
        };
        if lo >= hi {
            return Some(Vec::new());
        }
        let lo_u32 = lo as u32;
        let hi_u32 = hi as u32;

        let mut union_out: Vec<u32> = Vec::new();
        for branch_tris in branches {
            if branch_tris.is_empty() {
                return None;
            }
            let mut lists: Vec<(u32, u32)> = Vec::with_capacity(branch_tris.len());
            let mut impossible = false;
            for &tri in branch_tris {
                match self.table.get(&tri) {
                    Some(&(off, len)) if len > 0 => lists.push((off, len)),
                    _ => {
                        impossible = true;
                        break;
                    }
                }
            }
            if impossible {
                continue;
            }
            // Intersect rarest posting lists first (up to 8 rarest is more than enough)
            lists.sort_unstable_by_key(|&(_, len)| len);
            lists.truncate(8);

            let (first_off, first_len) = lists[0];
            let mut acc = Vec::with_capacity(self.postings.slice_len(first_off, first_len).min(hi - lo));
            self.postings
                .copy_into(first_off, first_len, lo_u32, hi_u32, &mut acc);
            for &(off, len) in &lists[1..] {
                if acc.is_empty() {
                    break;
                }
                self.postings.intersect_sorted(off, len, &mut acc);
            }
            if branches.len() == 1 {
                return Some(acc);
            }
            union_out.extend(acc);
        }

        if branches.len() > 1 {
            union_out.sort_unstable();
            union_out.dedup();
        }
        Some(union_out)
    }
}

/// Sound conservative trigram query planner for literals and regexes (guaranteed zero false negatives).
fn build_trigram_plan(pattern: &str, is_plain_literal: bool, case_insensitive: bool) -> TrigramPlan {
    if is_plain_literal {
        let tris = extract_literal_run_trigrams(pattern, case_insensitive);
        if tris.is_empty() {
            return TrigramPlan::AllFiles;
        }
        return TrigramPlan::UnionOfAnds(vec![tris]);
    }

    // Split top-level alternation `a|b` (at paren_depth == 0, outside `[...]` and escapes)
    let branches = split_top_level_alternation(pattern);
    let mut plan_branches = Vec::with_capacity(branches.len());
    for branch in branches {
        let tris = extract_required_regex_trigrams(branch, case_insensitive);
        if tris.is_empty() {
            // If ANY top-level alternation branch has no guaranteed 3-byte literal, we must scan all files.
            return TrigramPlan::AllFiles;
        }
        plan_branches.push(tris);
    }
    if plan_branches.is_empty() {
        TrigramPlan::AllFiles
    } else {
        TrigramPlan::UnionOfAnds(plan_branches)
    }
}

fn extract_literal_run_trigrams(s: &str, case_insensitive: bool) -> Vec<u32> {
    if !case_insensitive {
        return extract_unique_trigrams_bytes(s.as_bytes());
    }
    // For case-insensitive mode, skip non-ASCII runs to avoid any Unicode case-folding edge mismatch.
    let mut tris = Vec::new();
    let mut run = Vec::new();
    for ch in s.chars() {
        if ch.is_ascii() {
            run.push(ch as u8);
        } else {
            if run.len() >= 3 {
                tris.extend(extract_unique_trigrams_bytes(&run));
            }
            run.clear();
        }
    }
    if run.len() >= 3 {
        tris.extend(extract_unique_trigrams_bytes(&run));
    }
    tris.sort_unstable();
    tris.dedup();
    tris
}

fn split_top_level_alternation(pattern: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0usize;
    let mut in_class = false;
    let mut escaped = false;

    for (idx, ch) in pattern.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            continue;
        }
        match ch {
            '[' => in_class = true,
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '|' if paren_depth == 0 => {
                out.push(&pattern[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    out.push(&pattern[start..]);
    out
}

/// Extract trigrams from literal runs guaranteed to appear in every match of a single regex branch.
fn extract_required_regex_trigrams(branch: &str, case_insensitive: bool) -> Vec<u32> {
    let mut all_tris: Vec<u32> = Vec::new();
    let mut run = String::new();
    let mut paren_depth = 0usize;
    let mut in_class = false;
    let chars: Vec<char> = branch.chars().collect();
    let mut i = 0usize;

    let flush_run = |run: &mut String, out: &mut Vec<u32>| {
        if run.len() >= 3 {
            out.extend(extract_literal_run_trigrams(run, case_insensitive));
        }
        run.clear();
    };

    while i < chars.len() {
        let ch = chars[i];
        if in_class {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == ']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if paren_depth > 0 {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == '[' {
                in_class = true;
            } else if ch == '(' {
                paren_depth += 1;
            } else if ch == ')' {
                paren_depth -= 1;
            }
            i += 1;
            continue;
        }

        match ch {
            '\\' => {
                if i + 1 < chars.len() {
                    let next = chars[i + 1];
                    // Safe escaped punctuation literals (e.g. `\.`, `\_`, `\/`, `\-`, `\:`)
                    if matches!(
                        next,
                        '.' | '_' | '/' | '-' | ':' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '*' | '?' | '^' | '$' | '|' | '\\'
                    ) {
                        run.push(next);
                        i += 2;
                        continue;
                    }
                }
                flush_run(&mut run, &mut all_tris);
                i += 2;
            }
            '(' => {
                flush_run(&mut run, &mut all_tris);
                paren_depth += 1;
                i += 1;
            }
            '[' => {
                flush_run(&mut run, &mut all_tris);
                in_class = true;
                i += 1;
            }
            '?' | '*' => {
                // Preceding character is optional (0 or more times) -> pop it before flushing!
                run.pop();
                flush_run(&mut run, &mut all_tris);
                i += 1;
            }
            '{' => {
                // `{0...}` makes preceding char optional; `{1...}` keeps it required
                let mut j = i + 1;
                let mut num = String::new();
                while j < chars.len() && chars[j].is_ascii_digit() {
                    num.push(chars[j]);
                    j += 1;
                }
                let min_rep: usize = num.parse().unwrap_or(0);
                if min_rep == 0 {
                    run.pop();
                }
                flush_run(&mut run, &mut all_tris);
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                i = (j + 1).min(chars.len());
            }
            '+' | '.' | '^' | '$' | ')' | ']' | '}' => {
                flush_run(&mut run, &mut all_tris);
                i += 1;
            }
            _ => {
                run.push(ch);
                i += 1;
            }
        }
    }
    flush_run(&mut run, &mut all_tris);
    all_tris.sort_unstable();
    all_tris.dedup();
    all_tris
}

/// Immutable in-memory structured view of a `(repo, commit_sha)` snapshot.
pub struct CommitRawSnapshot {
    pub repo: String,
    pub commit_sha: String,
    /// Full sorted tree (dirs + files, excluding `.git`/`node_modules`/`target`/etc.).
    pub tree: Vec<TreeEntry>,
    /// Contiguous sorted slice of searchable, non-secret text files for `rayon` parallel scans.
    pub files: Vec<(Arc<str>, Arc<RawBlob>)>,
    /// O(1) lookup from relative path -> index in `files`.
    pub path_to_file: HashMap<String, usize>,
    /// Compact CSR-packed trigram inverted index for sub-millisecond candidate file pruning.
    pub trigram_index: TrigramIndex,
    /// Lock-free last-used timestamp (milliseconds since `EPOCH_ORIGIN`) for zero-overhead hot hits.
    last_used_ms: AtomicU64,
}

impl CommitRawSnapshot {
    #[inline]
    pub fn touch_fast(&self) {
        self.last_used_ms
            .store(now_millis_since_origin(), Ordering::Relaxed);
    }

    #[inline]
    pub fn idle_secs(&self) -> u64 {
        let last = self.last_used_ms.load(Ordering::Relaxed);
        now_millis_since_origin().saturating_sub(last) / 1000
    }
    /// Pure in-memory `list_tree` using binary search on sorted `tree` (microsecond latency).
    pub fn list_tree(
        &self,
        norm_sub: &str,
        max_depth: usize,
        limit: usize,
    ) -> Result<(Vec<TreeEntry>, bool)> {
        let depth_cap = max_depth.clamp(1, 8);
        let entry_cap = limit.clamp(1, 1000);

        if !norm_sub.is_empty() {
            // O(log N) check if norm_sub is an exact file match
            let pos = self.tree.partition_point(|e| e.path.as_str() < norm_sub);
            if let Some(exact) = self.tree.get(pos)
                && !exact.is_dir
                && exact.path == norm_sub
            {
                return Ok((vec![exact.clone()], false));
            }
        }

        let dir_prefix = if norm_sub.is_empty() {
            String::new()
        } else {
            format!("{}/", norm_sub.trim_end_matches('/'))
        };

        let start_idx = if dir_prefix.is_empty() {
            0
        } else {
            self.tree
                .partition_point(|e| e.path.as_str() < dir_prefix.as_str())
        };

        let mut out = Vec::with_capacity(entry_cap.min(256));
        let mut truncated = false;
        let mut matched_any = norm_sub.is_empty();

        for entry in &self.tree[start_idx..] {
            let rel = if dir_prefix.is_empty() {
                entry.path.as_str()
            } else {
                if !entry.path.starts_with(&dir_prefix) {
                    break;
                }
                matched_any = true;
                if entry.is_dir && entry.path == dir_prefix {
                    continue;
                }
                &entry.path[dir_prefix.len()..]
            };

            let trimmed = rel.trim_end_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            let depth = trimmed.bytes().filter(|&b| b == b'/').count() + 1;
            if depth > depth_cap {
                continue;
            }
            if out.len() >= entry_cap {
                truncated = true;
                break;
            }
            out.push(entry.clone());
        }

        if !matched_any {
            return Err(anyhow!("path not found in branch: {norm_sub}"));
        }

        Ok((out, truncated))
    }

    /// O(1) pure in-memory file read (already secret-redacted).
    pub fn read_lines(
        &self,
        norm_path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Option<(usize, usize, usize, String, bool, usize)> {
        let &idx = self.path_to_file.get(norm_path)?;
        let blob = &self.files[idx].1;
        Some(blob.format_line_range(start_line, end_line))
    }

    /// O(log N) contiguous `[lo..hi)` index range in sorted `self.files` matching `norm_prefix`.
    fn prefix_file_range(&self, norm_prefix: &str) -> (usize, usize) {
        if norm_prefix.is_empty() {
            return (0, self.files.len());
        }
        // Check exact file match first via O(1) HashMap
        if let Some(&idx) = self.path_to_file.get(norm_prefix) {
            return (idx, idx + 1);
        }
        let dir_prefix = format!("{}/", norm_prefix.trim_end_matches('/'));
        let lo = self
            .files
            .partition_point(|(p, _)| p.as_ref() < dir_prefix.as_str());
        let mut hi = lo;
        while hi < self.files.len() && self.files[hi].0.starts_with(&dir_prefix) {
            hi += 1;
        }
        (lo, hi)
    }

    /// Ultra-fast trigram-pruned + SIMD/regex grep.
    /// Runs inline with zero thread wake-up overhead when candidates <= 32, and fans out on `rayon` pool otherwise.
    /// Returns `(matches, truncated, dlp_redactions_referenced)`.
    pub fn grep(
        &self,
        pool: &rayon::ThreadPool,
        pattern: &str,
        norm_prefix: &str,
        case_insensitive: bool,
        ctx: usize,
        limit: usize,
    ) -> Result<(Vec<GrepMatch>, bool, usize)> {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("grep pattern must not be empty"));
        }
        let cap = limit.clamp(1, 150);
        let ctx_lines = ctx.min(5);
        let early_stop_budget = cap.saturating_mul(2).max(cap + 16);

        let (lo, hi) = self.prefix_file_range(norm_prefix);
        if lo >= hi {
            return Ok((Vec::new(), false, 0));
        }

        let is_plain_literal = !trimmed.chars().any(|c| {
            matches!(
                c,
                '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '\\'
            )
        });

        // Step 1: Trigram candidate pruning (drops 4,076 files -> 0..30 files in ~10 microseconds!)
        let plan = build_trigram_plan(trimmed, is_plain_literal, case_insensitive);
        let pruned_candidates = self.trigram_index.query(&plan, lo, hi);
        if let Some(ref cands) = pruned_candidates
            && cands.is_empty()
        {
            return Ok((Vec::new(), false, 0));
        }

        // Step 2: Build or fetch cached matcher ONCE (zero per-call regex recompile!)
        enum Matcher {
            SimdLiteral(Finder<'static>),
            Regex(Arc<regex::Regex>),
        }

        let matcher = if is_plain_literal && !case_insensitive {
            Matcher::SimdLiteral(Finder::new(trimmed.as_bytes()).into_owned())
        } else {
            Matcher::Regex(compile_cached_regex(trimmed, case_insensitive)?)
        };

        let scan_one = |path: &Arc<str>, blob: &Arc<RawBlob>| -> Option<(Vec<GrepMatch>, usize)> {
            let mut line_indices: Vec<usize> = Vec::new();
            match &matcher {
                Matcher::SimdLiteral(finder) => {
                    let hay = blob.content.as_bytes();
                    let mut last_line: Option<usize> = None;
                    for pos in finder.find_iter(hay) {
                        let li = blob.line_index_at_byte(pos);
                        if last_line == Some(li) {
                            continue;
                        }
                        last_line = Some(li);
                        line_indices.push(li);
                        if line_indices.len() >= MAX_MATCHES_PER_FILE {
                            break;
                        }
                    }
                }
                Matcher::Regex(re) => {
                    let mut last_line: Option<usize> = None;
                    for m in re.find_iter(&blob.content) {
                        let li = blob.line_index_at_byte(m.start());
                        if last_line == Some(li) {
                            continue;
                        }
                        last_line = Some(li);
                        line_indices.push(li);
                        if line_indices.len() >= MAX_MATCHES_PER_FILE {
                            break;
                        }
                    }
                }
            }

            if line_indices.is_empty() {
                return None;
            }

            let total_lines = blob.total_lines();
            let mut matches = Vec::with_capacity(line_indices.len());
            for li in line_indices {
                let before = if ctx_lines > 0 {
                    let b_start = li.saturating_sub(ctx_lines);
                    (b_start..li)
                        .map(|i| truncate_line(blob.line_str(i), 400))
                        .collect()
                } else {
                    Vec::new()
                };
                let after = if ctx_lines > 0 {
                    let a_end = (li + 1 + ctx_lines).min(total_lines);
                    ((li + 1)..a_end)
                        .map(|i| truncate_line(blob.line_str(i), 400))
                        .collect()
                } else {
                    Vec::new()
                };
                matches.push(GrepMatch {
                    path: path.to_string(),
                    line_number: li + 1,
                    line: truncate_line(blob.line_str(li), 400),
                    before,
                    after,
                });
            }
            Some((matches, blob.dlp_redactions))
        };

        let files = &self.files;
        let mut all_matches: Vec<GrepMatch> = Vec::with_capacity(cap.min(64));
        let mut total_dlp = 0usize;
        let mut hit_early_stop = false;

        match pruned_candidates {
            Some(cands) => {
                // Step 3a: Adaptive sequential head probe in exact sorted path order.
                // - Dense matches hit `cap` within the first few files (ZERO Rayon wake-up or sort).
                // - Sparse matches with 0 hits after 8 files bail out immediately to 12-core Rayon!
                let head_cap = cands.len().min(INLINE_HEAD_PROBE);
                let mut probed = 0usize;
                for &fid in &cands[..head_cap] {
                    probed += 1;
                    let (path, blob) = &files[fid as usize];
                    if let Some((mut fm, dlp)) = scan_one(path, blob) {
                        total_dlp += dlp;
                        all_matches.append(&mut fm);
                        if all_matches.len() > cap {
                            hit_early_stop = true;
                            break;
                        }
                    } else if probed == 8 && all_matches.is_empty() && cands.len() > INLINE_HEAD_PROBE {
                        break;
                    }
                }

                // Step 3b: Fan out remaining candidates across Rayon if head probe didn't satisfy `cap`!
                if !hit_early_stop && probed < cands.len() {
                    let tail = &cands[probed..];
                    let found_total = AtomicUsize::new(all_matches.len());
                    let mut per_file_hits: Vec<(Vec<GrepMatch>, usize)> = pool.install(|| {
                        tail.par_iter()
                            .filter_map(|&fid| {
                                if found_total.load(Ordering::Relaxed) >= early_stop_budget {
                                    return None;
                                }
                                let (path, blob) = &files[fid as usize];
                                let (matches, dlp) = scan_one(path, blob)?;
                                found_total.fetch_add(matches.len(), Ordering::Relaxed);
                                Some((matches, dlp))
                            })
                            .collect()
                    });
                    for (mut fm, dlp) in per_file_hits.drain(..) {
                        all_matches.append(&mut fm);
                        total_dlp += dlp;
                    }
                    all_matches.sort_by(|a, b| {
                        a.path
                            .cmp(&b.path)
                            .then_with(|| a.line_number.cmp(&b.line_number))
                    });
                    hit_early_stop = found_total.load(Ordering::Relaxed) >= early_stop_budget;
                }
            }
            None => {
                let slice = &files[lo..hi];
                let head_end = slice.len().min(INLINE_HEAD_PROBE);
                for (path, blob) in &slice[..head_end] {
                    if let Some((mut fm, dlp)) = scan_one(path, blob) {
                        total_dlp += dlp;
                        all_matches.append(&mut fm);
                        if all_matches.len() > cap {
                            hit_early_stop = true;
                            break;
                        }
                    }
                }

                if !hit_early_stop && head_end < slice.len() {
                    let tail = &slice[head_end..];
                    let found_total = AtomicUsize::new(all_matches.len());
                    let mut per_file_hits: Vec<(Vec<GrepMatch>, usize)> = pool.install(|| {
                        tail.par_iter()
                            .filter_map(|(path, blob)| {
                                if found_total.load(Ordering::Relaxed) >= early_stop_budget {
                                    return None;
                                }
                                let (matches, dlp) = scan_one(path, blob)?;
                                found_total.fetch_add(matches.len(), Ordering::Relaxed);
                                Some((matches, dlp))
                            })
                            .collect()
                    });
                    for (mut fm, dlp) in per_file_hits.drain(..) {
                        all_matches.append(&mut fm);
                        total_dlp += dlp;
                    }
                    all_matches.sort_by(|a, b| {
                        a.path
                            .cmp(&b.path)
                            .then_with(|| a.line_number.cmp(&b.line_number))
                    });
                    hit_early_stop = found_total.load(Ordering::Relaxed) >= early_stop_budget;
                }
            }
        }

        let truncated = all_matches.len() > cap || hit_early_stop;
        all_matches.truncate(cap);
        Ok((all_matches, truncated, total_dlp))
    }
}

fn truncate_line(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s.to_string();
    }
    s.chars().take(max_chars).collect()
}

pub struct SnapshotPoolMetrics {
    pub active_snapshots: usize,
    pub max_snapshots: usize,
    pub cached_blobs: usize,
    pub memory_bytes: u64,
    pub max_memory_bytes: u64,
    pub rayon_threads: usize,
    pub snapshots: Vec<MemorySnapshotMetric>,
}

/// Thread-safe, deduplicated in-memory raw snapshot cache backed by a dedicated `rayon` pool.
pub struct RawSnapshotCache {
    /// Global deduplicated blob pool keyed by Git `blob_oid` hex string.
    blob_pool: DashMap<String, Arc<RawBlob>>,
    /// Zero-allocation lookup nested singleflight cache: `repo -> (commit_sha -> OnceCell<Arc<CommitRawSnapshot>>)`.
    snapshots: DashMap<String, DashMap<String, Arc<OnceCell<Arc<CommitRawSnapshot>>>>>,
    /// Last seen branch label per `(repo, commit_sha)` (updated only on snapshot hydration, never on hot hits).
    branch_labels: DashMap<(String, String), String>,
    /// Dedicated CPU worker pool isolated from Tokio async I/O threads.
    rayon_pool: Arc<rayon::ThreadPool>,
    rayon_threads: usize,
    max_snapshots: AtomicUsize,
    max_memory_bytes: AtomicU64,
}

impl RawSnapshotCache {
    pub fn new(max_snapshots: usize, max_memory_mb: u64, requested_threads: usize) -> Self {
        let threads = if requested_threads > 0 {
            requested_threads.clamp(1, 32)
        } else {
            let cpus = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            (cpus / 2).clamp(2, 12)
        };
        let rayon_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("distill-rayon-{i}"))
            .build()
            .expect("failed to build dedicated rayon thread pool");

        Self {
            blob_pool: DashMap::new(),
            snapshots: DashMap::new(),
            branch_labels: DashMap::new(),
            rayon_pool: Arc::new(rayon_pool),
            rayon_threads: threads,
            max_snapshots: AtomicUsize::new(max_snapshots.max(1)),
            max_memory_bytes: AtomicU64::new(max_memory_mb.max(32) * 1024 * 1024),
        }
    }

    pub fn rayon_pool(&self) -> &Arc<rayon::ThreadPool> {
        &self.rayon_pool
    }

    pub fn set_limits(&self, max_snapshots: usize, max_memory_mb: u64) {
        self.max_snapshots
            .store(max_snapshots.max(1), Ordering::Relaxed);
        self.max_memory_bytes
            .store(max_memory_mb.max(32) * 1024 * 1024, Ordering::Relaxed);
    }

    fn total_snapshot_count(&self) -> usize {
        self.snapshots.iter().map(|r| r.value().len()).sum()
    }

    /// Return cached `CommitRawSnapshot` with ZERO heap allocations & ZERO write locks on hot hits,
    /// or hydrate it once (singleflight) with `rayon` parallel preprocessing.
    pub async fn get_or_build(
        &self,
        repo: &str,
        branch: &str,
        commit_sha: &str,
        repo_path: &Path,
    ) -> Result<Arc<CommitRawSnapshot>> {
        // Ultra-fast hot-hit path: borrowed `&str` lookup + 1ns lock-free AtomicU64 touch!
        if let Some(repo_map) = self.snapshots.get(repo)
            && let Some(cell_ref) = repo_map.get(commit_sha)
            && let Some(snap) = cell_ref.value().get()
        {
            snap.touch_fast();
            return Ok(Arc::clone(snap));
        }

        let cell = {
            let repo_map = self
                .snapshots
                .entry(repo.to_string())
                .or_default();
            repo_map
                .entry(commit_sha.to_string())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };

        let mut just_built = false;
        let snap = cell
            .get_or_try_init(|| async {
                let built = self
                    .hydrate_snapshot(repo, commit_sha, repo_path)
                    .await?;
                just_built = true;
                Ok::<Arc<CommitRawSnapshot>, anyhow::Error>(Arc::new(built))
            })
            .await?;

        snap.touch_fast();
        if !branch.is_empty() {
            self.branch_labels
                .insert((repo.to_string(), commit_sha.to_string()), branch.to_string());
        }

        // Only enforce soft-cap when a new snapshot was actually hydrated
        if just_built {
            let max_snaps = self.max_snapshots.load(Ordering::Relaxed);
            let max_mem = self.max_memory_bytes.load(Ordering::Relaxed);
            if self.total_snapshot_count() > max_snaps || self.estimated_total_memory_bytes() > max_mem {
                self.gc(max_snaps, max_mem / (1024 * 1024), 0);
            }
        }

        Ok(Arc::clone(snap))
    }

    async fn hydrate_snapshot(
        &self,
        repo: &str,
        commit_sha: &str,
        repo_path: &Path,
    ) -> Result<CommitRawSnapshot> {
        let started = Instant::now();
        let repo_name = repo.to_string();
        let sha = commit_sha.to_string();
        let rpath = repo_path.to_path_buf();

        // Step 1: Read git tree + only uncached raw blob bytes sequentially from packfile
        let (tree, items) = tokio::task::spawn_blocking({
            let repo_path = rpath;
            let sha_clone = sha.clone();
            // Collect existing OIDs snapshot to avoid locking during git walk
            let existing_oids: std::collections::HashSet<String> = self
                .blob_pool
                .iter()
                .map(|e| e.key().clone())
                .collect();
            move || -> Result<(Vec<TreeEntry>, Vec<WalkRawFile>)> {
                let git_repo = gix::open(&repo_path)?;
                let id = gix::ObjectId::from_hex(sha_clone.as_bytes())?;
                let commit = git_repo.find_object(id)?.try_into_commit()?;
                let root_tree = commit.tree()?;

                let mut tree_entries = Vec::new();
                let mut file_items = Vec::new();
                walk_snapshot_tree(
                    &git_repo,
                    &root_tree,
                    "",
                    &existing_oids,
                    &mut tree_entries,
                    &mut file_items,
                )?;
                tree_entries.sort_by(|a, b| a.path.cmp(&b.path));
                file_items.sort_by(|a, b| a.path.cmp(&b.path));
                Ok((tree_entries, file_items))
            }
        })
        .await??;

        // Step 2: Run UTF-8 + secret DLP redaction + line_offsets in PARALLEL on dedicated rayon pool!
        let to_process: Vec<(String, Vec<u8>)> = items
            .iter()
            .filter_map(|it| it.raw_bytes.as_ref().map(|b| (it.oid.clone(), b.clone())))
            .collect();
        let uncached_count = to_process.len();

        let (tx, rx) = tokio::sync::oneshot::channel();
        let pool = Arc::clone(&self.rayon_pool);
        pool.spawn(move || {
            let built: Vec<(String, Arc<RawBlob>)> = to_process
                .into_par_iter()
                .filter_map(|(oid, bytes)| {
                    RawBlob::from_raw_bytes(&bytes).map(|blob| (oid, Arc::new(blob)))
                })
                .collect();
            let _ = tx.send(built);
        });
        let newly_built = rx.await.context("rayon snapshot worker canceled")?;

        for (oid, blob) in newly_built {
            self.blob_pool.entry(oid).or_insert(blob);
        }

        // Step 3: Assemble contiguous `files` + O(1) `path_to_file`
        let mut files: Vec<(Arc<str>, Arc<RawBlob>)> = Vec::with_capacity(items.len());
        let mut path_to_file: HashMap<String, usize> = HashMap::with_capacity(items.len());
        for it in items {
            if let Some(blob_ref) = self.blob_pool.get(&it.oid) {
                let idx = files.len();
                path_to_file.insert(it.path.clone(), idx);
                files.push((Arc::from(it.path), Arc::clone(blob_ref.value())));
            }
        }

        // Step 4: Build compact CSR TrigramIndex in parallel on dedicated Rayon pool
        let (tri_tx, tri_rx) = tokio::sync::oneshot::channel();
        let tri_pool = Arc::clone(&self.rayon_pool);
        let tri_pool_exec = Arc::clone(&tri_pool);
        let files_for_tri = files;
        tri_pool.spawn(move || {
            let idx = TrigramIndex::build(&tri_pool_exec, &files_for_tri);
            let _ = tri_tx.send((files_for_tri, idx));
        });
        let (files, trigram_index) = tri_rx.await.context("rayon trigram worker canceled")?;

        let elapsed = started.elapsed();
        info!(
            "hydrated raw memory snapshot '{}@{}': files={} tree={} new_blobs={} total_blobs={} blob_mem={:.1}MiB trigram_mem={:.1}MiB elapsed={:?}",
            repo_name,
            &sha[..sha.len().min(10)],
            files.len(),
            tree.len(),
            uncached_count,
            self.blob_pool.len(),
            self.estimated_blob_memory_bytes() as f64 / (1024.0 * 1024.0),
            trigram_index.estimated_bytes as f64 / (1024.0 * 1024.0),
            elapsed
        );

        Ok(CommitRawSnapshot {
            repo: repo_name,
            commit_sha: sha,
            tree,
            files,
            path_to_file,
            trigram_index,
            last_used_ms: AtomicU64::new(now_millis_since_origin()),
        })
    }

    pub fn estimated_blob_memory_bytes(&self) -> u64 {
        self.blob_pool
            .iter()
            .map(|e| e.value().estimated_bytes())
            .sum()
    }

    pub fn estimated_total_memory_bytes(&self) -> u64 {
        let blob_bytes = self.estimated_blob_memory_bytes();
        let mut trigram_bytes: u64 = 0;
        for repo_entry in self.snapshots.iter() {
            for snap_entry in repo_entry.value().iter() {
                if let Some(s) = snap_entry.value().get() {
                    trigram_bytes = trigram_bytes.saturating_add(s.trigram_index.estimated_bytes);
                }
            }
        }
        blob_bytes.saturating_add(trigram_bytes)
    }

    fn remove_snapshot(&self, repo: &str, sha: &str) -> bool {
        let removed = self
            .snapshots
            .get(repo)
            .is_some_and(|m| m.remove(sha).is_some());
        if removed {
            self.branch_labels.remove(&(repo.to_string(), sha.to_string()));
        }
        removed
    }

    /// Periodic LRU + idle-TTL + hard memory cap eviction, followed by unreferenced blob sweep.
    pub fn gc(&self, max_keep: usize, max_memory_mb: u64, idle_ttl_secs: u64) -> usize {
        let keep_cap = max_keep.max(1);
        let mem_cap = max_memory_mb.max(32) * 1024 * 1024;
        self.set_limits(keep_cap, max_memory_mb);

        let mut entries: Vec<((String, String), u64, u64)> = Vec::new();
        for repo_entry in self.snapshots.iter() {
            let repo = repo_entry.key().clone();
            for snap_entry in repo_entry.value().iter() {
                if let Some(snap) = snap_entry.value().get() {
                    let sha = snap_entry.key().clone();
                    let last_ms = snap.last_used_ms.load(Ordering::Relaxed);
                    let idle = snap.idle_secs();
                    entries.push(((repo.clone(), sha), last_ms, idle));
                }
            }
        }

        // Most-recently-used first
        entries.sort_by(|a, b| b.1.cmp(&a.1));

        let mut evicted = 0usize;

        // 1. Evict entries exceeding max_keep or idle TTL
        let mut kept: Vec<(String, String)> = Vec::new();
        for (idx, ((repo, sha), _, idle)) in entries.into_iter().enumerate() {
            let expired = idle_ttl_secs > 0 && idle >= idle_ttl_secs;
            if idx >= keep_cap || expired {
                if self.remove_snapshot(&repo, &sha) {
                    evicted += 1;
                }
            } else {
                kept.push((repo, sha));
            }
        }

        // Sweep unreferenced blobs before checking memory cap
        self.sweep_unreferenced_blobs();

        // 2. Enforce hard memory cap by evicting oldest snapshots (always keeping at least 1 if possible)
        while kept.len() > 1 && self.estimated_total_memory_bytes() > mem_cap {
            if let Some((repo, sha)) = kept.pop() {
                if self.remove_snapshot(&repo, &sha) {
                    evicted += 1;
                }
                self.sweep_unreferenced_blobs();
            }
        }

        evicted
    }

    fn sweep_unreferenced_blobs(&self) {
        self.blob_pool
            .retain(|_, blob| Arc::strong_count(blob) > 1);
    }

    pub fn snapshot_metrics(&self) -> SnapshotPoolMetrics {
        let mut list = Vec::new();
        for repo_entry in self.snapshots.iter() {
            let repo = repo_entry.key().clone();
            for snap_entry in repo_entry.value().iter() {
                let Some(snap) = snap_entry.value().get() else {
                    continue;
                };
                let sha = snap_entry.key().clone();
                let branch = self
                    .branch_labels
                    .get(&(repo.clone(), sha.clone()))
                    .map(|b| b.value().clone())
                    .unwrap_or_default();
                list.push(MemorySnapshotMetric {
                    repo: repo.clone(),
                    branch,
                    commit_sha: sha,
                    files_count: snap.files.len(),
                    tree_entries_count: snap.tree.len(),
                    idle_secs: snap.idle_secs(),
                });
            }
        }
        list.sort_by(|a, b| a.idle_secs.cmp(&b.idle_secs));

        SnapshotPoolMetrics {
            active_snapshots: list.len(),
            max_snapshots: self.max_snapshots.load(Ordering::Relaxed),
            cached_blobs: self.blob_pool.len(),
            memory_bytes: self.estimated_total_memory_bytes(),
            max_memory_bytes: self.max_memory_bytes.load(Ordering::Relaxed),
            rayon_threads: self.rayon_threads,
            snapshots: list,
        }
    }
}

struct WalkRawFile {
    path: String,
    oid: String,
    raw_bytes: Option<Vec<u8>>,
}

fn walk_snapshot_tree(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: &str,
    existing_oids: &std::collections::HashSet<String>,
    tree_out: &mut Vec<TreeEntry>,
    files_out: &mut Vec<WalkRawFile>,
) -> Result<()> {
    for entry_res in tree.iter() {
        let entry = entry_res?;
        let name = entry.filename().to_str_lossy();
        if crate::should_skip_tree_name(&name) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if entry.mode().is_tree() {
            tree_out.push(TreeEntry {
                path: format!("{rel}/"),
                is_dir: true,
                size_bytes: None,
            });
            let sub = repo.find_tree(entry.oid())?;
            walk_snapshot_tree(repo, &sub, &rel, existing_oids, tree_out, files_out)?;
        } else if entry.mode().is_blob() {
            let oid_hex = entry.oid().to_hex().to_string();
            let mut size_bytes = None;

            let keep_for_raw = !is_secret_file_path(&rel) && !should_skip_grep_path(&rel);
            if keep_for_raw {
                if existing_oids.contains(&oid_hex) {
                    files_out.push(WalkRawFile {
                        path: rel.clone(),
                        oid: oid_hex,
                        raw_bytes: None,
                    });
                } else if let Ok(obj) = repo.find_object(entry.oid()) {
                    size_bytes = Some(obj.data.len() as u64);
                    if obj.data.len() <= MAX_SNAPSHOT_BLOB_BYTES && !obj.data.contains(&0u8) {
                        files_out.push(WalkRawFile {
                            path: rel.clone(),
                            oid: oid_hex,
                            raw_bytes: Some(obj.data.to_vec()),
                        });
                    }
                }
            } else if let Ok(obj) = repo.find_object(entry.oid()) {
                size_bytes = Some(obj.data.len() as u64);
            }

            tree_out.push(TreeEntry {
                path: rel,
                is_dir: false,
                size_bytes,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_blob_indexes_lines_and_redacts_secrets() {
        let raw = b"line1\nconst token = 'Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NSJ9.abcdefghijklmno';\nline3\n";
        let blob = RawBlob::from_raw_bytes(raw).unwrap();
        assert_eq!(blob.total_lines(), 3);
        assert_eq!(blob.line_str(0), "line1");
        assert!(blob.dlp_redactions >= 1);
        assert!(!blob.line_str(1).contains("eyJhbGci"));
        assert_eq!(blob.line_str(2), "line3");

        let (s, e, tot, out, trunc, red) = blob.format_line_range(Some(1), Some(2));
        assert_eq!((s, e, tot, trunc), (1, 2, 3, false));
        assert!(red >= 1);
        assert!(out.contains("    1 | line1"));
    }

    #[test]
    fn snapshot_parallel_grep_and_tree_work() {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        let b1 = Arc::new(RawBlob::from_sanitized(
            "fn main() {\n  HttpAccess::log();\n  println!(\"ok\");\n}\n".into(),
            0,
        ));
        let b2 = Arc::new(RawBlob::from_sanitized(
            "export const MODE = 'httpaccess';\n".into(),
            0,
        ));
        let mut path_to_file = HashMap::new();
        path_to_file.insert("apps/api/src/main.rs".into(), 0);
        path_to_file.insert("apps/web/src/app.ts".into(), 1);
        let files = vec![
            (Arc::from("apps/api/src/main.rs"), b1),
            (Arc::from("apps/web/src/app.ts"), b2),
        ];
        let trigram_index = TrigramIndex::build(&pool, &files);
        let snap = CommitRawSnapshot {
            repo: "twosite".into(),
            commit_sha: "abc123".into(),
            tree: vec![
                TreeEntry { path: "apps/".into(), is_dir: true, size_bytes: None },
                TreeEntry { path: "apps/api/".into(), is_dir: true, size_bytes: None },
                TreeEntry { path: "apps/api/src/".into(), is_dir: true, size_bytes: None },
                TreeEntry { path: "apps/api/src/main.rs".into(), is_dir: false, size_bytes: Some(40) },
                TreeEntry { path: "apps/web/".into(), is_dir: true, size_bytes: None },
                TreeEntry { path: "apps/web/src/".into(), is_dir: true, size_bytes: None },
                TreeEntry { path: "apps/web/src/app.ts".into(), is_dir: false, size_bytes: Some(30) },
            ],
            files,
            path_to_file,
            trigram_index,
            last_used_ms: AtomicU64::new(0),
        };

        // Case-insensitive grep across both files
        let (hits, trunc, _) = snap.grep(&pool, "HttpAccess", "", true, 1, 10).unwrap();
        assert!(!trunc);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].path, "apps/api/src/main.rs");
        assert_eq!(hits[0].line_number, 2);
        assert_eq!(hits[0].before, vec!["fn main() {"]);
        assert_eq!(hits[0].after, vec!["  println!(\"ok\");"]);

        // Scoped prefix grep
        let (scoped, _, _) = snap.grep(&pool, "HttpAccess", "apps/api", true, 0, 10).unwrap();
        assert_eq!(scoped.len(), 1);

        // Regex with quantifier, alternation, and miss pruning
        let (rx_hits, _, _) = snap.grep(&pool, r"HttpAccess::[a-z]+\(\)", "", false, 0, 10).unwrap();
        assert_eq!(rx_hits.len(), 1);
        let (alt_hits, _, _) = snap.grep(&pool, r"println|MODE", "", false, 0, 10).unwrap();
        assert_eq!(alt_hits.len(), 2);
        let (opt_hits, _, _) = snap.grep(&pool, r"MODEX?", "", false, 0, 10).unwrap();
        assert_eq!(opt_hits.len(), 1);
        let (miss_hits, _, _) = snap.grep(&pool, "ZZZ_NEVER_EXISTS", "", false, 0, 10).unwrap();
        assert!(miss_hits.is_empty());

        // Tree depth filter
        let (entries, _) = snap.list_tree("apps", 1, 20).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "apps/api/");
        assert_eq!(entries[1].path, "apps/web/");
    }
}
