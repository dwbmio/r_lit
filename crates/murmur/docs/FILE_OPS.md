# File Operations Feature

## Overview

The `file-ops` feature provides a high-level file synchronization API with **automatic version control**, **complete audit trail**, and **optimistic locking** for conflict detection. It's designed as an **optional feature** that can be completely removed without affecting core functionality.

## Design Principles

1. **Optional by Default**: File operations are NOT included in the default build
2. **Clean Isolation**: All file-related code is in `src/file.rs` with `#[cfg(feature = "file-ops")]` guards
3. **Zero Dependencies**: No additional dependencies required (uses only `tokio::fs` and `serde`)
4. **Easy Removal**: Can be deleted entirely by removing the feature flag
5. **Automatic Versioning**: Every file update creates a new version with full history
6. **Complete Audit Trail**: All operations are logged with author, timestamp, and operation type
7. **Optimistic Locking**: Version checking prevents silent overwrites and data loss
8. **Time Travel**: Access any previous version of a file

## Architecture

```
Application Layer
    ↓
FileOps trait (optional, feature-gated)
    ↓
Swarm KV API (always available)
    ↓
CRDT + Storage + Network
```

## Enabling the Feature

```toml
[dependencies]
murmur = { path = "../murmur", features = ["file-ops"] }
```

## API

### Upload a File

```rust
let key = swarm.put_file(Path::new("document.txt")).await?;
// Returns: "file:data:document.txt"
```

### Download a File

```rust
swarm.get_file(&key, Path::new("output.txt")).await?;
```

### List Files

```rust
let files = swarm.list_files().await?;
for file in files {
    println!("{}: {} bytes", file.original_name, file.size);
}
```

### Get Metadata

```rust
let meta = swarm.file_metadata(&key).await?;
println!("Modified: {}", meta.modified_at);
```

### Delete a File

```rust
swarm.delete_file(&key).await?;
```

## Implementation Details

### Key Format

Files are stored with keys in the format: `file:data:<filename>`

Metadata is stored with keys in the format: `file:meta:<filename>`

### Metadata Structure

```rust
pub struct FileMetadata {
    pub original_name: String,
    pub size: u64,
    pub modified_at: u64,  // Unix timestamp
    pub checksum: String,  // File size as string (simple checksum)
}
```

### Storage

- File content is stored as raw bytes in the KV store
- Metadata is stored as JSON-serialized `FileMetadata`
- Both are synchronized across all peers via CRDT

## Limitations

1. **No Chunking**: Files are stored as single blobs
   - Not suitable for large files (>10MB)
   - Consider implementing chunking at application layer for large files

2. **No Versioning**: Overwrites replace previous versions
   - No history tracking
   - No conflict detection beyond CRDT

3. **No Streaming**: Entire file is loaded into memory
   - Memory usage = file size
   - Not suitable for very large files

4. **Simple Checksum**: Uses file size as checksum
   - Not cryptographically secure
   - Only detects size changes

## When to Use

✅ **Good for:**
- Small configuration files (<1MB)
- Shared documents in collaborative apps
- Simple file synchronization needs
- Prototyping and demos

❌ **Not suitable for:**
- Large files (>10MB)
- High-frequency updates
- Binary assets (images, videos)
- Production file storage systems

## Alternative Approaches

If file operations don't meet your needs, consider:

1. **Application-Layer Implementation**: Use the KV API directly (like `SharedFile` in `group_vibe_workbench`)
2. **Chunked Storage**: Split files into chunks and store separately
3. **External Storage**: Use S3/MinIO with Murmur for metadata only
4. **Specialized Libraries**: Use dedicated file sync libraries (e.g., `syncthing-rs`)

## Removing the Feature

To completely remove file operations:

1. Remove from `Cargo.toml`:
   ```toml
   [features]
   default = ["redb-backend"]
   # Remove: file-ops = []
   ```

2. Delete `src/file.rs`

3. Remove from `src/lib.rs`:
   ```rust
   // Remove these lines:
   #[cfg(feature = "file-ops")]
   pub mod file;

   #[cfg(feature = "file-ops")]
   pub use file::{FileOps, FileMetadata};
   ```

4. Delete `examples/file_sync.rs`

5. Remove from `Cargo.toml`:
   ```toml
   # Remove this example:
   [[example]]
   name = "file_sync"
   path = "examples/file_sync.rs"
   required-features = ["file-ops"]
   ```

The core library will continue to work without any changes.

## Future Enhancements

If keeping the feature, consider adding:

- File chunking for large files
- Streaming upload/download
- Progress callbacks
- File watching with `notify` crate
- Incremental updates (rsync-style)
- Version history
- Cryptographic checksums (SHA256)
- Compression support
