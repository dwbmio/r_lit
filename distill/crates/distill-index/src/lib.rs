use anyhow::{Context, Result};
use dashmap::DashMap;
use distill_crypto::{DlpGate, EnvelopeCipher, SourceFingerprintIndex};
use distill_git::{GitRepoManager, dir_size_bytes};
use distill_proto::{
    BranchBehaviorDiff, BranchIndexMetric, SearchResultPayload, SemanticCard, SemanticKind,
};
use distill_semantic::extract_file_semantics;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, STORED, STRING, Schema, TEXT, Value};
use tantivy::tokenizer::{LowerCaser, TextAnalyzer, Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, ReloadPolicy, TantivyDocument, doc};
use tokio::sync::{OnceCell, Semaphore};
use tracing::info;

const TOKENIZER_NAME: &str = "distill_cjk_code";

#[derive(Serialize, Deserialize)]
struct CachedBlobSemantics {
    cards: Vec<SemanticCard>,
    /// Pre-computed 6-gram hashes not persisted separately; we store only cards and re-ingest fingerprints lazily or store raw hashes.
    fingerprint_hashes: Vec<u64>,
}

pub struct CommitIndex {
    pub repo: String,
    pub commit_sha: String,
    pub total_cards: usize,
    cards_by_id: HashMap<String, SemanticCard>,
    fingerprints: SourceFingerprintIndex,
    index: Index,
    reader: IndexReader,
    f_card_id: Field,
    _f_kind: Field,
    f_domain: Field,
    f_title: Field,
    f_summary: Field,
    f_tags: Field,
}

impl CommitIndex {
    pub fn search(
        &self,
        query_text: &str,
        kind_filter: Option<SemanticKind>,
        limit: usize,
    ) -> Result<(Vec<SemanticCard>, usize)> {
        let searcher = self.reader.searcher();
        let mut parser = QueryParser::for_index(
            &self.index,
            vec![self.f_title, self.f_summary, self.f_tags, self.f_domain],
        );
        parser.set_conjunction_by_default();

        // Fallback to lenient OR parsing if conjunction yields 0 hits
        let clean_query = sanitize_query_string(query_text);
        let parsed_strict = parser.parse_query_lenient(&clean_query).0;
        let fetch_limit = (limit * 4).clamp(20, 200);
        let mut docs = searcher.search(&parsed_strict, &TopDocs::with_limit(fetch_limit).order_by_score())?;

        if docs.is_empty() && !clean_query.is_empty() {
            let parser_or = QueryParser::for_index(
                &self.index,
                vec![self.f_title, self.f_summary, self.f_tags, self.f_domain],
            );
            let parsed_or = parser_or.parse_query_lenient(&clean_query).0;
            docs = searcher.search(&parsed_or, &TopDocs::with_limit(fetch_limit).order_by_score())?;
        }

        let mut hits = Vec::new();
        let mut dlp_redactions = 0usize;

        for (_score, addr) in docs {
            let retrieved: TantivyDocument = searcher.doc(addr)?;
            let Some(cid) = retrieved
                .get_first(self.f_card_id)
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let Some(orig_card) = self.cards_by_id.get(cid) else {
                continue;
            };
            if let Some(kf) = kind_filter
                && orig_card.kind != kf
            {
                continue;
            }
            let mut card = orig_card.clone();
            let report = DlpGate::sanitize_card(&mut card, Some(&self.fingerprints));
            dlp_redactions += report.redactions;
            hits.push(card);
            if hits.len() >= limit {
                break;
            }
        }

        Ok((hits, dlp_redactions))
    }

    pub fn cards_map(&self) -> &HashMap<String, SemanticCard> {
        &self.cards_by_id
    }

    pub fn fingerprints(&self) -> &SourceFingerprintIndex {
        &self.fingerprints
    }
}

pub struct IndexService {
    app_home: PathBuf,
    git: Arc<GitRepoManager>,
    cipher: EnvelopeCipher,
    /// Singleflight map per (repo, commit_sha): concurrent callers share one build.
    cache: DashMap<(String, String), Arc<OnceCell<Arc<CommitIndex>>>>,
    /// Tracks (last_branch_name, last_used_at) per (repo, commit_sha) for LRU/TTL GC & metrics.
    activity: DashMap<(String, String), (String, Instant)>,
    build_semaphore: Arc<Semaphore>,
}

impl IndexService {
    pub fn new(
        app_home: PathBuf,
        git: Arc<GitRepoManager>,
        cipher: EnvelopeCipher,
        max_concurrent_builds: usize,
    ) -> Self {
        Self {
            app_home,
            git,
            cipher,
            cache: DashMap::new(),
            activity: DashMap::new(),
            build_semaphore: Arc::new(Semaphore::new(max_concurrent_builds.max(1))),
        }
    }

    pub fn git(&self) -> &Arc<GitRepoManager> {
        &self.git
    }

    pub fn cached_index_count(&self) -> usize {
        self.cache.iter().filter(|e| e.value().initialized()).count()
    }

    pub fn indexed_cards_for(&self, repo: &str, commit_sha: &str) -> Option<usize> {
        let key = (repo.to_string(), commit_sha.to_string());
        self.cache
            .get(&key)
            .and_then(|cell| cell.get().map(|idx| idx.total_cards))
    }

    fn touch_activity(&self, repo: &str, commit_sha: &str, branch: Option<&str>) {
        let key = (repo.to_string(), commit_sha.to_string());
        let now = Instant::now();
        self.activity
            .entry(key)
            .and_modify(|(b, ts)| {
                if let Some(name) = branch
                    && !name.is_empty()
                {
                    *b = name.to_string();
                }
                *ts = now;
            })
            .or_insert_with(|| (branch.unwrap_or("").to_string(), now));
    }

    pub fn snapshot_index_metrics(&self) -> Vec<BranchIndexMetric> {
        let mut out = Vec::new();
        for entry in self.cache.iter() {
            let Some(idx) = entry.value().get() else {
                continue;
            };
            let (repo, sha) = entry.key();
            let (branch, idle_secs) = self
                .activity
                .get(&(repo.clone(), sha.clone()))
                .map(|a| (a.value().0.clone(), a.value().1.elapsed().as_secs()))
                .unwrap_or_default();
            let disk_dir = self
                .app_home
                .join("indexes")
                .join(repo)
                .join(&sha[..sha.len().min(16)]);
            out.push(BranchIndexMetric {
                repo: repo.clone(),
                branch,
                commit_sha: sha.clone(),
                total_cards: idx.total_cards,
                disk_bytes: dir_size_bytes(&disk_dir),
                idle_secs,
            });
        }
        out.sort_by(|a, b| a.idle_secs.cmp(&b.idle_secs));
        out
    }

    /// Periodic LRU & idle-TTL cleanup for cached branch indexes (memory + disk).
    pub fn gc_indexes(&self, max_keep: usize, idle_ttl_secs: u64) -> usize {
        let keep_cap = max_keep.max(1);
        let mut entries: Vec<((String, String), Instant)> = Vec::new();
        for entry in self.cache.iter() {
            if !entry.value().initialized() {
                continue;
            }
            let key = entry.key().clone();
            let ts = self
                .activity
                .get(&key)
                .map(|a| a.value().1)
                .unwrap_or_else(Instant::now);
            entries.push((key, ts));
        }

        // Most-recently-used first
        entries.sort_by(|a, b| b.1.cmp(&a.1));

        let mut to_evict: Vec<(String, String)> = Vec::new();
        let ttl = (idle_ttl_secs > 0).then(|| Duration::from_secs(idle_ttl_secs));
        for (idx, (key, last_used)) in entries.iter().enumerate() {
            let expired = ttl.is_some_and(|d| last_used.elapsed() >= d);
            if idx >= keep_cap || expired {
                to_evict.push(key.clone());
            }
        }

        let mut evicted = 0usize;
        for key in &to_evict {
            if self.cache.remove(key).is_some() {
                self.activity.remove(key);
                let disk_dir = self
                    .app_home
                    .join("indexes")
                    .join(&key.0)
                    .join(&key.1[..key.1.len().min(16)]);
                if disk_dir.exists() {
                    let _ = std::fs::remove_dir_all(&disk_dir);
                }
                info!("gc evicted branch index {}@{}", key.0, &key.1[..key.1.len().min(10)]);
                evicted += 1;
            }
        }

        // Prune orphan on-disk index directories beyond keep_cap (sorted by mtime)
        let indexes_root = self.app_home.join("indexes");
        if indexes_root.is_dir() {
            let active_dirs: HashSet<PathBuf> = self
                .cache
                .iter()
                .map(|e| {
                    indexes_root
                        .join(&e.key().0)
                        .join(&e.key().1[..e.key().1.len().min(16)])
                })
                .collect();
            let mut disk_dirs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
            if let Ok(repos) = std::fs::read_dir(&indexes_root) {
                for repo_ent in repos.flatten() {
                    let rdir = repo_ent.path();
                    if !rdir.is_dir() {
                        continue;
                    }
                    if let Ok(shas) = std::fs::read_dir(&rdir) {
                        for sha_ent in shas.flatten() {
                            let sdir = sha_ent.path();
                            if sdir.is_dir() && !active_dirs.contains(&sdir) {
                                let mtime = std::fs::metadata(&sdir)
                                    .and_then(|m| m.modified())
                                    .unwrap_or(std::time::UNIX_EPOCH);
                                disk_dirs.push((sdir, mtime));
                            }
                        }
                    }
                }
            }
            disk_dirs.sort_by(|a, b| b.1.cmp(&a.1));
            let disk_budget = keep_cap.saturating_sub(active_dirs.len());
            for (i, (dir, mtime)) in disk_dirs.into_iter().enumerate() {
                let expired = ttl.is_some_and(|d| {
                    mtime.elapsed().map(|el| el >= d).unwrap_or(false)
                });
                if i >= disk_budget || expired {
                    if std::fs::remove_dir_all(&dir).is_ok() {
                        info!("gc pruned stale index dir: {}", dir.display());
                        evicted += 1;
                    }
                }
            }
        }

        evicted
    }

    pub async fn get_or_build(&self, repo: &str, commit_sha: &str) -> Result<Arc<CommitIndex>> {
        self.touch_activity(repo, commit_sha, None);
        let key = (repo.to_string(), commit_sha.to_string());
        let cell = self
            .cache
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();

        let idx = cell
            .get_or_try_init(|| async {
                let _permit = self.build_semaphore.acquire().await?;
                self.build_commit_index(repo, commit_sha).await
            })
            .await?;

        Ok(Arc::clone(idx))
    }

    pub async fn search_branch(
        &self,
        repo: &str,
        branch: &str,
        query: &str,
        kind: Option<SemanticKind>,
        limit: usize,
    ) -> Result<SearchResultPayload> {
        let commit_sha = self.git.resolve_branch(repo, branch).await?;
        self.touch_activity(repo, &commit_sha, Some(branch));
        let index = self.get_or_build(repo, &commit_sha).await?;
        let capped = limit.clamp(1, 50);
        let (hits, dlp_redactions) = index.search(query, kind, capped)?;
        Ok(SearchResultPayload {
            repo: repo.to_string(),
            branch: branch.to_string(),
            commit_sha,
            total_cards_indexed: index.total_cards,
            dlp_redactions,
            hits,
        })
    }

    pub async fn diff_branch_behavior(
        &self,
        repo: &str,
        base_branch: &str,
        target_branch: &str,
    ) -> Result<BranchBehaviorDiff> {
        let base_sha = self.git.resolve_branch(repo, base_branch).await?;
        let target_sha = self.git.resolve_branch(repo, target_branch).await?;
        self.touch_activity(repo, &base_sha, Some(base_branch));
        self.touch_activity(repo, &target_sha, Some(target_branch));
        let (base_idx, target_idx) = tokio::try_join!(
            self.get_or_build(repo, &base_sha),
            self.get_or_build(repo, &target_sha)
        )?;

        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut removed_titles = Vec::new();
        let mut dlp_redactions = 0usize;

        for (cid, tcard) in target_idx.cards_map() {
            match base_idx.cards_map().get(cid) {
                None => {
                    let mut c = tcard.clone();
                    dlp_redactions +=
                        DlpGate::sanitize_card(&mut c, Some(target_idx.fingerprints())).redactions;
                    added.push(c);
                }
                Some(bcard) if bcard != tcard => {
                    let mut c = tcard.clone();
                    dlp_redactions +=
                        DlpGate::sanitize_card(&mut c, Some(target_idx.fingerprints())).redactions;
                    modified.push(c);
                }
                _ => {}
            }
        }

        for (cid, bcard) in base_idx.cards_map() {
            if !target_idx.cards_map().contains_key(cid) {
                let (safe_title, r) =
                    DlpGate::sanitize_text(&bcard.title, Some(base_idx.fingerprints()));
                dlp_redactions += r;
                removed_titles.push(safe_title);
            }
        }

        added.sort_by(|a, b| a.title.cmp(&b.title));
        modified.sort_by(|a, b| a.title.cmp(&b.title));
        removed_titles.sort();

        // Cap diff response size cleanly for MCP consumption
        added.truncate(80);
        modified.truncate(80);
        removed_titles.truncate(80);

        Ok(BranchBehaviorDiff {
            repo: repo.to_string(),
            base_branch: base_branch.to_string(),
            base_commit_sha: base_sha,
            target_branch: target_branch.to_string(),
            target_commit_sha: target_sha,
            added,
            modified,
            removed_titles,
            dlp_redactions,
        })
    }

    async fn build_commit_index(&self, repo: &str, commit_sha: &str) -> Result<Arc<CommitIndex>> {
        let started = std::time::Instant::now();
        let entries = self.git.list_indexable_blobs(repo, commit_sha).await?;
        let blob_cache_root = self.app_home.join("blob-cache").join(repo);
        std::fs::create_dir_all(&blob_cache_root)?;

        let mut all_cards: BTreeMap<String, SemanticCard> = BTreeMap::new();
        let mut fingerprints = SourceFingerprintIndex::default();
        let mut cache_hits = 0usize;
        let mut cache_misses = 0usize;

        for entry in entries {
            let cache_key = blake3::hash(format!("{}:{}", entry.path, entry.blob_oid).as_bytes())
                .to_hex()
                .to_string();
            let cache_path = blob_cache_root
                .join(&cache_key[..2])
                .join(format!("{cache_key}.enc"));
            let aad = format!("blob:{repo}:{cache_key}");

            let cards = if cache_path.exists() {
                if let Ok(enc) = std::fs::read(&cache_path)
                    && let Ok(raw) = self.cipher.decrypt(aad.as_bytes(), &enc)
                    && let Ok(cached) = postcard::from_bytes::<CachedBlobSemantics>(&raw)
                {
                    cache_hits += 1;
                    fingerprints.ingest_hashes(cached.fingerprint_hashes);
                    cached.cards
                } else {
                    cache_misses += 1;
                    self.extract_and_cache(
                        repo,
                        &entry.path,
                        &entry.blob_oid,
                        &cache_path,
                        aad.as_bytes(),
                        &mut fingerprints,
                    )
                    .await?
                }
            } else {
                cache_misses += 1;
                self.extract_and_cache(
                    repo,
                    &entry.path,
                    &entry.blob_oid,
                    &cache_path,
                    aad.as_bytes(),
                    &mut fingerprints,
                )
                .await?
            };

            for card in cards {
                all_cards.insert(card.card_id.clone(), card);
            }
        }

        // Build Tantivy in-memory/persisted index under $HOME/.distill/indexes/<repo>/<sha>
        let idx_dir = self
            .app_home
            .join("indexes")
            .join(repo)
            .join(&commit_sha[..commit_sha.len().min(16)]);
        if idx_dir.exists() {
            let _ = std::fs::remove_dir_all(&idx_dir);
        }
        std::fs::create_dir_all(&idx_dir)?;

        let (index, f_card_id, f_kind, f_domain, f_title, f_summary, f_tags) =
            create_tantivy_index(&idx_dir)?;

        let mut writer = index.writer(32 * 1024 * 1024)?;
        let mut cards_by_id = HashMap::with_capacity(all_cards.len());
        for (cid, card) in all_cards {
            let meta_joined = card
                .metadata
                .iter()
                .map(|(k, v)| format!("{k} {v}"))
                .collect::<Vec<_>>()
                .join(" ");
            let tags_joined = format!("{} {}", card.tags.join(" "), meta_joined);
            writer.add_document(doc!(
                f_card_id => cid.as_str(),
                f_kind => card.kind.as_str(),
                f_domain => card.domain.as_str(),
                f_title => card.title.as_str(),
                f_summary => card.summary.as_str(),
                f_tags => tags_joined.as_str(),
            ))?;
            cards_by_id.insert(cid, card);
        }
        writer.commit()?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;

        info!(
            "indexed repo='{repo}' sha={} cards={} cache_hits={} cache_misses={} elapsed={:?}",
            &commit_sha[..commit_sha.len().min(10)],
            cards_by_id.len(),
            cache_hits,
            cache_misses,
            started.elapsed()
        );

        Ok(Arc::new(CommitIndex {
            repo: repo.to_string(),
            commit_sha: commit_sha.to_string(),
            total_cards: cards_by_id.len(),
            cards_by_id,
            fingerprints,
            index,
            reader,
            f_card_id,
            _f_kind: f_kind,
            f_domain,
            f_title,
            f_summary,
            f_tags,
        }))
    }

    async fn extract_and_cache(
        &self,
        repo: &str,
        rel_path: &str,
        blob_oid: &str,
        cache_path: &Path,
        aad: &[u8],
        fingerprints: &mut SourceFingerprintIndex,
    ) -> Result<Vec<SemanticCard>> {
        let Some(text) = self.git.read_blob_utf8(repo, blob_oid).await? else {
            return Ok(Vec::new());
        };
        let fp_hashes = if rel_path.ends_with(".ts") || rel_path.ends_with(".tsx") {
            let h = SourceFingerprintIndex::extract_source_hashes(&text);
            fingerprints.ingest_hashes(h.iter().copied());
            h
        } else {
            Vec::new()
        };
        let cards = extract_file_semantics(rel_path, &text);
        let payload = CachedBlobSemantics {
            cards: cards.clone(),
            fingerprint_hashes: fp_hashes,
        };
        if let Ok(raw) = postcard::to_allocvec(&payload)
            && let Ok(enc) = self.cipher.encrypt(aad, &raw)
        {
            if let Some(parent) = cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(cache_path, enc);
        }
        Ok(cards)
    }
}

fn create_tantivy_index(
    dir: &Path,
) -> Result<(Index, Field, Field, Field, Field, Field, Field)> {
    let mut sb = Schema::builder();
    let text_opts = tantivy::schema::TextOptions::default()
        .set_indexing_options(
            tantivy::schema::TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER_NAME)
                .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();

    let f_card_id = sb.add_text_field("card_id", STRING | STORED);
    let f_kind = sb.add_text_field("kind", STRING | STORED);
    let f_domain = sb.add_text_field("domain", text_opts.clone());
    let f_title = sb.add_text_field("title", text_opts.clone());
    let f_summary = sb.add_text_field("summary", text_opts.clone());
    let f_tags = sb.add_text_field("tags", text_opts);
    let schema = sb.build();

    let index = Index::create_in_dir(dir, schema)
        .with_context(|| format!("failed to create tantivy index at {}", dir.display()))?;
    let analyzer = TextAnalyzer::builder(CjkCodeTokenizer)
        .filter(LowerCaser)
        .build();
    index.tokenizers().register(TOKENIZER_NAME, analyzer);

    let _ = TEXT; // keep import tidy
    Ok((index, f_card_id, f_kind, f_domain, f_title, f_summary, f_tags))
}

fn sanitize_query_string(q: &str) -> String {
    q.chars()
        .map(|c| {
            if matches!(c, '+' | '-' | '!' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '"' | '~' | '*' | '?' | ':' | '\\' | '/') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Pure Rust CJK + camelCase + snake_case aware tokenizer for Tantivy.
#[derive(Clone, Default)]
struct CjkCodeTokenizer;

struct CjkCodeTokenStream {
    tokens: Vec<Token>,
    idx: usize,
}

impl Tokenizer for CjkCodeTokenizer {
    type TokenStream<'a> = CjkCodeTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let mut tokens = Vec::new();
        let mut pos = 0usize;
        let bytes = text.as_bytes();
        let mut i = 0usize;

        while i < bytes.len() {
            let ch = text[i..].chars().next().unwrap();
            let ch_len = ch.len_utf8();

            if is_cjk(ch) {
                tokens.push(Token {
                    offset_from: i,
                    offset_to: i + ch_len,
                    position: pos,
                    text: ch.to_string(),
                    position_length: 1,
                });
                pos += 1;
                i += ch_len;
            } else if ch.is_ascii_alphanumeric() {
                let start = i;
                while i < bytes.len() {
                    let c = text[i..].chars().next().unwrap();
                    if c.is_ascii_alphanumeric() {
                        i += c.len_utf8();
                    } else {
                        break;
                    }
                }
                let word = &text[start..i];
                // Emit full token
                tokens.push(Token {
                    offset_from: start,
                    offset_to: i,
                    position: pos,
                    text: word.to_string(),
                    position_length: 1,
                });
                // Also emit camelCase subtokens for code identifiers like qaValuePointRound1
                for sub in split_camel(word) {
                    if sub.len() < word.len() && sub.len() >= 2 {
                        tokens.push(Token {
                            offset_from: start,
                            offset_to: i,
                            position: pos,
                            text: sub.to_string(),
                            position_length: 1,
                        });
                    }
                }
                pos += 1;
            } else {
                i += ch_len;
            }
        }

        CjkCodeTokenStream { tokens, idx: 0 }
    }
}

impl TokenStream for CjkCodeTokenStream {
    fn advance(&mut self) -> bool {
        if self.idx < self.tokens.len() {
            self.idx += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.idx - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.idx - 1]
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF)
}

fn split_camel(word: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let bytes = word.as_bytes();
    let mut start = 0usize;
    for i in 1..bytes.len() {
        let prev = bytes[i - 1];
        let curr = bytes[i];
        if (prev.is_ascii_lowercase() && curr.is_ascii_uppercase())
            || (prev.is_ascii_alphabetic() && curr.is_ascii_digit())
            || (prev.is_ascii_digit() && curr.is_ascii_alphabetic())
        {
            parts.push(&word[start..i]);
            start = i;
        }
    }
    if start < bytes.len() {
        parts.push(&word[start..]);
    }
    parts
}
