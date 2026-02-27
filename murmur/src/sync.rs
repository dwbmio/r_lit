use crate::{Error, Result};
use automerge::{Automerge, AutoCommit, ObjType, ReadDoc, transaction::Transactable};
use std::collections::HashMap;
use tracing::{debug, warn};

/// CRDT synchronization layer using Automerge.
pub struct Sync {
    doc: AutoCommit,
    /// Track which keys exist in the document
    keys: HashMap<String, automerge::ObjId>,
}

impl Sync {
    /// Create a new sync coordinator.
    pub fn new() -> Self {
        let doc = AutoCommit::new();
        Self {
            doc,
            keys: HashMap::new(),
        }
    }

    /// Put a key-value pair into the CRDT document.
    pub fn put(&mut self, key: &str, value: &[u8]) -> Result<Vec<u8>> {
        debug!("CRDT put: key={}", key);

        // Convert value to string for Automerge (it works best with text/maps)
        let value_str = String::from_utf8_lossy(value).to_string();

        // Put the value in the root map
        self.doc.put(automerge::ROOT, key, value_str)
            .map_err(|e| Error::Sync(format!("Failed to put value: {}", e)))?;

        // Get the changes since last save
        let changes = self.doc.get_last_local_change()
            .ok_or_else(|| Error::Sync("No changes generated".to_string()))?;

        // Serialize the change
        let change_bytes = changes.raw_bytes().to_vec();

        Ok(change_bytes)
    }

    /// Get a value by key from the CRDT document.
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        debug!("CRDT get: key={}", key);

        match self.doc.get(automerge::ROOT, key) {
            Ok(Some((value, _))) => {
                // Convert Automerge value to bytes
                let value_str = value.to_str()
                    .ok_or_else(|| Error::Sync("Value is not a string".to_string()))?;
                Ok(Some(value_str.as_bytes().to_vec()))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(Error::Sync(format!("Failed to get value: {}", e))),
        }
    }

    /// Delete a key from the CRDT document.
    pub fn delete(&mut self, key: &str) -> Result<Vec<u8>> {
        debug!("CRDT delete: key={}", key);

        self.doc.delete(automerge::ROOT, key)
            .map_err(|e| Error::Sync(format!("Failed to delete value: {}", e)))?;

        // Get the changes
        let changes = self.doc.get_last_local_change()
            .ok_or_else(|| Error::Sync("No changes generated".to_string()))?;

        let change_bytes = changes.raw_bytes().to_vec();

        Ok(change_bytes)
    }

    /// Apply a CRDT operation from another node.
    pub fn apply_changes(&mut self, change_bytes: &[u8]) -> Result<()> {
        debug!("Applying CRDT changes ({} bytes)", change_bytes.len());

        self.doc.load_incremental(change_bytes)
            .map_err(|e| Error::Sync(format!("Failed to apply changes: {}", e)))?;

        Ok(())
    }

    /// Get all changes since the beginning (for full sync).
    pub fn get_all_changes(&mut self) -> Result<Vec<u8>> {
        let changes = self.doc.save();
        Ok(changes)
    }

    /// Load a full document state from bytes.
    pub fn load_document(&mut self, data: &[u8]) -> Result<()> {
        self.doc.load_incremental(data)
            .map_err(|e| Error::Sync(format!("Failed to load document: {}", e)))?;

        Ok(())
    }

    /// Get all keys in the document.
    pub fn keys(&self) -> Vec<String> {
        let keys = self.doc.keys(automerge::ROOT);
        keys.map(|k| k.to_string()).collect()
    }

    /// Merge with another document.
    pub fn merge(&mut self, other_doc_bytes: &[u8]) -> Result<()> {
        debug!("Merging with another document");

        self.doc.load_incremental(other_doc_bytes)
            .map_err(|e| Error::Sync(format!("Failed to merge: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_put_get() {
        let mut sync = Sync::new();

        let changes = sync.put("key1", b"value1").unwrap();
        assert!(!changes.is_empty());

        let value = sync.get("key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_sync_delete() {
        let mut sync = Sync::new();

        sync.put("key1", b"value1").unwrap();
        assert!(sync.get("key1").unwrap().is_some());

        sync.delete("key1").unwrap();
        assert!(sync.get("key1").unwrap().is_none());
    }

    #[test]
    fn test_sync_merge() {
        let mut sync1 = Sync::new();
        let mut sync2 = Sync::new();

        // Node 1 puts a value
        let changes1 = sync1.put("key1", b"value1").unwrap();

        // Node 2 applies the changes
        sync2.apply_changes(&changes1).unwrap();

        // Node 2 should have the value
        assert_eq!(sync2.get("key1").unwrap(), Some(b"value1".to_vec()));
    }
}
