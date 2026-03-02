//! File synchronization module for Murmur
//!
//! Provides high-level file operations with version control, audit trail, and CRDT-based conflict resolution.
//!
//! ## Features
//!
//! - **Version Control**: Every file update creates a new version
//! - **Audit Trail**: All operations are logged with timestamp and node ID
//! - **Conflict Detection**: Optimistic locking with version checking
//! - **History**: Access previous versions of files
//! - **CRDT Merge**: Automatic conflict resolution at the KV layer
//! - **Size Limit**: Files larger than MAX_FILE_SIZE will be rejected

use crate::{Result, Error, Swarm};
use std::path::Path;
use tokio::fs;
use serde::{Serialize, Deserialize};

/// Maximum file size in bytes (10 MB)
/// Files larger than this will be rejected with FileTooLarge error
pub const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

/// File metadata with version control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    /// Original file name
    pub name: String,
    /// File size in bytes
    pub size: usize,
    /// Last modified timestamp (Unix epoch)
    pub modified: u64,
    /// Simple checksum (file size as string)
    pub checksum: String,
    /// Current version number (increments on each update)
    pub version: u64,
    /// Node ID that created this version
    pub author: String,
}

/// File version history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileVersion {
    /// Version number
    pub version: u64,
    /// Content hash or key
    pub content_key: String,
    /// Timestamp
    pub timestamp: u64,
    /// Author node ID
    pub author: String,
    /// File size
    pub size: usize,
    /// Operation type
    pub operation: FileOperation,
}

/// File operation types for audit trail
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileOperation {
    /// File created
    Create,
    /// File updated
    Update,
    /// File deleted
    Delete,
}

/// Conflict information when version mismatch occurs
#[derive(Debug, Clone)]
pub struct VersionConflict {
    /// Expected version
    pub expected: u64,
    /// Current version in store
    pub current: u64,
    /// Current file metadata
    pub current_metadata: FileMetadata,
}

/// File operations extension for Swarm
pub trait FileOps {
    /// Store a file in the distributed store (auto-increment version)
    async fn put_file(&self, file_path: &Path) -> Result<String>;

    /// Store a file with version checking (optimistic locking)
    async fn put_file_with_version(
        &self,
        file_path: &Path,
        expected_version: Option<u64>
    ) -> Result<String>;

    /// Retrieve a file from the distributed store (latest version)
    async fn get_file(&self, key: &str, output_path: &Path) -> Result<()>;

    /// Get a specific version of a file
    async fn get_file_version(&self, key: &str, version: u64, output_path: &Path) -> Result<()>;

    /// List all files in the store
    async fn list_files(&self) -> Result<Vec<FileMetadata>>;

    /// Delete a file from the store
    async fn delete_file(&self, key: &str) -> Result<()>;

    /// Get file metadata without downloading content
    async fn file_metadata(&self, key: &str) -> Result<Option<FileMetadata>>;

    /// Get version history for a file
    async fn file_history(&self, key: &str) -> Result<Vec<FileVersion>>;

    /// Get audit trail (all operations across all files)
    async fn audit_trail(&self, limit: Option<usize>) -> Result<Vec<FileVersion>>;
}

impl FileOps for Swarm {
    async fn put_file(&self, file_path: &Path) -> Result<String> {
        self.put_file_with_version(file_path, None).await
    }

    async fn put_file_with_version(
        &self,
        file_path: &Path,
        expected_version: Option<u64>
    ) -> Result<String> {
        // Read file content
        let content = fs::read(file_path).await
            .map_err(|e| Error::Io(e))?;

        // Check file size limit
        if content.len() > MAX_FILE_SIZE {
            return Err(Error::FileTooLarge {
                size: content.len(),
                max: MAX_FILE_SIZE,
            });
        }

        // Get file name
        let file_name = file_path.file_name()
            .ok_or_else(|| Error::Other("Invalid file path".into()))?
            .to_string_lossy()
            .to_string();

        let content_key = format!("file:data:{}", file_name);
        let meta_key = format!("file:meta:{}", file_name);

        // Get current metadata (if exists)
        let current_meta = if let Some(meta_bytes) = self.get(&meta_key).await? {
            Some(serde_json::from_slice::<FileMetadata>(&meta_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?)
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

        // Determine new version and operation
        let (new_version, operation) = if let Some(ref meta) = current_meta {
            (meta.version + 1, FileOperation::Update)
        } else {
            (1, FileOperation::Create)
        };

        // Get node ID for author
        let author = self.node_id().await;

        // Create new metadata
        let metadata = FileMetadata {
            name: file_name.clone(),
            size: content.len(),
            modified: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            checksum: format!("{}", content.len()),
            version: new_version,
            author: author.clone(),
        };

        // Store content with version
        let versioned_content_key = format!("file:data:{}:v{}", file_name, new_version);
        self.put(&versioned_content_key, &content).await?;

        // Store latest content (for quick access)
        self.put(&content_key, &content).await?;

        // Store metadata
        let meta_bytes = serde_json::to_vec(&metadata)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&meta_key, &meta_bytes).await?;

        // Create version history entry
        let version_entry = FileVersion {
            version: new_version,
            content_key: versioned_content_key.clone(),
            timestamp: metadata.modified,
            author: author.clone(),
            size: content.len(),
            operation,
        };

        // Append to history
        let history_key = format!("file:history:{}", file_name);
        let mut history = self.file_history(&content_key).await.unwrap_or_default();
        history.push(version_entry.clone());
        let history_bytes = serde_json::to_vec(&history)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&history_key, &history_bytes).await?;

        // Append to global audit trail
        let audit_key = format!("audit:{}:{}", metadata.modified, author);
        let audit_bytes = serde_json::to_vec(&version_entry)
            .map_err(|e| Error::Serialization(e.to_string()))?;
        self.put(&audit_key, &audit_bytes).await?;

        Ok(content_key)
    }

    async fn get_file(&self, key: &str, output_path: &Path) -> Result<()> {
        // Get latest content
        let content = self.get(key).await?
            .ok_or_else(|| Error::Other("File not found".into()))?;

        // Write to output path
        fs::write(output_path, content).await
            .map_err(|e| Error::Io(e))?;

        Ok(())
    }

    async fn get_file_version(&self, key: &str, version: u64, output_path: &Path) -> Result<()> {
        // Extract file name
        let file_name = key.strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;

        // Get versioned content
        let versioned_key = format!("file:data:{}:v{}", file_name, version);
        let content = self.get(&versioned_key).await?
            .ok_or_else(|| Error::Other(format!("Version {} not found", version)))?;

        // Write to output path
        fs::write(output_path, content).await
            .map_err(|e| Error::Io(e))?;

        Ok(())
    }

    async fn list_files(&self) -> Result<Vec<FileMetadata>> {
        // TODO: Implement key listing in Swarm
        // For now, return empty list
        Ok(Vec::new())
    }

    async fn delete_file(&self, key: &str) -> Result<()> {
        // Extract file name from key
        let file_name = key.strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;

        // Get current metadata for version info
        let meta_key = format!("file:meta:{}", file_name);
        let metadata = if let Some(meta_bytes) = self.get(&meta_key).await? {
            Some(serde_json::from_slice::<FileMetadata>(&meta_bytes)
                .map_err(|e| Error::Serialization(e.to_string()))?)
        } else {
            None
        };

        // Create delete entry in history
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

            // Append to history
            let history_key = format!("file:history:{}", file_name);
            let mut history = self.file_history(key).await.unwrap_or_default();
            history.push(delete_entry.clone());
            let history_bytes = serde_json::to_vec(&history)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            self.put(&history_key, &history_bytes).await?;

            // Append to audit trail
            let audit_key = format!("audit:{}:{}", timestamp, author);
            let audit_bytes = serde_json::to_vec(&delete_entry)
                .map_err(|e| Error::Serialization(e.to_string()))?;
            self.put(&audit_key, &audit_bytes).await?;
        }

        // Delete metadata
        self.delete(&meta_key).await?;

        // Delete latest content
        self.delete(key).await?;

        // Note: We keep versioned content and history for audit purposes

        Ok(())
    }

    async fn file_metadata(&self, key: &str) -> Result<Option<FileMetadata>> {
        // Extract file name from key
        let file_name = key.strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;

        // Get metadata
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
        // Extract file name from key
        let file_name = key.strip_prefix("file:data:")
            .ok_or_else(|| Error::Other("Invalid file key".into()))?;

        // Get history
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
        // TODO: Implement key prefix scanning in Swarm
        // For now, return empty list
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_version_control() {
        // TODO: Add tests for version control
    }

    #[tokio::test]
    async fn test_conflict_detection() {
        // TODO: Add tests for conflict detection
    }

    #[tokio::test]
    async fn test_audit_trail() {
        // TODO: Add tests for audit trail
    }
}
