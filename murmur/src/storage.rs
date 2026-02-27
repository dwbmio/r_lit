use crate::{Error, Result, StorageBackend};
use std::path::Path;

// SQLite backend
#[cfg(feature = "sqlite-backend")]
mod sqlite_impl {
    use super::*;
    use rusqlite::{Connection, params};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    pub struct SqliteStorage {
        conn: Arc<Mutex<Connection>>,
        _path: PathBuf,
    }

    impl SqliteStorage {
        pub fn new(path: &Path) -> Result<Self> {
            std::fs::create_dir_all(path)?;
            let db_path = path.join("murmur.db");
            let conn = Connection::open(&db_path)?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS kv_store (
                    key TEXT PRIMARY KEY,
                    value BLOB NOT NULL,
                    version INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                )",
                [],
            )?;

            Ok(Self {
                conn: Arc::new(Mutex::new(conn)),
                _path: db_path,
            })
        }
    }

    impl StorageBackend for SqliteStorage {
        fn put(&self, key: &str, value: &[u8]) -> Result<()> {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Time went backwards")
                .as_secs() as i64;

            let conn = self.conn.lock()
                .map_err(|_e| Error::Other("Mutex lock failed".to_string()))?;

            conn.execute(
                "INSERT OR REPLACE INTO kv_store (key, value, version, updated_at)
                 VALUES (?1, ?2, 1, ?3)",
                params![key, value, timestamp],
            ).map_err(|e| Error::Other(format!("SQLite error: {}", e)))?;

            Ok(())
        }

        fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            let conn = self.conn.lock()
                .map_err(|_e| Error::Other("Mutex lock failed".to_string()))?;

            let mut stmt = conn.prepare("SELECT value FROM kv_store WHERE key = ?1")
                .map_err(|e| Error::Other(format!("SQLite error: {}", e)))?;
            let mut rows = stmt.query(params![key])
                .map_err(|e| Error::Other(format!("SQLite error: {}", e)))?;

            if let Some(row) = rows.next().map_err(|e| Error::Other(format!("SQLite error: {}", e)))? {
                let value: Vec<u8> = row.get(0).map_err(|e| Error::Other(format!("SQLite error: {}", e)))?;
                Ok(Some(value))
            } else {
                Ok(None)
            }
        }

        fn delete(&self, key: &str) -> Result<()> {
            let conn = self.conn.lock()
                .map_err(|_e| Error::Other("Mutex lock failed".to_string()))?;

            conn.execute("DELETE FROM kv_store WHERE key = ?1", params![key])
                .map_err(|e| Error::Other(format!("SQLite error: {}", e)))?;
            Ok(())
        }
    }
}

// redb backend
#[cfg(feature = "redb-backend")]
mod redb_impl {
    use super::*;
    use redb::{Database, ReadableTable, TableDefinition};
    use std::sync::Arc;

    const KV_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("kv_store");

    #[derive(Clone)]
    pub struct RedbStorage {
        db: Arc<Database>,
    }

    impl RedbStorage {
        pub fn new(path: &Path) -> Result<Self> {
            std::fs::create_dir_all(path)?;
            let db_path = path.join("murmur.redb");

            let db = Database::create(&db_path)
                .map_err(|e| Error::Other(format!("Failed to create redb: {}", e)))?;

            let write_txn = db.begin_write()
                .map_err(|e| Error::Other(format!("Failed to begin write: {}", e)))?;

            {
                let _table = write_txn.open_table(KV_TABLE)
                    .map_err(|e| Error::Other(format!("Failed to open table: {}", e)))?;
            }

            write_txn.commit()
                .map_err(|e| Error::Other(format!("Failed to commit: {}", e)))?;

            Ok(Self { db: Arc::new(db) })
        }
    }

    impl StorageBackend for RedbStorage {
        fn put(&self, key: &str, value: &[u8]) -> Result<()> {
            let write_txn = self.db.begin_write()
                .map_err(|e| Error::Other(format!("Failed to begin write: {}", e)))?;

            {
                let mut table = write_txn.open_table(KV_TABLE)
                    .map_err(|e| Error::Other(format!("Failed to open table: {}", e)))?;

                table.insert(key, value)
                    .map_err(|e| Error::Other(format!("Failed to insert: {}", e)))?;
            }

            write_txn.commit()
                .map_err(|e| Error::Other(format!("Failed to commit: {}", e)))?;

            Ok(())
        }

        fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            let read_txn = self.db.begin_read()
                .map_err(|e| Error::Other(format!("Failed to begin read: {}", e)))?;

            let table = read_txn.open_table(KV_TABLE)
                .map_err(|e| Error::Other(format!("Failed to open table: {}", e)))?;

            match table.get(key) {
                Ok(Some(value)) => Ok(Some(value.value().to_vec())),
                Ok(None) => Ok(None),
                Err(e) => Err(Error::Other(format!("Failed to get: {}", e))),
            }
        }

        fn delete(&self, key: &str) -> Result<()> {
            let write_txn = self.db.begin_write()
                .map_err(|e| Error::Other(format!("Failed to begin write: {}", e)))?;

            {
                let mut table = write_txn.open_table(KV_TABLE)
                    .map_err(|e| Error::Other(format!("Failed to open table: {}", e)))?;

                table.remove(key)
                    .map_err(|e| Error::Other(format!("Failed to remove: {}", e)))?;
            }

            write_txn.commit()
                .map_err(|e| Error::Other(format!("Failed to commit: {}", e)))?;

            Ok(())
        }
    }
}

// RocksDB backend
#[cfg(feature = "rocksdb-backend")]
mod rocksdb_impl {
    use super::*;
    use rocksdb::{DB, Options};
    use std::sync::Arc;

    #[derive(Clone)]
    pub struct RocksDbStorage {
        db: Arc<DB>,
    }

    impl RocksDbStorage {
        pub fn new(path: &Path) -> Result<Self> {
            std::fs::create_dir_all(path)?;
            let db_path = path.join("murmur.rocksdb");

            let mut opts = Options::default();
            opts.create_if_missing(true);
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);

            let db = DB::open(&opts, &db_path)
                .map_err(|e| Error::Other(format!("Failed to open RocksDB: {}", e)))?;

            Ok(Self { db: Arc::new(db) })
        }
    }

    impl StorageBackend for RocksDbStorage {
        fn put(&self, key: &str, value: &[u8]) -> Result<()> {
            self.db.put(key.as_bytes(), value)
                .map_err(|e| Error::Other(format!("RocksDB put failed: {}", e)))?;
            Ok(())
        }

        fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
            match self.db.get(key.as_bytes()) {
                Ok(Some(value)) => Ok(Some(value)),
                Ok(None) => Ok(None),
                Err(e) => Err(Error::Other(format!("RocksDB get failed: {}", e))),
            }
        }

        fn delete(&self, key: &str) -> Result<()> {
            self.db.delete(key.as_bytes())
                .map_err(|e| Error::Other(format!("RocksDB delete failed: {}", e)))?;
            Ok(())
        }
    }
}

// Export the appropriate storage type based on features
#[cfg(feature = "sqlite-backend")]
pub use sqlite_impl::SqliteStorage as Storage;

#[cfg(feature = "redb-backend")]
pub use redb_impl::RedbStorage as Storage;

#[cfg(feature = "rocksdb-backend")]
pub use rocksdb_impl::RocksDbStorage as Storage;
