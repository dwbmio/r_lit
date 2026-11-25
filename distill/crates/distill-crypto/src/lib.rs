use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use distill_proto::SemanticCard;
use rand::RngCore;
use regex::Regex;
use sha2::Sha256;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;
use thiserror::Error;

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;
const FINGERPRINT_NGRAM: usize = 6;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("encryption failed")]
    Encrypt,
    #[error("decryption failed or authentication tag mismatch")]
    Decrypt,
    #[error("ciphertext too short")]
    Truncated,
    #[error("key derivation failed")]
    Hkdf,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid key material: {0}")]
    InvalidKey(String),
}

/// Pure Rust envelope AEAD cipher using HKDF-SHA256 + ChaCha20-Poly1305.
#[derive(Clone)]
pub struct EnvelopeCipher {
    master_key: [u8; KEY_LEN],
}

impl EnvelopeCipher {
    pub fn new(master_key: [u8; KEY_LEN]) -> Self {
        Self { master_key }
    }

    pub fn load_or_generate(path: &Path) -> Result<Self, CryptoError> {
        if path.exists() {
            let raw = fs::read_to_string(path)?;
            let decoded = hex::decode(raw.trim())
                .map_err(|e| CryptoError::InvalidKey(format!("hex decode failed: {e}")))?;
            let key: [u8; KEY_LEN] = decoded
                .try_into()
                .map_err(|_| CryptoError::InvalidKey("expected 32-byte key".into()))?;
            return Ok(Self::new(key));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut key = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut key);
        fs::write(path, format!("{}\n", hex::encode(key)))?;
        set_owner_only_permissions(path)?;
        Ok(Self::new(key))
    }

    fn derive_subkey(&self, context: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
        let hk = Hkdf::<Sha256>::new(Some(b"distill-v1"), &self.master_key);
        let mut okm = [0u8; KEY_LEN];
        hk.expand(context, &mut okm).map_err(|_| CryptoError::Hkdf)?;
        Ok(okm)
    }

    pub fn encrypt(&self, context: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let subkey = self.derive_subkey(context)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&subkey));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: context,
                },
            )
            .map_err(|_| CryptoError::Encrypt)?;

        let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    pub fn decrypt(&self, context: &[u8], blob: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < NONCE_LEN + 16 {
            return Err(CryptoError::Truncated);
        }
        let subkey = self.derive_subkey(context)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&subkey));
        let nonce = Nonce::from_slice(&blob[..NONCE_LEN]);
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: &blob[NONCE_LEN..],
                    aad: context,
                },
            )
            .map_err(|_| CryptoError::Decrypt)
    }
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

/// Compact BLAKE3 6-gram fingerprint table of raw source code tokens.
/// Used by the outbound DLP gate to guarantee raw source code snippets can never leak.
#[derive(Debug, Clone, Default)]
pub struct SourceFingerprintIndex {
    hashes: HashSet<u64>,
}

impl SourceFingerprintIndex {
    pub fn extract_source_hashes(source: &str) -> Vec<u64> {
        let tokens = tokenize_code(source);
        if tokens.len() < FINGERPRINT_NGRAM {
            return Vec::new();
        }
        tokens.windows(FINGERPRINT_NGRAM).map(hash_ngram).collect()
    }

    pub fn ingest_source(&mut self, source: &str) {
        for h in Self::extract_source_hashes(source) {
            self.hashes.insert(h);
        }
    }

    pub fn ingest_hashes(&mut self, hashes: impl IntoIterator<Item = u64>) {
        self.hashes.extend(hashes);
    }

    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    pub fn matches_source_window(&self, text: &str) -> bool {
        if self.hashes.is_empty() {
            return false;
        }
        let tokens = tokenize_code(text);
        if tokens.len() < FINGERPRINT_NGRAM {
            return false;
        }
        tokens
            .windows(FINGERPRINT_NGRAM)
            .any(|w| self.hashes.contains(&hash_ngram(w)))
    }
}

fn tokenize_code(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
        } else {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            if matches!(ch, '{' | '}' | '(' | ')' | ';' | '=' | ':' | '<' | '>' | '[' | ']') {
                tokens.push(ch.to_string());
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn hash_ngram(tokens: &[String]) -> u64 {
    let mut hasher = blake3::Hasher::new();
    for (idx, tok) in tokens.iter().enumerate() {
        if idx > 0 {
            hasher.update(b"\x1f");
        }
        hasher.update(tok.as_bytes());
    }
    let digest = hasher.finalize();
    u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap())
}

static RE_CODE_FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)```.*?```").unwrap());
static RE_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:bearer\s+[a-z0-9._\-]{16,}|(?:postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|redis)://[^\s'"`]+|-----BEGIN [A-Z ]+PRIVATE KEY-----[\s\S]*?-----END [A-Z ]+PRIVATE KEY-----|-----BEGIN [A-Z ]+PRIVATE KEY-----|\b(?:sk-[A-Za-z0-9_\-]{20,}|ghp_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_\-]{16,}|AKIA[0-9A-Z]{16})\b|(?:api[_-]?key|secret|token|password|passwd|access[_-]?key)\s*[:=]\s*['"]?[A-Za-z0-9_\-\.]{12,}['"]?)"#).unwrap()
});
static RE_HOST_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:/Users/[A-Za-z0-9._\-]+|/home/[A-Za-z0-9._\-]+|/www/wwwroot/[A-Za-z0-9._\-/]+|/var/lib/[A-Za-z0-9._\-/]+)").unwrap()
});
static RE_CODE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:import\s+.*\s+from\s+|export\s+(?:default\s+)?(?:async\s+)?(?:function|class|const|let|interface)\s+|async\s+function\s+|return\s+await\s+|await\s+this\.|if\s*\(.+\)\s*\{)").unwrap()
});

#[derive(Debug, Default)]
pub struct DlpSanitizeReport {
    pub redactions: usize,
}

/// Deterministic Pure Rust outbound DLP gate.
pub struct DlpGate;

impl DlpGate {
    /// Redact only secrets, credentials, private keys, and connection strings.
    /// Keeps source code, configs, and documentation intact for fast raw-mode access.
    pub fn sanitize_secrets(input: &str) -> (String, usize) {
        if !RE_SECRET.is_match(input) {
            return (input.to_string(), 0);
        }
        let count = RE_SECRET.find_iter(input).count();
        let cleaned = RE_SECRET
            .replace_all(input, "[REDACTED_SECRET]")
            .into_owned();
        (cleaned, count)
    }

    pub fn sanitize_text(
        input: &str,
        fingerprints: Option<&SourceFingerprintIndex>,
    ) -> (String, usize) {
        let mut redactions = 0usize;
        let mut out = input.to_string();

        // 1. Strip any fenced code blocks unconditionally.
        if RE_CODE_FENCE.is_match(&out) {
            redactions += RE_CODE_FENCE.find_iter(&out).count();
            out = RE_CODE_FENCE
                .replace_all(&out, "[已按安全策略隐藏代码块]")
                .into_owned();
        }

        // 2. Redact secrets and host filesystem paths.
        if RE_SECRET.is_match(&out) {
            redactions += RE_SECRET.find_iter(&out).count();
            out = RE_SECRET.replace_all(&out, "[REDACTED_SECRET]").into_owned();
        }
        if RE_HOST_PATH.is_match(&out) {
            redactions += RE_HOST_PATH.find_iter(&out).count();
            out = RE_HOST_PATH.replace_all(&out, "[REDACTED_PATH]").into_owned();
        }

        // 3. Line-level code syntax & BLAKE3 source fingerprint enforcement.
        let mut safe_lines = Vec::new();
        for line in out.lines() {
            let looks_like_code = RE_CODE_LINE.is_match(line) || has_high_code_symbol_density(line);
            let matches_fingerprint = fingerprints
                .map(|fp| fp.matches_source_window(line))
                .unwrap_or(false);

            if looks_like_code || matches_fingerprint {
                redactions += 1;
                safe_lines.push("[已按源码保护策略隐藏实现细节]");
            } else {
                safe_lines.push(line);
            }
        }

        (safe_lines.join("\n"), redactions)
    }

    pub fn sanitize_card(
        card: &mut SemanticCard,
        fingerprints: Option<&SourceFingerprintIndex>,
    ) -> DlpSanitizeReport {
        let mut report = DlpSanitizeReport::default();

        let (title, r1) = Self::sanitize_text(&card.title, fingerprints);
        card.title = title;
        report.redactions += r1;

        let (summary, r2) = Self::sanitize_text(&card.summary, fingerprints);
        card.summary = summary;
        report.redactions += r2;

        for val in card.metadata.values_mut() {
            let (clean, r) = Self::sanitize_text(val, fingerprints);
            *val = clean;
            report.redactions += r;
        }

        report
    }
}

fn has_high_code_symbol_density(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 24 {
        return false;
    }
    let code_syms = trimmed
        .chars()
        .filter(|c| matches!(c, '{' | '}' | ';' | '(' | ')' | '=' | '>' | '<'))
        .count();
    // Require both semicolon/braces and high symbol ratio so normal prose/JSON descriptions pass.
    let has_stmt_punct = trimmed.ends_with(';') || (trimmed.contains('{') && trimmed.contains('}'));
    has_stmt_punct && code_syms * 100 / trimmed.len() >= 18
}

#[cfg(test)]
mod tests {
    use super::*;
    use distill_proto::SemanticKind;
    use std::collections::BTreeMap;

    #[test]
    fn envelope_encrypt_decrypt_and_aad_binding() {
        let cipher = EnvelopeCipher::new([7u8; KEY_LEN]);
        let ct = cipher.encrypt(b"repo:twosite:blob1", b"semantic payload").unwrap();
        let pt = cipher.decrypt(b"repo:twosite:blob1", &ct).unwrap();
        assert_eq!(pt, b"semantic payload");
        // Wrong AAD must fail authentication.
        assert!(cipher.decrypt(b"repo:twosite:blob2", &ct).is_err());
    }

    #[test]
    fn dlp_blocks_code_fences_and_source_fingerprints_and_secrets() {
        let mut fp = SourceFingerprintIndex::default();
        fp.ingest_source(
            "async function computePriorityScore(manager, workspaceId, workItemId) { return await manager.findOneBy(WorkItemEntity, { id: workItemId }); }",
        );

        let leaked = "说明如下：\n```ts\nconst x = 1;\n```\n调用细节：computePriorityScore(manager, workspaceId, workItemId)\n连接串 postgres://user:pass@10.0.0.1:5432/db\n路径 /Users/admin/data0/twosite";
        let (clean, count) = DlpGate::sanitize_text(leaked, Some(&fp));
        assert!(count >= 4);
        assert!(!clean.contains("const x = 1"));
        assert!(!clean.contains("computePriorityScore"));
        assert!(!clean.contains("postgres://"));
        assert!(!clean.contains("/Users/admin"));
        assert!(clean.contains("说明如下："));
    }

    #[test]
    fn dlp_allows_clean_business_semantic_card() {
        let mut card = SemanticCard {
            card_id: "api:1".into(),
            kind: SemanticKind::ApiEndpoint,
            domain: "work-items".into(),
            title: "PATCH /api/v1/work-items/:workItemId — 更新工作项字段".into(),
            summary: "按 expectedStateVersion 乐观锁增量更新 customFields（如 qaValuePointRound1 价值点（集测1轮））。".into(),
            metadata: BTreeMap::from([("permissions".into(), "work-item:edit".into())]),
            tags: vec!["work-items".into()],
        };
        let report = DlpGate::sanitize_card(&mut card, None);
        assert_eq!(report.redactions, 0);
        assert!(card.summary.contains("qaValuePointRound1"));
    }
}
