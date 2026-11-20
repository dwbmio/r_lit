//! Bounded, offline inspection of build directories, ZIP, and TAR.GZ files.
//!
//! The report deliberately keeps each container's accounting independent. In
//! particular, a nested ZIP's uncompressed members are not added to its parent
//! container's totals. No archived content is executed or extracted to disk.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const TOOL_NAME: &str = "artifact-inspect";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

const DEFAULT_LIMITATIONS: &[&str] = &[
    "Raw bytes are uncompressed file sizes; ZIP compressed bytes are central-directory entry payload sizes, not HTTP/network transfer bytes.",
    "TAR.GZ has no reliable per-member compressed-byte field, so zip_compressed_bytes is null for TAR.GZ members and directory files.",
    "Container totals are independent; no cross-layer byte total is reported because nested content would otherwise be counted as parent storage and again as members.",
    "Godot and Unity proprietary formats are classified but their engine asset semantics and provenance are not parsed.",
    "Previews are bounded; HTML/JavaScript is escaped text, bitmap previews use inert data URIs, and sensitive configuration content is suppressed.",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Limits {
    pub max_entries: usize,
    pub max_total_raw_bytes: u64,
    pub max_nested_bytes: u64,
    pub max_depth: usize,
    pub max_preview_bytes: u64,
    pub max_total_preview_bytes: u64,
    pub max_image_pixels: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_total_raw_bytes: 2_000_000_000,
            max_nested_bytes: 64 * 1024 * 1024,
            max_depth: 4,
            max_preview_bytes: 64 * 1024,
            max_total_preview_bytes: 8 * 1024 * 1024,
            max_image_pixels: 16 * 1024 * 1024,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    #[error("input error: {0}")]
    Input(String),
    #[error("usage error: {0}")]
    Usage(String),
    #[error("cannot {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot read ZIP {path}: {source}")]
    Zip {
        path: String,
        #[source]
        source: zip::result::ZipError,
    },
    #[error("cannot render HTML report: {0}")]
    Format(#[from] std::fmt::Error),
    #[error("cannot serialize JSON report: {0}")]
    Json(#[from] serde_json::Error),
}

impl InspectError {
    pub fn is_input(&self) -> bool {
        match self {
            Self::Input(_) | Self::Zip { .. } => true,
            Self::Io { source, .. } => matches!(
                source.kind(),
                std::io::ErrorKind::NotFound
                    | std::io::ErrorKind::PermissionDenied
                    | std::io::ErrorKind::InvalidData
                    | std::io::ErrorKind::InvalidInput
                    | std::io::ErrorKind::UnexpectedEof
            ),
            _ => false,
        }
    }

    pub fn is_usage(&self) -> bool {
        matches!(self, Self::Usage(_))
    }
}

fn io_error(
    action: &'static str,
    path: impl Into<PathBuf>,
    source: std::io::Error,
) -> InspectError {
    InspectError::Io {
        action,
        path: path.into(),
        source,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Report {
    pub schema_version: u32,
    pub tool: String,
    pub tool_version: String,
    pub input: InputInfo,
    pub limits: Limits,
    pub containers: Vec<Container>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputInfo {
    pub path: String,
    pub kind: String,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub depth: usize,
    pub parent: Option<NestedParent>,
    pub files: Vec<FileEntry>,
    pub issues: Vec<Issue>,
    pub summary: Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NestedParent {
    pub container_id: String,
    pub relative_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub container_id: String,
    pub container: String,
    pub relative_path: String,
    pub kind: String,
    pub raw_bytes: u64,
    pub zip_compressed_bytes: Option<u64>,
    pub preview: Preview,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Preview {
    pub kind: String,
    pub limitation: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

impl Preview {
    fn unavailable(kind: &str, limitation: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            limitation: limitation.into(),
            truncated: false,
            media_type: None,
            text: None,
            data_uri: None,
            width: None,
            height: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Issue {
    pub kind: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Summary {
    pub file_count: usize,
    pub raw_bytes: u64,
    pub zip_compressed_bytes: Option<u64>,
    pub preview_count: usize,
    pub suppressed_preview_count: usize,
}

#[derive(Debug)]
struct PendingNested {
    parent_container_id: String,
    parent_path: String,
    relative_path: String,
    depth: usize,
    inherited_sensitive: bool,
    bytes: Vec<u8>,
}

struct Inspector {
    limits: Limits,
    containers: Vec<Container>,
    total_entries: usize,
    total_raw_bytes: u64,
    total_preview_bytes: u64,
}

/// Inspect an input without writing beside it or executing any archived code.
pub fn inspect_path(input: &Path, limits: &Limits) -> Result<Report, InspectError> {
    validate_limits(limits)?;
    let metadata = std::fs::symlink_metadata(input)
        .map_err(|e| io_error("read input metadata for", input, e))?;
    if metadata.file_type().is_symlink() {
        return Err(InspectError::Input(format!(
            "refusing to follow symlink input {}",
            input.display()
        )));
    }
    let canonical =
        std::fs::canonicalize(input).map_err(|e| io_error("resolve input", input, e))?;
    let input_kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        detect_outer_file_kind(input)?
    } else {
        return Err(InspectError::Input(format!(
            "unsupported special input {}",
            input.display()
        )));
    };

    let mut inspector = Inspector {
        limits: limits.clone(),
        containers: Vec::new(),
        total_entries: 0,
        total_raw_bytes: 0,
        total_preview_bytes: 0,
    };
    let source = canonical.display().to_string();
    let mut pending = VecDeque::new();
    match input_kind {
        "directory" => {
            pending.extend(inspector.inspect_directory(&canonical, &source)?);
        }
        "zip" => {
            let file = File::open(&canonical).map_err(|e| io_error("open ZIP", &canonical, e))?;
            pending.extend(inspector.inspect_zip(file, source.clone(), 0, None, false)?);
        }
        "tar.gz" => {
            let file =
                File::open(&canonical).map_err(|e| io_error("open TAR.GZ", &canonical, e))?;
            pending.extend(inspector.inspect_tar_gz(file, &source)?);
        }
        _ => {
            return Err(InspectError::Input(format!(
                "unsupported input {}; expected a directory, ZIP, TAR.GZ, or TGZ",
                input.display()
            )));
        }
    }

    while let Some(item) = pending.pop_front() {
        let checkpoint = inspector.containers.len();
        match inspector.inspect_zip(
            Cursor::new(item.bytes),
            format!("{}!{}", item.parent_path, item.relative_path),
            item.depth,
            Some(NestedParent {
                container_id: item.parent_container_id.clone(),
                relative_path: item.relative_path.clone(),
            }),
            item.inherited_sensitive,
        ) {
            Ok(children) => pending.extend(children),
            Err(error) => {
                inspector.containers.truncate(checkpoint);
                inspector.add_issue(
                    &item.parent_container_id,
                    Issue {
                        kind: "nested_zip_invalid".to_string(),
                        path: item.relative_path,
                        message: format!("nested ZIP was not inspected: {error}"),
                    },
                );
            }
        }
    }

    Ok(Report {
        schema_version: REPORT_SCHEMA_VERSION,
        tool: TOOL_NAME.to_string(),
        tool_version: TOOL_VERSION.to_string(),
        input: InputInfo {
            path: source,
            kind: input_kind.to_string(),
            bytes: if metadata.is_file() {
                Some(metadata.len())
            } else {
                None
            },
        },
        limits: limits.clone(),
        containers: inspector.containers,
        limitations: DEFAULT_LIMITATIONS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
    })
}

fn validate_limits(limits: &Limits) -> Result<(), InspectError> {
    if limits.max_entries == 0
        || limits.max_total_raw_bytes == 0
        || limits.max_total_preview_bytes == 0
        || limits.max_image_pixels == 0
    {
        return Err(InspectError::Usage(
            "entry, total-byte, total-preview, and image-pixel limits must be greater than zero"
                .to_string(),
        ));
    }
    Ok(())
}

fn detect_outer_file_kind(path: &Path) -> Result<&'static str, InspectError> {
    let mut file = File::open(path).map_err(|e| io_error("open input", path, e))?;
    let mut magic = [0_u8; 4];
    let count = file
        .read(&mut magic)
        .map_err(|e| io_error("read input signature from", path, e))?;
    if count >= 2 && magic[..2] == [0x1f, 0x8b] {
        return Ok("tar.gz");
    }
    if count >= 4 && matches!(&magic, b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08") {
        return Ok("zip");
    }
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".zip") {
        Ok("zip")
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Ok("tar.gz")
    } else {
        Err(InspectError::Input(format!(
            "unsupported input file {}",
            path.display()
        )))
    }
}

impl Inspector {
    fn next_container_id(&self) -> String {
        format!("c{}", self.containers.len())
    }

    fn inspect_directory(
        &mut self,
        root: &Path,
        source: &str,
    ) -> Result<Vec<PendingNested>, InspectError> {
        let id = self.next_container_id();
        let mut files = Vec::new();
        let mut issues = Vec::new();
        let mut pending = Vec::new();
        let mut visited = 0_usize;
        for item in WalkDir::new(root).follow_links(false).sort_by_file_name() {
            visited = visited.saturating_add(1);
            if visited > self.limits.max_entries {
                return Err(InspectError::Input(format!(
                    "entry limit {} exceeded while walking directory",
                    self.limits.max_entries
                )));
            }
            let entry = match item {
                Ok(entry) => entry,
                Err(error) => {
                    issues.push(Issue {
                        kind: "walk_error".to_string(),
                        path: root.display().to_string(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            if entry.depth() == 0 {
                continue;
            }
            let file_type = entry.file_type();
            let relative = safe_relative_path(entry.path().strip_prefix(root).map_err(|_| {
                InspectError::Input("directory walk escaped its input root".to_string())
            })?)?;
            if file_type.is_symlink() {
                issues.push(Issue {
                    kind: "symlink_skipped".to_string(),
                    path: relative,
                    message: "symbolic link was not followed".to_string(),
                });
                continue;
            }
            if file_type.is_dir() {
                continue;
            }
            if !file_type.is_file() {
                issues.push(Issue {
                    kind: "special_file_skipped".to_string(),
                    path: relative,
                    message: "only regular files are listed".to_string(),
                });
                continue;
            }
            let raw_bytes = entry
                .metadata()
                .map_err(|error| {
                    io_error(
                        "read file metadata for",
                        entry.path(),
                        std::io::Error::other(error),
                    )
                })?
                .len();
            let mut file =
                File::open(entry.path()).map_err(|e| io_error("open file", entry.path(), e))?;
            let (record, nested, mut file_issues, _) =
                self.process_file(&id, source, 0, relative, raw_bytes, None, false, &mut file)?;
            files.push(record);
            issues.append(&mut file_issues);
            pending.extend(nested);
        }
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        pending.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.push_container(id, source.to_string(), "directory", 0, None, files, issues)?;
        Ok(pending)
    }

    fn inspect_tar_gz<R: Read>(
        &mut self,
        reader: R,
        source: &str,
    ) -> Result<Vec<PendingNested>, InspectError> {
        let id = self.next_container_id();
        let decoder = GzDecoder::new(reader);
        let mut archive = tar::Archive::new(decoder);
        let entries = archive
            .entries()
            .map_err(|e| io_error("read TAR.GZ entries in", source, e))?;
        let mut files = Vec::new();
        let mut issues = Vec::new();
        let mut pending = Vec::new();
        for item in entries {
            let mut entry = item.map_err(|e| io_error("read TAR.GZ member in", source, e))?;
            let path = entry
                .path()
                .map_err(|e| io_error("read TAR.GZ member path in", source, e))?
                .into_owned();
            let entry_type = entry.header().entry_type();
            // Only canonical TAR root directory markers are not inventory items.
            // Match the original spelling: Path equality normalizes extra `.` parts.
            if entry_type.is_dir() && matches!(entry.path_bytes().as_ref(), b"." | b"./") {
                continue;
            }
            let relative = safe_relative_path(&path)?;
            if entry_type.is_dir() {
                continue;
            }
            if entry_type.is_symlink() || entry_type.is_hard_link() {
                issues.push(Issue {
                    kind: "link_skipped".to_string(),
                    path: relative,
                    message: "TAR link member was not followed".to_string(),
                });
                continue;
            }
            if !entry_type.is_file() {
                issues.push(Issue {
                    kind: "special_member_skipped".to_string(),
                    path: relative,
                    message: "only regular TAR members are listed".to_string(),
                });
                continue;
            }
            let raw_bytes = entry
                .header()
                .size()
                .map_err(|e| io_error("read TAR.GZ member size in", source, e))?;
            let (record, nested, mut file_issues, _) =
                self.process_file(&id, source, 0, relative, raw_bytes, None, false, &mut entry)?;
            files.push(record);
            issues.append(&mut file_issues);
            pending.extend(nested);
        }
        let mut decoder = archive.into_inner();
        drain_to_end(&mut decoder).map_err(|e| io_error("validate gzip trailer in", source, e))?;
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        pending.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.push_container(id, source.to_string(), "tar.gz", 0, None, files, issues)?;
        Ok(pending)
    }

    fn inspect_zip<R: Read + Seek>(
        &mut self,
        reader: R,
        source: String,
        depth: usize,
        parent: Option<NestedParent>,
        inherited_sensitive: bool,
    ) -> Result<Vec<PendingNested>, InspectError> {
        let mut archive =
            zip::ZipArchive::new(reader).map_err(|source_error| InspectError::Zip {
                path: source.clone(),
                source: source_error,
            })?;
        if archive.len() > self.limits.max_entries {
            return Err(InspectError::Input(format!(
                "entry limit {} exceeded by ZIP {}",
                self.limits.max_entries, source
            )));
        }
        let id = self.next_container_id();
        let mut files = Vec::new();
        let mut issues = Vec::new();
        let mut pending = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|source_error| InspectError::Zip {
                    path: source.clone(),
                    source: source_error,
                })?;
            let enclosed = entry.enclosed_name().ok_or_else(|| {
                InspectError::Input(format!("unsafe ZIP member path in {source}"))
            })?;
            let relative = safe_relative_path(&enclosed)?;
            if entry.is_dir() {
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                issues.push(Issue {
                    kind: "symlink_skipped".to_string(),
                    path: relative,
                    message: "ZIP symbolic link was not followed".to_string(),
                });
                continue;
            }
            let raw_bytes = entry.size();
            let compressed = Some(entry.compressed_size());
            let (record, nested, mut file_issues, consumed) = self.process_file(
                &id,
                &source,
                depth,
                relative,
                raw_bytes,
                compressed,
                inherited_sensitive,
                &mut entry,
            )?;
            if consumed > raw_bytes {
                return Err(InspectError::Input(format!(
                    "ZIP member size mismatch in {source}: read {consumed} bytes, declared {raw_bytes}"
                )));
            }
            drain_zip_remainder(&mut entry, raw_bytes - consumed, &source)?;
            files.push(record);
            issues.append(&mut file_issues);
            pending.extend(nested);
        }
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        pending.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        self.push_container(id, source, "zip", depth, parent, files, issues)?;
        Ok(pending)
    }

    #[allow(clippy::too_many_arguments)]
    fn process_file<R: Read>(
        &mut self,
        container_id: &str,
        container_path: &str,
        depth: usize,
        relative_path: String,
        raw_bytes: u64,
        zip_compressed_bytes: Option<u64>,
        inherited_sensitive: bool,
        reader: &mut R,
    ) -> Result<(FileEntry, Option<PendingNested>, Vec<Issue>, u64), InspectError> {
        self.total_entries = self
            .total_entries
            .checked_add(1)
            .ok_or_else(|| InspectError::Input("entry counter overflow".to_string()))?;
        if self.total_entries > self.limits.max_entries {
            return Err(InspectError::Input(format!(
                "entry limit {} exceeded",
                self.limits.max_entries
            )));
        }
        self.total_raw_bytes = self
            .total_raw_bytes
            .checked_add(raw_bytes)
            .ok_or_else(|| InspectError::Input("raw-byte counter overflow".to_string()))?;
        if self.total_raw_bytes > self.limits.max_total_raw_bytes {
            return Err(InspectError::Input(format!(
                "total raw-byte limit {} exceeded",
                self.limits.max_total_raw_bytes
            )));
        }

        let sensitive = inherited_sensitive || is_sensitive_path(&relative_path);
        let preview_cap = if sensitive {
            4
        } else {
            self.limits.max_preview_bytes.saturating_add(1).max(4)
        };
        let prefix = read_at_most(&mut *reader, preview_cap)
            .map_err(|e| io_error("read file bytes from", &relative_path, e))?;
        let mut bytes_consumed = prefix.len() as u64;
        let kind = classify_file(&relative_path, &prefix);
        let mut issues = Vec::new();
        let looks_nested = is_zip_name(&relative_path) || is_zip_magic(&prefix);
        let nested = if looks_nested {
            if depth >= self.limits.max_depth {
                issues.push(Issue {
                    kind: "nested_depth_skipped".to_string(),
                    path: relative_path.clone(),
                    message: format!("nested ZIP depth limit {} reached", self.limits.max_depth),
                });
                None
            } else if raw_bytes > self.limits.max_nested_bytes {
                issues.push(Issue {
                    kind: "nested_size_skipped".to_string(),
                    path: relative_path.clone(),
                    message: format!(
                        "nested ZIP is {raw_bytes} bytes, above max_nested_bytes={}",
                        self.limits.max_nested_bytes
                    ),
                });
                None
            } else {
                let chained = Cursor::new(prefix.clone()).chain(reader);
                let bytes = read_at_most(chained, self.limits.max_nested_bytes.saturating_add(1))
                    .map_err(|e| io_error("read nested ZIP", &relative_path, e))?;
                bytes_consumed = bytes.len() as u64;
                if bytes.len() as u64 > self.limits.max_nested_bytes {
                    issues.push(Issue {
                        kind: "nested_size_skipped".to_string(),
                        path: relative_path.clone(),
                        message: "nested ZIP exceeded its configured byte limit while reading"
                            .to_string(),
                    });
                    None
                } else {
                    if bytes.len() as u64 != raw_bytes {
                        issues.push(Issue {
                            kind: "size_mismatch".to_string(),
                            path: relative_path.clone(),
                            message: format!(
                                "declared {raw_bytes} bytes but read {}; nested inspection skipped",
                                bytes.len()
                            ),
                        });
                        None
                    } else {
                        Some(PendingNested {
                            parent_container_id: container_id.to_string(),
                            parent_path: container_path.to_string(),
                            relative_path: relative_path.clone(),
                            depth: depth + 1,
                            inherited_sensitive: sensitive,
                            bytes,
                        })
                    }
                }
            }
        } else {
            None
        };

        let preview = self.make_preview(&kind, raw_bytes, &prefix, sensitive);
        Ok((
            FileEntry {
                container_id: container_id.to_string(),
                container: container_path.to_string(),
                relative_path,
                kind,
                raw_bytes,
                zip_compressed_bytes,
                preview,
            },
            nested,
            issues,
            bytes_consumed,
        ))
    }

    fn make_preview(
        &mut self,
        file_kind: &str,
        raw_bytes: u64,
        prefix: &[u8],
        sensitive: bool,
    ) -> Preview {
        if sensitive {
            return Preview::unavailable(
                "suppressed_sensitive",
                "sensitive-looking configuration/key content is not previewed; path and byte size remain listed",
            );
        }
        let proprietary = proprietary_limitation(file_kind);
        if let Some((media_type, format)) = bitmap_type(file_kind) {
            if raw_bytes > self.limits.max_preview_bytes {
                return Preview::unavailable(
                    "skipped_size",
                    format!(
                        "bitmap exceeds max_preview_bytes={}",
                        self.limits.max_preview_bytes
                    ),
                );
            }
            if !self.reserve_preview(raw_bytes) {
                return Preview::unavailable(
                    "skipped_budget",
                    "total preview-byte budget exhausted",
                );
            }
            let bytes = &prefix[..prefix.len().min(raw_bytes as usize)];
            match validate_bitmap(bytes, self.limits.max_image_pixels) {
                Ok((width, height)) => Preview {
                    kind: "image".to_string(),
                    limitation: format!(
                        "validated {width}x{height} {format}; embedded as an inert data URI, never executed"
                    ),
                    truncated: false,
                    media_type: Some(media_type.to_string()),
                    text: None,
                    data_uri: Some(format!("data:{media_type};base64,{}", BASE64.encode(bytes))),
                    width: Some(width),
                    height: Some(height),
                },
                Err(message) => Preview::unavailable("invalid_image", message),
            }
        } else if is_text_kind(file_kind) {
            if raw_bytes > self.limits.max_preview_bytes && self.limits.max_preview_bytes == 0 {
                return Preview::unavailable("skipped_size", "text previews are disabled");
            }
            let take = prefix.len().min(self.limits.max_preview_bytes as usize);
            let bytes = &prefix[..take];
            let mut valid_len = bytes.len();
            while std::str::from_utf8(&bytes[..valid_len]).is_err() && valid_len > 0 {
                valid_len -= 1;
            }
            if valid_len == 0 && !bytes.is_empty() {
                return Preview::unavailable("none", "not valid bounded UTF-8 text");
            }
            let text = match std::str::from_utf8(&bytes[..valid_len]) {
                Ok(text) if !text.contains('\0') => text,
                _ => return Preview::unavailable("none", "not valid bounded UTF-8 text"),
            };
            if !self.reserve_preview(valid_len as u64) {
                return Preview::unavailable(
                    "skipped_budget",
                    "total preview-byte budget exhausted",
                );
            }
            let active = matches!(file_kind, "html_text" | "javascript" | "svg_text");
            let limitation = if let Some(note) = proprietary {
                format!("bounded escaped text only; {note}")
            } else if active {
                "bounded escaped text only; active HTML/JavaScript/SVG is never executed"
                    .to_string()
            } else {
                "bounded escaped UTF-8 text only".to_string()
            };
            Preview {
                kind: "text".to_string(),
                limitation,
                truncated: raw_bytes > valid_len as u64,
                media_type: None,
                text: Some(text.to_string()),
                data_uri: None,
                width: None,
                height: None,
            }
        } else {
            Preview::unavailable(
                "none",
                proprietary.unwrap_or("no safe preview for this binary or unknown format"),
            )
        }
    }

    fn reserve_preview(&mut self, bytes: u64) -> bool {
        let Some(next) = self.total_preview_bytes.checked_add(bytes) else {
            return false;
        };
        if next > self.limits.max_total_preview_bytes {
            return false;
        }
        self.total_preview_bytes = next;
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn push_container(
        &mut self,
        id: String,
        path: String,
        kind: &str,
        depth: usize,
        parent: Option<NestedParent>,
        files: Vec<FileEntry>,
        issues: Vec<Issue>,
    ) -> Result<(), InspectError> {
        let raw_bytes = files
            .iter()
            .try_fold(0_u64, |total, file| total.checked_add(file.raw_bytes))
            .ok_or_else(|| InspectError::Input("container raw-byte total overflow".to_string()))?;
        let zip_compressed_bytes = if kind == "zip" {
            Some(
                files
                    .iter()
                    .try_fold(0_u64, |total, file| {
                        total.checked_add(file.zip_compressed_bytes.unwrap_or(0))
                    })
                    .ok_or_else(|| {
                        InspectError::Input("ZIP compressed-byte total overflow".to_string())
                    })?,
            )
        } else {
            None
        };
        let summary = Summary {
            file_count: files.len(),
            raw_bytes,
            zip_compressed_bytes,
            preview_count: files
                .iter()
                .filter(|f| matches!(f.preview.kind.as_str(), "text" | "image"))
                .count(),
            suppressed_preview_count: files
                .iter()
                .filter(|f| f.preview.kind == "suppressed_sensitive")
                .count(),
        };
        log::info!(
            "inspected {}: {} files, {} raw bytes, {} issue(s)",
            path,
            summary.file_count,
            summary.raw_bytes,
            issues.len()
        );
        self.containers.push(Container {
            id,
            path,
            kind: kind.to_string(),
            depth,
            parent,
            files,
            issues,
            summary,
        });
        Ok(())
    }

    fn add_issue(&mut self, container_id: &str, issue: Issue) {
        log::warn!("{}: {}", issue.kind, issue.message);
        if let Some(container) = self.containers.iter_mut().find(|c| c.id == container_id) {
            container.issues.push(issue);
        }
    }
}

fn read_at_most<R: Read>(reader: R, limit: u64) -> std::io::Result<Vec<u8>> {
    let allocation = limit.min(1024 * 1024) as usize;
    let mut bytes = Vec::with_capacity(allocation);
    reader.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn drain_to_end<R: Read>(reader: &mut R) -> std::io::Result<()> {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
    }
}

fn drain_zip_remainder<R: Read>(
    reader: &mut R,
    expected: u64,
    source: &str,
) -> Result<(), InspectError> {
    let limit = expected
        .checked_add(1)
        .ok_or_else(|| InspectError::Input(format!("ZIP member size overflow in {source}")))?;
    let mut limited = reader.take(limit);
    let mut buffer = [0_u8; 16 * 1024];
    let mut actual = 0_u64;
    loop {
        let count = limited
            .read(&mut buffer)
            .map_err(|e| io_error("validate ZIP member in", source, e))?;
        if count == 0 {
            break;
        }
        actual = actual
            .checked_add(count as u64)
            .ok_or_else(|| InspectError::Input(format!("ZIP member size overflow in {source}")))?;
    }
    if actual != expected {
        return Err(InspectError::Input(format!(
            "ZIP member size mismatch in {source}: expected {expected} remaining bytes, read {actual}"
        )));
    }
    Ok(())
}

fn safe_relative_path(path: &Path) -> Result<String, InspectError> {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains('\0') {
        return Err(InspectError::Input(format!(
            "unsafe absolute/NUL archive path {}",
            path.display()
        )));
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(InspectError::Input(format!(
                    "unsafe parent traversal in archive path {}",
                    path.display()
                )));
            }
            value if parts.is_empty() && value.ends_with(':') => {
                return Err(InspectError::Input(format!(
                    "unsafe drive-prefixed archive path {}",
                    path.display()
                )));
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return Err(InspectError::Input("empty archive path".to_string()));
    }
    Ok(parts.join("/"))
}

/// Validate and resolve an output directory without creating it inside an input directory.
pub fn validate_output_dir(input: &Path, output: &Path) -> Result<PathBuf, InspectError> {
    let input_meta = std::fs::symlink_metadata(input)
        .map_err(|e| io_error("read input metadata for", input, e))?;
    if input_meta.file_type().is_symlink() {
        return Err(InspectError::Usage(
            "input must not be a symlink".to_string(),
        ));
    }
    let resolved_output = resolve_output_candidate(output)?;
    if resolved_output.exists() && !resolved_output.is_dir() {
        return Err(InspectError::Usage(format!(
            "output path {} is not a directory",
            resolved_output.display()
        )));
    }
    if input_meta.is_dir() {
        let canonical_input = std::fs::canonicalize(input)
            .map_err(|e| io_error("resolve input directory", input, e))?;
        if resolved_output == canonical_input || resolved_output.starts_with(&canonical_input) {
            return Err(InspectError::Usage(format!(
                "output directory {} must not be the input directory or its descendant",
                resolved_output.display()
            )));
        }
        validate_output_targets(&resolved_output, None)?;
    } else {
        let canonical_input =
            std::fs::canonicalize(input).map_err(|e| io_error("resolve input file", input, e))?;
        validate_output_targets(&resolved_output, Some(&canonical_input))?;
    }
    Ok(resolved_output)
}

fn validate_output_targets(
    output_dir: &Path,
    canonical_input: Option<&Path>,
) -> Result<(), InspectError> {
    for name in [
        "artifact-inspect.json",
        "artifact-inspect.html",
        "artifact-inspect.log",
    ] {
        validate_output_target(&output_dir.join(name), canonical_input)?;
    }
    Ok(())
}

fn validate_output_target(path: &Path, canonical_input: Option<&Path>) -> Result<(), InspectError> {
    if canonical_input.is_some_and(|input| input == path) {
        return Err(InspectError::Usage(format!(
            "output target {} aliases the input file",
            path.display()
        )));
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error("read output target metadata for", path, error)),
    };
    if metadata.file_type().is_symlink() {
        return Err(InspectError::Usage(format!(
            "refusing symlink output target {}",
            path.display()
        )));
    }
    if !metadata.is_file() {
        return Err(InspectError::Usage(format!(
            "output target {} is not a regular file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if metadata.nlink() > 1 {
            return Err(InspectError::Usage(format!(
                "refusing hard-linked output target {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn resolve_output_candidate(output: &Path) -> Result<PathBuf, InspectError> {
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| io_error("resolve current directory", output, e))?
            .join(output)
    };
    if let Ok(metadata) = std::fs::symlink_metadata(&absolute) {
        if metadata.file_type().is_symlink() {
            return Err(InspectError::Usage(format!(
                "refusing symlink output directory {}",
                absolute.display()
            )));
        }
        return std::fs::canonicalize(&absolute)
            .map_err(|e| io_error("resolve output", &absolute, e));
    }

    let mut existing = absolute.as_path();
    let mut suffix = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            InspectError::Usage(format!("cannot resolve output path {}", absolute.display()))
        })?;
        suffix.push(name.to_os_string());
        existing = existing.parent().ok_or_else(|| {
            InspectError::Usage(format!("cannot resolve output path {}", absolute.display()))
        })?;
    }
    let mut resolved = std::fs::canonicalize(existing)
        .map_err(|e| io_error("resolve existing output ancestor", existing, e))?;
    for component in suffix.iter().rev() {
        if component == ".." || component == "." {
            return Err(InspectError::Usage(
                "output path must not contain parent/current-directory components".to_string(),
            ));
        }
        resolved.push(component);
    }
    Ok(resolved)
}

/// Write the full versioned JSON report and its standalone offline HTML view.
/// Existing ordinary reports are atomically replaced; links are never followed.
pub fn write_reports(
    report: &Report,
    output_dir: &Path,
) -> Result<(PathBuf, PathBuf), InspectError> {
    std::fs::create_dir_all(output_dir)
        .map_err(|e| io_error("create output directory", output_dir, e))?;
    validate_output_targets(output_dir, None)?;
    let json_path = output_dir.join("artifact-inspect.json");
    let html_path = output_dir.join("artifact-inspect.html");
    let mut json = serde_json::to_vec_pretty(report)?;
    json.push(b'\n');
    write_output_atomically(&json_path, &json)?;
    let html = render_html(report)?;
    write_output_atomically(&html_path, html.as_bytes())?;
    Ok((json_path, html_path))
}

/// Flush the shared run log through a same-directory temporary file and rename.
/// This preserves the normal overwrite behavior without following a destination link.
pub fn flush_runlog(path: &Path, header: &[String]) -> Result<(), InspectError> {
    validate_output_target(path, None)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| io_error("create run log directory", parent, e))?;
    }
    let (temporary, file) = create_temporary_output(path)?;
    drop(file);
    rlit_cli::runlog::flush(&temporary, header);
    let metadata = std::fs::symlink_metadata(&temporary)
        .map_err(|e| io_error("read temporary run log", &temporary, e))?;
    if !metadata.is_file() || metadata.len() == 0 {
        let _ = std::fs::remove_file(&temporary);
        return Err(InspectError::Io {
            action: "flush run log to",
            path: temporary,
            source: std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "shared runlog did not produce a regular non-empty log",
            ),
        });
    }
    replace_temporary_output(temporary, path)
}

fn write_output_atomically(path: &Path, bytes: &[u8]) -> Result<(), InspectError> {
    validate_output_target(path, None)?;
    let (temporary, mut file) = create_temporary_output(path)?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.flush()) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(io_error("write temporary output for", path, error));
    }
    drop(file);
    replace_temporary_output(temporary, path)
}

fn create_temporary_output(path: &Path) -> Result<(PathBuf, File), InspectError> {
    let parent = path.parent().ok_or_else(|| {
        InspectError::Usage(format!(
            "output target {} has no parent directory",
            path.display()
        ))
    })?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "artifact-inspect-output".into());
    for _ in 0..100 {
        let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(io_error("create temporary output for", path, error)),
        }
    }
    Err(InspectError::Usage(format!(
        "could not allocate a unique temporary output for {}",
        path.display()
    )))
}

fn replace_temporary_output(temporary: PathBuf, path: &Path) -> Result<(), InspectError> {
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        return Err(io_error("atomically replace output", path, error));
    }
    Ok(())
}

/// Render a self-contained report without scripts, network references, or raw active content.
pub fn render_html(report: &Report) -> Result<String, InspectError> {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\n<title>artifact-inspect report</title>\n");
    out.push_str("<style>body{font:14px/1.45 ui-monospace,SFMono-Regular,Consolas,monospace;margin:24px;color:#20242a;background:#fff}h1,h2{font-family:system-ui,sans-serif}table{border-collapse:collapse;width:100%;table-layout:fixed}th,td{border:1px solid #ccd2d8;padding:6px;vertical-align:top;overflow-wrap:anywhere}th{background:#edf1f5;text-align:left}.num{text-align:right;white-space:nowrap}.issue{color:#8a3b00}.muted{color:#56616d}pre{white-space:pre-wrap;overflow-wrap:anywhere;max-height:28em;overflow:auto;background:#f5f7f9;padding:8px}img{max-width:min(100%,640px);max-height:480px;image-rendering:auto}details{margin-top:4px}.container{margin-bottom:32px}</style></head><body>\n");
    writeln!(
        out,
        "<h1>artifact-inspect report</h1><p>schema v{} · tool {}</p>",
        report.schema_version,
        escape(&report.tool_version)
    )?;
    writeln!(
        out,
        "<p><strong>Input:</strong> {} ({})</p>",
        escape(&report.input.path),
        escape(&report.input.kind)
    )?;
    out.push_str("<p class=\"muted\">All sizes are bytes. ZIP compressed sizes are entry payload sizes, not network transfer sizes. Layers are never added together.</p>\n");
    for container in &report.containers {
        out.push_str("<section class=\"container\">");
        writeln!(
            out,
            "<h2>{} · {}</h2>",
            escape(&container.id),
            escape(&container.kind)
        )?;
        writeln!(
            out,
            "<p><strong>Container:</strong> {}<br><strong>Files:</strong> {} · <strong>Raw:</strong> {} · <strong>ZIP compressed:</strong> {}</p>",
            escape(&container.path),
            container.summary.file_count,
            container.summary.raw_bytes,
            container
                .summary
                .zip_compressed_bytes
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string())
        )?;
        if !container.issues.is_empty() {
            out.push_str("<ul>");
            for issue in &container.issues {
                writeln!(
                    out,
                    "<li class=\"issue\">{} · {} · {}</li>",
                    escape(&issue.kind),
                    escape(&issue.path),
                    escape(&issue.message)
                )?;
            }
            out.push_str("</ul>");
        }
        out.push_str("<table><thead><tr><th style=\"width:22%\">Relative path</th><th style=\"width:12%\">Type</th><th style=\"width:9%\">Raw B</th><th style=\"width:12%\">ZIP compressed B</th><th>Preview / limitation</th></tr></thead><tbody>\n");
        for file in &container.files {
            writeln!(
                out,
                "<tr><td>{}</td><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td>",
                escape(&file.relative_path),
                escape(&file.kind),
                file.raw_bytes,
                file.zip_compressed_bytes
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "null".to_string())
            )?;
            writeln!(
                out,
                "<span class=\"muted\">{}</span><br>{}",
                escape(&file.preview.kind),
                escape(&file.preview.limitation)
            )?;
            if let Some(uri) = &file.preview.data_uri {
                writeln!(
                    out,
                    "<details><summary>Bitmap preview</summary><img alt=\"{}\" src=\"{}\"></details>",
                    escape(&file.relative_path),
                    uri
                )?;
            } else if let Some(text) = &file.preview.text {
                writeln!(
                    out,
                    "<details><summary>Escaped text preview{}</summary><pre>{}</pre></details>",
                    if file.preview.truncated {
                        " (truncated)"
                    } else {
                        ""
                    },
                    escape(text)
                )?;
            }
            out.push_str("</td></tr>\n");
        }
        out.push_str("</tbody></table></section>\n");
    }
    out.push_str("<h2>Limitations</h2><ul>");
    for limitation in &report.limitations {
        writeln!(out, "<li>{}</li>", escape(limitation))?;
    }
    out.push_str("</ul></body></html>\n");
    Ok(out)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn is_zip_name(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".zip")
}

fn is_zip_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && matches!(&bytes[..4], b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08")
}

fn is_sensitive_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let file_name = lower.rsplit('/').next().unwrap_or(&lower);
    if file_name.ends_with(".env")
        || file_name.starts_with(".env.")
        || file_name == ".envrc"
        || [
            ".netrc",
            ".npmrc",
            ".pypirc",
            "id_rsa",
            "id_dsa",
            "id_ecdsa",
            "id_ed25519",
        ]
        .contains(&file_name)
    {
        return true;
    }
    if ["pem", "key", "p12", "pfx", "kdbx", "keystore", "jks"]
        .iter()
        .any(|extension| file_name.ends_with(&format!(".{extension}")))
    {
        return true;
    }
    lower.split(['/', '.', '_', '-']).any(|part| {
        matches!(
            part,
            "private" | "secret" | "secrets" | "credential" | "credentials" | "token" | "tokens"
        )
    })
}

fn classify_file(path: &str, bytes: &[u8]) -> String {
    let lower = path.to_ascii_lowercase();
    let extension = lower.rsplit('.').next().unwrap_or("");
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "png".to_string();
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return "jpeg".to_string();
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return "gif".to_string();
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "webp".to_string();
    }
    let kind = match extension {
        "pck" => "godot_package",
        "res" | "scn" => "godot_binary_resource",
        "tres" | "tscn" | "godot" | "import" => "godot_text_resource",
        "unity" => "unity_scene",
        "prefab" => "unity_prefab",
        "asset" | "assets" | "bundle" | "unitypackage" => "unity_asset",
        "html" | "htm" | "xhtml" => "html_text",
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => "javascript",
        "svg" => "svg_text",
        "json" | "json5" => "json",
        "txt" | "md" | "csv" | "tsv" | "xml" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf"
        | "log" | "gd" | "gdshader" | "css" | "sh" | "py" | "rs" | "java" | "c" | "h" | "cpp"
        | "hpp" => "text",
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "gif" => "gif",
        "webp" => "webp",
        "wasm" => "webassembly_binary",
        "zip" => "zip_archive",
        "gz" => "gzip_binary",
        "br" => "brotli_binary",
        _ => {
            if !bytes.is_empty() && !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
                "text"
            } else {
                "binary"
            }
        }
    };
    kind.to_string()
}

fn bitmap_type(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "png" => Some(("image/png", "PNG")),
        "jpeg" => Some(("image/jpeg", "JPEG")),
        "gif" => Some(("image/gif", "GIF")),
        "webp" => Some(("image/webp", "WebP")),
        _ => None,
    }
}

fn is_text_kind(kind: &str) -> bool {
    matches!(
        kind,
        "text"
            | "html_text"
            | "javascript"
            | "svg_text"
            | "json"
            | "godot_text_resource"
            | "unity_scene"
            | "unity_prefab"
            | "unity_asset"
    )
}

fn proprietary_limitation(kind: &str) -> Option<&'static str> {
    match kind {
        "godot_package" | "godot_binary_resource" | "godot_text_resource" => {
            Some("Godot resource/package semantics and asset provenance are not parsed")
        }
        "unity_scene" | "unity_prefab" | "unity_asset" => {
            Some("Unity resource semantics and asset provenance are not parsed")
        }
        _ => None,
    }
}

fn validate_bitmap(bytes: &[u8], max_pixels: u64) -> Result<(u32, u32), String> {
    use image::ImageReader;
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("image format detection failed: {e}"))?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| format!("image header validation failed: {e}"))?;
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > max_pixels {
        return Err(format!(
            "image has {pixels} pixels, above max_image_pixels={max_pixels}"
        ));
    }
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("image format detection failed: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(pixels.saturating_mul(4).max(1024));
    decoder.limits(limits);
    decoder
        .decode()
        .map_err(|e| format!("image decode validation failed: {e}"))?;
    Ok((width, height))
}

/// Return a normalized relative path for callers constructing their own inputs.
/// This helper intentionally accepts only ordinary components.
pub fn normalize_relative_path(path: &Path) -> Result<String, InspectError> {
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(InspectError::Input(format!(
            "unsafe path {}",
            path.display()
        )));
    }
    safe_relative_path(path)
}
