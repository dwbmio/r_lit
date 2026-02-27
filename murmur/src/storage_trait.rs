use crate::{Error, Result};

/// Trait for storage backends.
///
/// This allows swapping between different storage engines
/// (SQLite, redb, RocksDB) without changing the core logic.
pub trait StorageBackend: Send + Sync {
    /// Store a key-value pair.
    fn put(&self, key: &str, value: &[u8]) -> Result<()>;

    /// Retrieve a value by key.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete a key.
    fn delete(&self, key: &str) -> Result<()>;

    /// List all keys (optional, for debugging).
    fn keys(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }
}
