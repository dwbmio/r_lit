use crate::{Error, Result};
use automerge::{AutoCommit, ReadDoc, sync::SyncDoc, transaction::Transactable};
use std::collections::HashMap;
use tracing::{debug, info};

/// CRDT synchronization layer using Automerge.
///
/// Supports both:
/// - **Incremental sync**: per-peer `automerge::sync::State` with bloom-filter-based
///   diff exchange (only missing changes are transmitted).
/// - **Real-time updates**: individual `CrdtUpdate` messages for single-key changes.
pub struct Sync {
    doc: AutoCommit,
    /// Per-peer sync states for incremental sync protocol.
    peer_states: HashMap<String, automerge::sync::State>,
    /// Track which keys exist in the document
    #[allow(dead_code)]
    keys: HashMap<String, automerge::ObjId>,
}

impl Sync {
    /// Create a new sync coordinator.
    pub fn new() -> Self {
        let doc = AutoCommit::new();
        Self {
            doc,
            peer_states: HashMap::new(),
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

    /// Apply a CRDT operation from another node (single CrdtUpdate message).
    pub fn apply_changes(&mut self, change_bytes: &[u8]) -> Result<()> {
        debug!("Applying CRDT changes ({} bytes)", change_bytes.len());

        self.doc.load_incremental(change_bytes)
            .map_err(|e| Error::Sync(format!("Failed to apply changes: {}", e)))?;

        Ok(())
    }

    /// Get all changes since the beginning (for full sync — kept as fallback).
    pub fn get_all_changes(&mut self) -> Result<Vec<u8>> {
        let changes = self.doc.save();
        Ok(changes)
    }

    /// Load a full document state from bytes (fallback).
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

    /// Compute a state hash from the CRDT document heads (first 16 hex chars).
    pub fn state_hash(&mut self) -> String {
        let heads = self.doc.get_heads();
        if heads.is_empty() {
            return "empty".into();
        }
        let mut parts: Vec<String> = heads
            .iter()
            .map(|h| h.0.iter().map(|b| format!("{:02x}", b)).collect::<String>())
            .collect();
        parts.sort();
        let combined = parts.join("");
        combined[..16.min(combined.len())].to_string()
    }

    /// Check whether a key has conflicting concurrent values in the CRDT.
    pub fn has_conflicts(&self, key: &str) -> bool {
        self.doc.get_all(automerge::ROOT, key)
            .map(|values| values.len() > 1)
            .unwrap_or(false)
    }

    /// Merge with another document.
    pub fn merge(&mut self, other_doc_bytes: &[u8]) -> Result<()> {
        debug!("Merging with another document");

        self.doc.load_incremental(other_doc_bytes)
            .map_err(|e| Error::Sync(format!("Failed to merge: {}", e)))?;

        Ok(())
    }

    // ── Incremental sync protocol ────────────────────────────

    /// Generate an incremental sync message for a peer.
    ///
    /// Returns `Some(encoded_bytes)` if there's data to send, `None` if fully synced.
    /// Uses automerge's bloom-filter-based sync protocol to only send missing changes.
    pub fn generate_sync_message(&mut self, peer_id: &str) -> Option<Vec<u8>> {
        let state = self.peer_states.entry(peer_id.to_string())
            .or_insert_with(automerge::sync::State::new);

        self.doc.sync().generate_sync_message(state)
            .map(|msg| {
                let encoded = msg.encode();
                debug!(
                    "Generated sync message for peer {} ({} bytes)",
                    &peer_id[..8.min(peer_id.len())],
                    encoded.len()
                );
                encoded
            })
    }

    /// Receive an incremental sync message from a peer.
    ///
    /// Applies the peer's changes to our document and updates the sync state.
    /// After calling this, call `generate_sync_message` to produce a reply.
    ///
    /// Returns the list of keys that were updated by this sync message.
    pub fn receive_sync_message(&mut self, peer_id: &str, msg_bytes: &[u8]) -> Result<Vec<String>> {
        let msg = automerge::sync::Message::decode(msg_bytes)
            .map_err(|e| Error::Sync(format!("Failed to decode sync message: {}", e)))?;

        debug!(
            "Received sync message from peer {} ({} bytes, {} changes)",
            &peer_id[..8.min(peer_id.len())],
            msg_bytes.len(),
            msg.changes.len()
        );

        // Snapshot keys before applying
        let keys_before: std::collections::HashSet<String> = self.doc.keys(automerge::ROOT)
            .map(|k| k.to_string())
            .collect();
        // Snapshot values of existing keys for change detection
        let values_before: HashMap<String, Option<Vec<u8>>> = keys_before.iter()
            .map(|k| (k.clone(), self.get(k).ok().flatten()))
            .collect();

        let state = self.peer_states.entry(peer_id.to_string())
            .or_insert_with(automerge::sync::State::new);

        self.doc.sync().receive_sync_message(state, msg)
            .map_err(|e| Error::Sync(format!("Failed to receive sync message: {}", e)))?;

        // Find keys that changed
        let keys_after: std::collections::HashSet<String> = self.doc.keys(automerge::ROOT)
            .map(|k| k.to_string())
            .collect();

        let mut changed_keys = Vec::new();

        // New keys
        for k in keys_after.difference(&keys_before) {
            changed_keys.push(k.clone());
        }

        // Existing keys whose values changed
        for k in keys_before.intersection(&keys_after) {
            let new_val = self.get(k).ok().flatten();
            let old_val = values_before.get(k).cloned().flatten();
            if new_val != old_val {
                changed_keys.push(k.clone());
            }
        }

        // Deleted keys
        for k in keys_before.difference(&keys_after) {
            changed_keys.push(k.clone());
        }

        if !changed_keys.is_empty() {
            info!(
                "Sync from {} updated {} keys: {:?}",
                &peer_id[..8.min(peer_id.len())],
                changed_keys.len(),
                &changed_keys[..changed_keys.len().min(5)]
            );
        }

        Ok(changed_keys)
    }

    /// Check if sync with a peer is complete (no more messages to exchange).
    pub fn is_sync_complete(&mut self, peer_id: &str) -> bool {
        self.generate_sync_message(peer_id).is_none()
    }

    /// Remove sync state for a disconnected peer.
    pub fn remove_peer_state(&mut self, peer_id: &str) {
        self.peer_states.remove(peer_id);
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

    #[test]
    fn test_incremental_sync_basic() {
        let mut sync1 = Sync::new();
        let mut sync2 = Sync::new();

        // Node 1 has some data
        sync1.put("key1", b"value1").unwrap();
        sync1.put("key2", b"value2").unwrap();

        // Incremental sync: node1 → node2
        let peer2_id = "peer2_node_id_placeholder";
        let peer1_id = "peer1_node_id_placeholder";

        // Round 1: node1 generates initial message
        let msg1 = sync1.generate_sync_message(peer2_id);
        assert!(msg1.is_some(), "First sync message should not be None");

        // Node2 receives and replies
        let changed = sync2.receive_sync_message(peer1_id, &msg1.unwrap()).unwrap();
        // First message is just heads exchange, no data yet
        let msg2 = sync2.generate_sync_message(peer1_id);
        assert!(msg2.is_some(), "Node2 should reply");

        // Node1 receives reply
        sync1.receive_sync_message(peer2_id, &msg2.unwrap()).unwrap();

        // Continue until sync complete
        let mut rounds = 0;
        loop {
            let m1 = sync1.generate_sync_message(peer2_id);
            let m2 = sync2.generate_sync_message(peer1_id);

            if m1.is_none() && m2.is_none() {
                break;
            }

            if let Some(data) = m1 {
                sync2.receive_sync_message(peer1_id, &data).unwrap();
            }
            if let Some(data) = m2 {
                sync1.receive_sync_message(peer2_id, &data).unwrap();
            }

            rounds += 1;
            assert!(rounds < 20, "Sync should converge in reasonable rounds");
        }

        // Node2 should have all data
        assert_eq!(sync2.get("key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(sync2.get("key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_incremental_sync_bidirectional() {
        let mut sync1 = Sync::new();
        let mut sync2 = Sync::new();

        // Both nodes have different data
        sync1.put("from_node1", b"hello").unwrap();
        sync2.put("from_node2", b"world").unwrap();

        let p1 = "node1";
        let p2 = "node2";

        // Multi-round sync
        let mut rounds = 0;
        loop {
            let m1to2 = sync1.generate_sync_message(p2);
            let m2to1 = sync2.generate_sync_message(p1);

            if m1to2.is_none() && m2to1.is_none() {
                break;
            }

            if let Some(data) = m1to2 {
                sync2.receive_sync_message(p1, &data).unwrap();
            }
            if let Some(data) = m2to1 {
                sync1.receive_sync_message(p2, &data).unwrap();
            }

            rounds += 1;
            assert!(rounds < 20, "Sync should converge");
        }

        // Both should have all data
        assert_eq!(sync1.get("from_node2").unwrap(), Some(b"world".to_vec()));
        assert_eq!(sync2.get("from_node1").unwrap(), Some(b"hello".to_vec()));
    }

    #[test]
    fn test_incremental_sync_only_sends_diff() {
        let mut sync1 = Sync::new();
        let mut sync2 = Sync::new();

        // Add enough initial data so the full state is meaningfully larger
        for i in 0..20 {
            sync1.put(&format!("key_{:03}", i), format!("value_{}", i).as_bytes()).unwrap();
        }

        let p1 = "node1";
        let p2 = "node2";

        // Full sync round
        loop {
            let m1 = sync1.generate_sync_message(p2);
            let m2 = sync2.generate_sync_message(p1);
            if m1.is_none() && m2.is_none() { break; }
            if let Some(d) = m1 { sync2.receive_sync_message(p1, &d).unwrap(); }
            if let Some(d) = m2 { sync1.receive_sync_message(p2, &d).unwrap(); }
        }

        assert_eq!(sync2.get("key_000").unwrap(), Some(b"value_0".to_vec()));

        // Now add one new key on node1
        sync1.put("new_key", b"new_value").unwrap();

        // The next sync should only send the new change, not the full doc
        let msg = sync1.generate_sync_message(p2);
        assert!(msg.is_some());
        let msg_bytes = msg.unwrap();

        // The message should be smaller than the full state (20 keys + 1)
        let full_state = sync1.get_all_changes().unwrap();
        assert!(
            msg_bytes.len() < full_state.len(),
            "Incremental message ({} bytes) should be smaller than full state ({} bytes)",
            msg_bytes.len(),
            full_state.len()
        );

        // Apply and verify
        let changed = sync2.receive_sync_message(p1, &msg_bytes).unwrap();
        assert!(changed.contains(&"new_key".to_string()));
        assert_eq!(sync2.get("new_key").unwrap(), Some(b"new_value".to_vec()));
    }
}
