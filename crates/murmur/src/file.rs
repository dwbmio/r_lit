//! File synchronization module for Murmur
//!
//! Provides high-level file operations with version control, audit trail,
//! conflict detection/locking, and CRDT-based synchronization.
//!
//! ## Features
//!
//! - **Version Control**: Every file update creates a new version
//! - **Audit Trail**: All operations are logged per-file with timestamp and node ID
//! - **Conflict Detection & Lock**: When a version mismatch is detected the file is
//!   locked across all nodes until the conflict originator resolves it.
//! - **History**: Access previous versions of files
//! - **CRDT Merge**: Automatic conflict resolution at the KV layer
//! - **Size Limit**: Files larger than MAX_FILE_SIZE will be rejected

use crate::{Result, Error, Swarm, StorageBackend};
use std::path::Path;
use tokio::fs;
use serde::{Serialize, Deserialize};

/// Maximum file size in bytes (10 MB)
pub const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

/// File metadata with version control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub name: String,
    pub size: usize,
    /// Last modified timestamp (Unix epoch seconds)
    pub modified: u64,
    pub checksum: String,
    /// Current version number (increments on each update)
    pub version: u64,
    /// Node ID that created this version
    pub author: String,
}

/// File version history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileVersion {
    pub version: u64,
    pub content_key: String,
    pub timestamp: u64,
    pub author: String,
    pub size: usize,
    pub operation: FileOperation,
}

/// File operation types for audit trail
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileOperation {
    Create,
    Update,
    Delete,
}

/// File operations extension for Swarm
pub trait FileOps {
    /// Store a file (auto-detects version; locks on conflict).
    async fn put_file(&self, file_path: &Path) -> Result<String>;

    /// Store a file with explicit version checking (optimistic locking).
    async fn put_file_with_version(
        &self,
        file_path: &Path,
        expected_version: Option<u64>,
    ) -> Result<String>;

    /// Retrieve a file (latest version).
    async fn get_file(&self, key: &str, output_path: &Path) -> Result<()>;

    /// Get a specific version of a file.
    async fn get_file_version(&self, key: &str, version: u64, output_path: &Path) -> Result<()>;

    /// List all files in the store.
    async fn list_files(&self) -> Result<Vec<FileMetadata>>;

    /// Delete a file.
    async fn delete_file(&self, key: &str) -> Result<()>;

    /// Get file metadata without downloading content.
    async fn file_metadata(&self, key: &str) -> Result<Option<FileMetadata>>;

    /// Get version history for a file.
    async fn file_history(&self, key: &str) -> Result<Vec<FileVersion>>;

    /// Get audit trail (all operations across all files).
    async fn audit_trail(&self, limit: Option<usize>) -> Result<Vec<FileVersion>>;
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl FileOps for Swarm {
    async fn put_file(&self, file_path: &Path) -> Result<String> {
        let file_name = file_path
            .file_name()
            .ok_or_else(|| Error::Other("Invalid file path".into()))?
            .to_string_lossy()
            .to_string();

        // Reject writes while the file is conflict-locked
        if self.is_file_locked(&file_name).await {
            return Err(Error::FileConflictLocked {
                file_name: file_name.clone(),
            });
        }

        // Read current version so we can detect concurrent modifications
        let content_key = format!("file:data:{}", file_name);
        let current_version = match self.file_metadata(&content_key).await? {
            Some(meta) => meta.version,
            None => 0,
        };

        match self.put_file_with_version(file_path, Some(current_version)).await {
            Ok(key) => Ok(key),
            Err(Error::VersionConflict { expected, current }) => {
                let my_id = self.node_id().await;
                self.lock_file_conflict(&file_name, &my_id, expected, current)
                    .await?;
                Err(Error::VersionConflict { expected, current })
            }
            Err(e) => Err(e),
        }
    }

    async fn put_file_with_version(
        &self,
        file_path: &Path,
        expected_version: Option<u64>,
    ) -> Result<String> {
        let content = fs::read(file_path).await.map_err(Error::Io)?;

        if content.len() > MAX_FILE_SIZE {
            return Err(Error::FileTooLarge {
                size: content.len(),
                max: MAX_FILE_SIZE,
            });
        }

        let file_name = file_path
            .file_name()
            .ok_or_else(|| Error::Other("Invalid file path".into()))?
            .to_string_lossy()
            .to_string();

        // Reject writes while the file is conflict-locked
        if self.is_file_locked(&file_name).await {
            return Err(Error::FileConflictLocked {
                file_name: file_name.clone(),
            });
        }

        let content_key = format!("file:data:{}", file_name);
        let meta_key = format!("file:meta:{}", file_name);

        let current_meta = if let Some(meta_bytes) = self.get(&meta_key).await? {
            Some(
                serde_json::from_slice::<FileMetadata>(&meta_bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
            )
        } else {
            None
        };

        // Version conflict check
        if let Some(expected) = expected_version {
            if let Some(ref meta) = current_meta {
                if meta.version != expected {
                    return Err(Error::VersionConflict {
                        expected,
                        current: meta.version,
                    });
                }
            } else if expected != 0 {
                return Err(Error::VersionConflict {
                    expected,
                    current: 0,
                });
            }
        }

        let (new_version, operation) = if let Some(ref meta) = current_meta {
            (meta.version + 1, FileOperation::Update)
        } else {
            (1, FileOperation::Create)
        };

        let author = self.node_id().await;
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let metadata = FileMetadata {
            name: file_name.clone(),
            size: content.len(),
            modified: now_ts,
            checksum: format!("{}", content.len()),
            version: new_version,
            author: author.clone(),
        };

        // Versioned snapshot (immutable)
        let versioned_content_key = format!("file:data:{}:v{}", file_name, new_version);
        self.put(&versioned_content_key, &content).await?;

        // Latest content (quick access)
        self.put(&content_key, &content).await?;

        // Metadata
        let meta_bytes =
            serde_json::to_vec(&metadata).map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&meta_key, &meta_bytes).await?;

        // Per-file history
        let version_entry = FileVersion {
            version: new_version,
            content_key: versioned_content_key,
            timestamp: now_ts,
            author: author.clone(),
            size: content.len(),
            operation,
        };

        let history_key = format!("file:history:{}", file_name);
        let mut history = self.file_history(&content_key).await.unwrap_or_default();
        history.push(version_entry.clone());
        let history_bytes =
            serde_json::to_vec(&history).map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&history_key, &history_bytes).await?;

        // Global audit trail (include version to guarantee uniqueness within the same second)
        let audit_key = format!("audit:{}:{}:{}:v{}", now_ts, author, file_name, new_version);
        let audit_bytes =
            serde_json::to_vec(&version_entry).map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&audit_key, &audit_bytes).await?;

        Ok(content_key)
    }

    async fn get_file(&self, key: &str, output_path: &Path) -> Result<()> {
        let content = self
            .get(key)
            .await?
            .ok_or_else(|| Error::Other("File not found".into()))?;
        fs::write(output_path, content).await.map_err(Error::Io)?;
        Ok(())
    }

    async fn get_file_version(&self, key: &str, version: u64, output_path: &Path) -> Result<()> {
        let file_name = key
            .strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;
        let versioned_key = format!("file:data:{}:v{}", file_name, version);
        let content = self
            .get(&versioned_key)
            .await?
            .ok_or_else(|| Error::Other(format!("Version {} not found", version)))?;
        fs::write(output_path, content).await.map_err(Error::Io)?;
        Ok(())
    }

    async fn list_files(&self) -> Result<Vec<FileMetadata>> {
        let meta_keys = self.inner.storage.keys_with_prefix("file:meta:")?;
        let mut files = Vec::with_capacity(meta_keys.len());
        for key in meta_keys {
            if let Some(meta_bytes) = self.get(&key).await? {
                if let Ok(meta) = serde_json::from_slice::<FileMetadata>(&meta_bytes) {
                    files.push(meta);
                }
            }
        }
        files.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(files)
    }

    async fn delete_file(&self, key: &str) -> Result<()> {
        let file_name = key
            .strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;

        if self.is_file_locked(file_name).await {
            return Err(Error::FileConflictLocked {
                file_name: file_name.to_string(),
            });
        }

        let meta_key = format!("file:meta:{}", file_name);
        let metadata = if let Some(meta_bytes) = self.get(&meta_key).await? {
            Some(
                serde_json::from_slice::<FileMetadata>(&meta_bytes)
                    .map_err(|e| Error::Serialization(e.to_string()))?,
            )
        } else {
            None
        };

        if let Some(meta) = metadata {
            let author = self.node_id().await;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let delete_entry = FileVersion {
                version: meta.version + 1,
                content_key: key.to_string(),
                timestamp,
                author: author.clone(),
                size: 0,
                operation: FileOperation::Delete,
            };

            let history_key = format!("file:history:{}", file_name);
            let mut history = self.file_history(key).await.unwrap_or_default();
            history.push(delete_entry.clone());
            let history_bytes =
                serde_json::to_vec(&history).map_err(|e| Error::Serialization(e.to_string()))?;
            self.put(&history_key, &history_bytes).await?;

            let audit_key = format!("audit:{}:{}:{}:v{}", timestamp, author, file_name, delete_entry.version);
            let audit_bytes = serde_json::to_vec(&delete_entry)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            self.put(&audit_key, &audit_bytes).await?;
        }

        self.delete(&meta_key).await?;
        self.delete(key).await?;
        Ok(())
    }

    async fn file_metadata(&self, key: &str) -> Result<Option<FileMetadata>> {
        let file_name = key
            .strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;
        let meta_key = format!("file:meta:{}", file_name);
        if let Some(meta_bytes) = self.get(&meta_key).await? {
            let metadata: FileMetadata = serde_json::from_slice(&meta_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(Some(metadata))
        } else {
            Ok(None)
        }
    }

    async fn file_history(&self, key: &str) -> Result<Vec<FileVersion>> {
        let file_name = key
            .strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;
        let history_key = format!("file:history:{}", file_name);
        if let Some(history_bytes) = self.get(&history_key).await? {
            let history: Vec<FileVersion> = serde_json::from_slice(&history_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            Ok(history)
        } else {
            Ok(Vec::new())
        }
    }

    async fn audit_trail(&self, limit: Option<usize>) -> Result<Vec<FileVersion>> {
        let audit_keys = self.inner.storage.keys_with_prefix("audit:")?;
        let cap = limit.unwrap_or(usize::MAX);

        let mut entries = Vec::new();
        // Keys are "audit:{ts}:{author}:{file}", sort descending by key (newest first)
        let mut sorted_keys = audit_keys;
        sorted_keys.sort_unstable_by(|a, b| b.cmp(a));

        for key in sorted_keys.into_iter().take(cap) {
            if let Some(bytes) = self.get(&key).await? {
                if let Ok(entry) = serde_json::from_slice::<FileVersion>(&bytes) {
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }
}
