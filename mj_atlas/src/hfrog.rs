//! Minimal hfrog artifact-registry client.
//!
//! Wraps the `PUT /artifactory/add_form_file` endpoint that hfrog exposes for
//! multipart uploads (server: `hfrog/src/services/api/artifactory/`). The
//! client is intentionally narrow — we only push artifacts; reads / queries
//! are out of scope for this milestone.
//!
//! Failure semantics: every method returns `Result<()>` but mj_atlas wraps
//! these calls in best-effort flushes — a failed upload is logged via the
//! runlog and surfaced in the GUI status bar but never aborts the local
//! save / pack pipeline. Losing the mirror is preferable to losing the user's
//! local work.

use crate::config::HfrogConfig;
use crate::error::{AppError, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// One artifact's per-upload metadata (corresponds to hfrog's `ArtifactoryModel`).
/// Fields that the server assigns (pid, create_time) or that we don't use
/// (tag, min/max_runtime_ver, ci_info) are omitted.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ArtifactSpec {
    /// `name = <project>.<file_kind>` — see `artifact_name_for` for the
    /// convention used by mj_atlas (e.g. `myproj.tpproj`, `myproj.atlas.png`).
    pub name: String,
    pub ver: String,
    /// Server expects exactly 32 chars of MD5 hex; we send the first 32 chars
    /// of the SHA-256 hex digest of the file contents (collisions are still
    /// astronomically unlikely at that prefix length, and we don't have to
    /// pull in an MD5 dependency just to satisfy a name).
    pub md5: String,
    pub descript: String,
    pub cont_size: i64,
    pub runtime: String,
    /// S3 key the server should store the bytes under. We pick a deterministic
    /// path: `mj_atlas/<project>/<ver>/<filename>`.
    pub s3_key: String,
    pub s3_inc_id: i32,
    pub is_artifactory_ready: bool,
    /// `is_raw=Some(false)` ⇒ standard "file" artifact path; we always set false
    /// because we use the `add_form_file` endpoint.
    pub is_raw: Option<bool>,
    /// Stable file extension hint for the registry's UI / search.
    pub key_extension: Option<String>,
}

/// Stateless client — we instantiate per-upload so the GUI can swap config
/// without reaching into the runtime. `reqwest::blocking::Client` is fine
/// because hfrog uploads happen from background threads (GUI worker) or
/// from the synchronous CLI — no shared async runtime needed.
pub struct Client {
    // Crate-visible so the `list_cloud_projects` / `download_project` free
    // functions can reuse the same auth + base URL without re-validating.
    pub(crate) inner: reqwest::blocking::Client,
    pub(crate) endpoint: String,
    pub(crate) token: String,
}

impl Client {
    /// Build a client from the user config. Returns `None` when the config is
    /// inactive (disabled / empty endpoint) — callers can early-return without
    /// constructing a client.
    pub fn from_config(cfg: &HfrogConfig) -> Option<Self> {
        if !cfg.is_active() {
            return None;
        }
        // 30 second timeout: atlas PNGs are typically <1 MB and uploads to
        // a same-region hfrog complete in <1 s, but slow networks shouldn't
        // hang the user's UI indefinitely — fail fast and log.
        let inner = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self {
            inner,
            endpoint: cfg.endpoint.trim_end_matches('/').to_string(),
            token: cfg.token.clone(),
        })
    }

    /// Push a single file's bytes plus its metadata to hfrog.
    ///
    /// `bytes` is consumed (moved into the multipart form). Returns Ok(()) on
    /// any HTTP 2xx response — hfrog's "already-exists" branch is treated as
    /// success because mj_atlas re-uploads idempotently each save.
    pub fn upload_file(&self, spec: &ArtifactSpec, bytes: Vec<u8>, filename: &str) -> Result<()> {
        // Hfrog mounts the artifactory routes under `/api/artifactory` (see
        // hfrog/src/services/api/artifactory/mod.rs::register). Earlier
        // releases of this client used `/artifactory` directly which 404'd
        // against the production deployment.
        let url = format!("{}/api/artifactory/add_form_file", self.endpoint);
        let json_payload = serde_json::to_string(spec)
            .map_err(|e| AppError::Custom(format!("hfrog: serialize spec: {}", e)))?;

        let file_part = reqwest::blocking::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(mime_for(filename))
            .map_err(|e| AppError::Custom(format!("hfrog: mime: {}", e)))?;

        // Server-side `MPJson<ArtifactoryModel>` requires the part's
        // Content-Type to be `application/json`. `Form::text(...)` would
        // tag it as `text/plain` and the actix-multipart extractor returns
        // `An error occurred processing field: json` instead. Build the
        // part explicitly with the correct mime.
        let json_part = reqwest::blocking::multipart::Part::text(json_payload)
            .mime_str("application/json")
            .map_err(|e| AppError::Custom(format!("hfrog: json mime: {}", e)))?;

        let form = reqwest::blocking::multipart::Form::new()
            .part("json", json_part)
            .part("file", file_part);

        let mut req = self.inner.put(&url).multipart(form);
        if !self.token.is_empty() {
            req = req.bearer_auth(&self.token);
        }

        log::info!(
            "hfrog: uploading {} ({} bytes, ver={}) → {}",
            spec.name,
            spec.cont_size,
            spec.ver,
            url
        );
        let response = req
            .send()
            .map_err(|e| AppError::Custom(format!("hfrog: send: {}", e)))?;

        let status = response.status();
        let body = response
            .text()
            .unwrap_or_else(|_| "(no body)".to_string());

        if !status.is_success() {
            return Err(AppError::Custom(format!(
                "hfrog: HTTP {} from {} — body: {}",
                status, url, body
            )));
        }

        // hfrog returns `RespRet { code, msg }`; code != 0 ⇒ business error.
        // A "1001 already exists" is treated as success (idempotent re-upload).
        if let Ok(parsed) = serde_json::from_str::<RespRet>(&body) {
            if parsed.code != 0 && parsed.code != 1001 {
                return Err(AppError::Custom(format!(
                    "hfrog: business error {} — {}",
                    parsed.code, parsed.msg
                )));
            }
            log::info!(
                "hfrog: upload OK {} (code={}, msg={})",
                spec.name,
                parsed.code,
                parsed.msg
            );
        } else {
            log::warn!(
                "hfrog: upload returned 2xx but body was unparseable: {}",
                body
            );
        }
        Ok(())
    }
}

/// Hfrog's universal response envelope. We only care about `code` for
/// success/error classification; `msg` is logged on failure for diagnosis.
#[derive(Debug, serde::Deserialize)]
struct RespRet {
    code: i32,
    #[serde(default)]
    msg: String,
}

/// Two-runtime taxonomy on the hfrog side. mj_atlas v0.5+ registers both
/// runtimes in the registry so cloud listings can be filtered cleanly:
///   - `mj_atlas-project`: the `.tpproj` project bundle (one per project)
///   - `mj_atlas-atlas`:   everything else (atlas .png / .json / .manifest /
///                         .tpsheet / .tres / log)
///
/// Earlier versions used a single configurable `default_runtime` ("asset-pack")
/// for everything. We pivoted because the Welcome screen needs to query
/// "give me all PROJECTS" without scanning per-pack atlas dumps.
pub const RUNTIME_PROJECT: &str = "mj_atlas-project";
pub const RUNTIME_ATLAS: &str = "mj_atlas-atlas";

fn runtime_for_kind(file_kind: &str) -> &'static str {
    if file_kind == "tpproj" {
        RUNTIME_PROJECT
    } else {
        RUNTIME_ATLAS
    }
}

/// Build a fresh `ArtifactSpec` for a file destined for hfrog.
///
/// `project_name` typically maps to the `.tpproj` basename. `file_kind` is a
/// short tag describing what KIND of artifact this is (`tpproj`, `atlas-png`,
/// `atlas-json`, `manifest`, `log`) — joined with `project_name` to form the
/// `name` field, used to derive the S3 key suffix, AND used to pick the
/// runtime ("mj_atlas-project" for tpproj, "mj_atlas-atlas" for everything
/// else).
pub fn build_spec(
    cfg: &HfrogConfig,
    project_name: &str,
    file_kind: &str,
    filename: &str,
    bytes: &[u8],
    ver: &str,
) -> ArtifactSpec {
    let cont_size = bytes.len() as i64;
    // Hfrog's `md5` column is exactly 32 chars — we hand it the first 32 hex
    // chars of SHA-256 instead of pulling in an MD5 dep. The collision space
    // (16^32) is more than enough for asset dedup.
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let md5 = digest
        .iter()
        .take(16)
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

    ArtifactSpec {
        name: format!("{}.{}", project_name, file_kind),
        ver: ver.to_string(),
        md5,
        descript: format!("mj_atlas {} of '{}'", file_kind, project_name),
        cont_size,
        runtime: runtime_for_kind(file_kind).to_string(),
        s3_key: format!("mj_atlas/{}/{}/{}", project_name, ver, filename),
        s3_inc_id: cfg.s3_inc_id as i32,
        is_artifactory_ready: false,
        is_raw: Some(false),
        key_extension: Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_string()),
    }
}

// ─── Read side: list cloud projects, download by name ──────────────────────
//
// New in v0.5.0 — turns hfrog from a write-only mirror into a "cloud drive"
// for the Welcome screen's project picker. Both helpers swallow network
// failures and return Ok(empty) / Err(string) the GUI can ignore safely.

/// One row in the cloud project list shown on the Welcome screen.
#[derive(Debug, Clone)]
pub struct CloudProject {
    /// Hfrog `name` field — `<project>.tpproj` for our convention.
    pub name: String,
    /// Project-level basename (the part before `.tpproj`). What the user types.
    pub display_name: String,
    /// Latest version string we've pushed. Used by `download_project` to
    /// resolve the right artifact when multiple versions coexist.
    pub ver: String,
    /// Pre-decode SHA256 prefix from when we last pushed (or whatever hfrog
    /// returns). Used for local-vs-cloud merge tagging.
    pub md5: String,
    /// File size in bytes (just for display).
    pub cont_size: i64,
}

/// List every `.tpproj` artifact registered under `mj_atlas-project`. The
/// hfrog `list` endpoint already takes care of pagination; we cap at 50 to
/// keep the Welcome screen quick.
pub fn list_cloud_projects(cfg: &HfrogConfig) -> Result<Vec<CloudProject>> {
    let client = Client::from_config(cfg)
        .ok_or_else(|| AppError::Custom("hfrog: client inactive".into()))?;
    let url = format!(
        "{}/api/artifactory/list?index=0&cnt=50",
        client.endpoint
    );
    let mut req = client.inner.get(&url);
    if !client.token.is_empty() {
        req = req.bearer_auth(&client.token);
    }

    let resp = req
        .send()
        .map_err(|e| AppError::Custom(format!("hfrog: list request: {}", e)))?;
    if !resp.status().is_success() {
        return Err(AppError::Custom(format!(
            "hfrog: list HTTP {}",
            resp.status()
        )));
    }
    let body = resp
        .text()
        .map_err(|e| AppError::Custom(format!("hfrog: list body: {}", e)))?;

    // Server response: `{"code":0,"msg":"","data":[ArtifactoryModel,...]}`
    // We only need a few fields — declare a thin shape so unknown fields
    // don't trip serde and a future schema change doesn't break us.
    #[derive(serde::Deserialize)]
    struct ListResp {
        code: i32,
        #[serde(default)]
        msg: String,
        #[serde(default)]
        data: Vec<ListItem>,
    }
    #[derive(serde::Deserialize)]
    struct ListItem {
        #[serde(default)]
        name: String,
        #[serde(default)]
        ver: String,
        #[serde(default)]
        md5: String,
        #[serde(default)]
        cont_size: i64,
        #[serde(default)]
        runtime: String,
    }

    let parsed: ListResp = serde_json::from_str(&body)
        .map_err(|e| AppError::Custom(format!("hfrog: list parse: {} (body={})", e, body)))?;
    if parsed.code != 0 {
        return Err(AppError::Custom(format!(
            "hfrog: list code={} msg={}",
            parsed.code, parsed.msg
        )));
    }

    // Filter to project-runtime entries. The server's `runtime` column is a
    // CHAR(20) so it pads with trailing spaces — we trim before comparing.
    let mut projects: Vec<CloudProject> = parsed
        .data
        .into_iter()
        .filter(|it| it.runtime.trim() == RUNTIME_PROJECT)
        .filter(|it| it.name.ends_with(".tpproj"))
        .map(|it| CloudProject {
            display_name: it
                .name
                .strip_suffix(".tpproj")
                .unwrap_or(&it.name)
                .to_string(),
            name: it.name,
            ver: it.ver,
            md5: it.md5,
            cont_size: it.cont_size,
        })
        .collect();

    // Stable order: most-likely-name-first by display name. The user clicks
    // by project name, not by upload time, so alphabetical reads cleaner.
    projects.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(projects)
}

/// Download one cloud project's `.tpproj` bytes via hfrog's presigned-URL
/// endpoint. Two HTTP calls: hfrog returns an S3 URL, we GET that URL.
///
/// When the server's S3 backend is misconfigured this fails at the first
/// hop (`get_object_presigned_url` returns code 1003 / 1004); the caller
/// surfaces that as a toast.
pub fn download_project(cfg: &HfrogConfig, project: &CloudProject) -> Result<Vec<u8>> {
    let client = Client::from_config(cfg)
        .ok_or_else(|| AppError::Custom("hfrog: client inactive".into()))?;
    let url = format!(
        "{}/api/artifactory/get_object_presigned_url?name={}&ver={}&runtime={}",
        client.endpoint,
        urlencode(&project.name),
        urlencode(&project.ver),
        urlencode(RUNTIME_PROJECT)
    );
    let mut req = client.inner.get(&url);
    if !client.token.is_empty() {
        req = req.bearer_auth(&client.token);
    }
    let resp = req
        .send()
        .map_err(|e| AppError::Custom(format!("hfrog: presign request: {}", e)))?;
    let body = resp
        .text()
        .map_err(|e| AppError::Custom(format!("hfrog: presign body: {}", e)))?;

    // Server replies with `RespVO<String>` where `data` is the presigned URL.
    #[derive(serde::Deserialize)]
    struct PresignResp {
        code: i32,
        #[serde(default)]
        msg: String,
        #[serde(default)]
        data: String,
    }
    let parsed: PresignResp = serde_json::from_str(&body)
        .map_err(|e| AppError::Custom(format!("hfrog: presign parse: {} (body={})", e, body)))?;
    if parsed.code != 0 || parsed.data.is_empty() {
        return Err(AppError::Custom(format!(
            "hfrog: presign code={} msg={}",
            parsed.code, parsed.msg
        )));
    }

    // Hop two: GET the S3 URL. No auth header — the URL is signed.
    let s3_resp = client
        .inner
        .get(&parsed.data)
        .send()
        .map_err(|e| AppError::Custom(format!("hfrog: S3 fetch: {}", e)))?;
    if !s3_resp.status().is_success() {
        return Err(AppError::Custom(format!(
            "hfrog: S3 fetch HTTP {}",
            s3_resp.status()
        )));
    }
    let bytes = s3_resp
        .bytes()
        .map_err(|e| AppError::Custom(format!("hfrog: S3 body: {}", e)))?;
    Ok(bytes.to_vec())
}

fn urlencode(s: &str) -> String {
    // Minimal percent-encode — hfrog query params are simple ASCII project
    // names and version strings. Escape spaces and the handful of reserved
    // chars that are likely to show up in user-chosen names.
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", b);
            }
        }
    }
    out
}

// ─── High-level orchestration: "mirror these artifacts to hfrog" ────────────
//
// The CLI calls `mirror_pack_artifacts` after a successful pack. The GUI
// calls `mirror_paths` after Save/Export. Both helpers swallow errors at the
// individual-file level — one failed file shouldn't abort the rest of the
// batch — and emit a single summary log line at the end.

/// Push every artifact produced by a `pack::execute` + `save_to_disk` cycle
/// to hfrog. No-op if the config is not active. Errors are logged but never
/// returned to the caller — the local pack is the source of truth.
pub fn mirror_pack_artifacts(
    cfg: &HfrogConfig,
    project_name: &str,
    ver: &str,
    output_dir: &Path,
    output_name: &str,
    is_multi_bin: bool,
    bin_count: usize,
) {
    let client = match Client::from_config(cfg) {
        Some(c) => c,
        None => return,
    };

    // Each pack run may produce multiple atlas variants when sprites overflow
    // a single bin. Mirror all of them — the s3_key includes the suffix so
    // hfrog stores them as distinct artifacts under the same project/version.
    let mut to_push: Vec<(String, &str)> = Vec::new();
    if is_multi_bin && bin_count > 1 {
        for i in 0..bin_count {
            let suffix = if i == 0 {
                String::new()
            } else {
                format!("_{}", i)
            };
            to_push.push((format!("{}{}.png", output_name, suffix), "atlas-png"));
            to_push.push((format!("{}{}.json", output_name, suffix), "atlas-json"));
        }
    } else {
        to_push.push((format!("{}.png", output_name), "atlas-png"));
        to_push.push((format!("{}.json", output_name), "atlas-json"));
    }
    // Manifest + log are per-pack, never per-bin.
    to_push.push((format!("{}.manifest.json", output_name), "manifest"));
    to_push.push((format!("{}.log", output_name), "log"));

    let mut ok = 0usize;
    let mut failed = 0usize;
    for (filename, kind) in to_push {
        let path = output_dir.join(&filename);
        if !path.is_file() {
            // Non-fatal: incremental cache hits skip the manifest write, the
            // log might land elsewhere on early failure, etc.
            log::debug!("hfrog: skip {} (not found)", path.display());
            continue;
        }
        match push_one(&client, cfg, project_name, kind, &filename, &path, ver) {
            Ok(()) => ok += 1,
            Err(e) => {
                failed += 1;
                log::error!("hfrog: upload failed for {}: {}", filename, e);
            }
        }
    }

    log::info!(
        "hfrog: mirror complete — {} ok, {} failed (project={}, ver={})",
        ok,
        failed,
        project_name,
        ver
    );
}

/// Mirror an arbitrary set of paths. Used by the GUI's Save/Export flow where
/// the file list is hand-picked rather than derived from a pack result.
pub fn mirror_paths(
    cfg: &HfrogConfig,
    project_name: &str,
    ver: &str,
    paths: &[(std::path::PathBuf, &str)],
) {
    let client = match Client::from_config(cfg) {
        Some(c) => c,
        None => return,
    };
    let mut ok = 0usize;
    let mut failed = 0usize;
    for (path, kind) in paths {
        if !path.is_file() {
            log::debug!("hfrog: skip {} (not found)", path.display());
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        match push_one(&client, cfg, project_name, kind, filename, path, ver) {
            Ok(()) => ok += 1,
            Err(e) => {
                failed += 1;
                log::error!("hfrog: upload failed for {}: {}", path.display(), e);
            }
        }
    }
    log::info!(
        "hfrog: mirror complete — {} ok, {} failed (project={}, ver={})",
        ok,
        failed,
        project_name,
        ver
    );
}

fn push_one(
    client: &Client,
    cfg: &HfrogConfig,
    project_name: &str,
    kind: &str,
    filename: &str,
    path: &Path,
    ver: &str,
) -> Result<()> {
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::Custom(format!("hfrog: read {}: {}", path.display(), e)))?;
    let spec = build_spec(cfg, project_name, kind, filename, &bytes, ver);
    client.upload_file(&spec, bytes, filename)
}

fn mime_for(filename: &str) -> &'static str {
    match Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "json" => "application/json",
        "tpproj" => "application/json", // .tpproj is JSON internally
        "log" | "txt" => "text/plain",
        "tres" => "text/plain",
        "tpsheet" => "application/json",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HfrogConfig;

    #[test]
    fn build_spec_stamps_expected_metadata() {
        let cfg = HfrogConfig {
            enabled: true,
            endpoint: "https://hfrog.test".into(),
            token: String::new(),
            default_runtime: "asset-pack".into(),
            s3_inc_id: 0,
        };
        let bytes = b"hello world";
        let spec = build_spec(&cfg, "myproj", "tpproj", "myproj.tpproj", bytes, "0.1.0");
        assert_eq!(spec.name, "myproj.tpproj");
        assert_eq!(spec.ver, "0.1.0");
        assert_eq!(spec.cont_size, bytes.len() as i64);
        // v0.5: file_kind "tpproj" routes to the project runtime, NOT the
        // legacy single `default_runtime`. Atlas / log / manifest go to
        // RUNTIME_ATLAS instead.
        assert_eq!(spec.runtime, RUNTIME_PROJECT);
        let png_spec = build_spec(&cfg, "myproj", "atlas-png", "myproj.png", b"x", "0.1.0");
        assert_eq!(png_spec.runtime, RUNTIME_ATLAS);
        assert_eq!(spec.s3_key, "mj_atlas/myproj/0.1.0/myproj.tpproj");
        assert_eq!(spec.s3_inc_id, 0);
        assert_eq!(spec.is_raw, Some(false));
        assert_eq!(spec.key_extension.as_deref(), Some("tpproj"));
        assert_eq!(spec.md5.len(), 32);
    }

    #[test]
    fn md5_is_stable_across_runs_for_same_content() {
        let cfg = HfrogConfig::default();
        let s1 = build_spec(&cfg, "p", "k", "p.bin", b"abc", "1");
        let s2 = build_spec(&cfg, "p", "k", "p.bin", b"abc", "1");
        assert_eq!(s1.md5, s2.md5, "md5 must be deterministic");

        let s3 = build_spec(&cfg, "p", "k", "p.bin", b"abd", "1");
        assert_ne!(s1.md5, s3.md5, "different content ⇒ different md5");
    }

    #[test]
    fn client_disabled_when_config_inactive() {
        // Default has a pre-populated endpoint; clear it to test the
        // independent gates one at a time.
        let mut cfg = HfrogConfig::default();
        cfg.endpoint.clear();
        assert!(Client::from_config(&cfg).is_none(), "disabled by default");

        cfg.enabled = true;
        assert!(
            Client::from_config(&cfg).is_none(),
            "enabled without endpoint must stay None"
        );

        cfg.endpoint = "https://hfrog.test".into();
        assert!(Client::from_config(&cfg).is_some());
    }

    #[test]
    fn mime_table_covers_atlas_outputs() {
        assert_eq!(mime_for("a.png"), "image/png");
        assert_eq!(mime_for("a.json"), "application/json");
        assert_eq!(mime_for("a.tpproj"), "application/json");
        assert_eq!(mime_for("a.tpsheet"), "application/json");
        assert_eq!(mime_for("a.tres"), "text/plain");
        assert_eq!(mime_for("a.log"), "text/plain");
        assert_eq!(mime_for("a.unknown"), "application/octet-stream");
    }
}
