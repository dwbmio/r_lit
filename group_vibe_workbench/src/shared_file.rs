use crate::error::Result;
use murmur::Swarm;
use std::path::PathBuf;
use tokio::sync::RwLock;
use std::sync::Arc;
use log::{info, error};
use notify::{Watcher, RecursiveMode, Event};
use tokio::sync::mpsc;
use chrono::{DateTime, Utc};

/// Manages a shared file that is synchronized across peers using murmur
pub struct SharedFile {
    swarm: Arc<Swarm>,
    file_key: String,
    local_path: PathBuf,
    content: Arc<RwLock<String>>,
    edit_history: Arc<RwLock<Vec<EditRecord>>>,
}

/// Record of an edit to the shared file
#[derive(Debug, Clone)]
pub struct EditRecord {
    pub timestamp: DateTime<Utc>,
    pub node_id: String,
    pub content_length: usize,
    pub is_local: bool,
}

impl SharedFile {
    /// Create a new shared file instance using an existing Swarm
    ///
    /// # Arguments
    /// - `swarm`: An existing Swarm instance (should be the global singleton)
    /// - `file_key`: Key to identify this file in the distributed store
    /// - `local_path`: Path to the local file
    pub async fn new(
        swarm: Swarm,
        file_key: String,
        local_path: PathBuf,
    ) -> Result<Self> {
        info!("Initializing shared file: key={}, path={:?}", file_key, local_path);

        // Load initial content from local file if it exists
        let initial_content = if local_path.exists() {
            tokio::fs::read_to_string(&local_path)
                .await
                .unwrap_or_else(|_| String::new())
        } else {
            String::new()
        };

        // Try to get content from swarm (might have been synced from peers)
        let synced_content = swarm.get(&file_key)
            .await
            .map_err(|e| crate::error::AppError::Other(format!("Failed to get from swarm: {}", e)))?;

        let content = if let Some(bytes) = synced_content {
            String::from_utf8(bytes).unwrap_or(initial_content)
        } else {
            // If no synced content, use local content and push to swarm
            if !initial_content.is_empty() {
                swarm.put(&file_key, initial_content.as_bytes())
                    .await
                    .map_err(|e| crate::error::AppError::Other(format!("Failed to put to swarm: {}", e)))?;
            }
            initial_content
        };

        Ok(Self {
            swarm: Arc::new(swarm),
            file_key,
            local_path,
            content: Arc::new(RwLock::new(content)),
            edit_history: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Start watching the file for changes and sync them
    pub async fn start_watching(&self) -> Result<()> {
        let local_path = self.local_path.clone();
        let swarm = self.swarm.clone();
        let file_key = self.file_key.clone();
        let content = self.content.clone();
        let edit_history = self.edit_history.clone();
        let node_id = swarm.node_id().await;

        tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(100);

            let mut watcher = match notify::recommended_watcher(move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    if matches!(event.kind, notify::EventKind::Modify(_)) {
                        let _ = tx.blocking_send(event);
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    error!("Failed to create file watcher: {}", e);
                    return;
                }
            };

            if let Err(e) = watcher.watch(&local_path, RecursiveMode::NonRecursive) {
                error!("Failed to watch file: {}", e);
                return;
            }

            info!("Started watching file: {:?}", local_path);

            while let Some(_event) = rx.recv().await {
                // Read the updated file content
                match tokio::fs::read_to_string(&local_path).await {
                    Ok(new_content) => {
                        // Check if content actually changed
                        let current_content = content.read().await.clone();
                        if new_content != current_content {
                            info!("File changed, syncing {} bytes", new_content.len());

                            // Update local content
                            {
                                let mut content_guard = content.write().await;
                                *content_guard = new_content.clone();
                            }

                            // Add to edit history
                            {
                                let mut history = edit_history.write().await;
                                history.push(EditRecord {
                                    timestamp: Utc::now(),
                                    node_id: node_id.clone(),
                                    content_length: new_content.len(),
                                    is_local: true,
                                });
                            }

                            // Sync to swarm
                            if let Err(e) = swarm.put(&file_key, new_content.as_bytes()).await {
                                error!("Failed to sync to swarm: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to read file: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Get the edit history
    pub async fn get_edit_history(&self) -> Vec<EditRecord> {
        self.edit_history.read().await.clone()
    }

    /// Get the current content of the shared file
    pub async fn get_content(&self) -> String {
        self.content.read().await.clone()
    }

    /// Update the content of the shared file and sync to peers
    pub async fn update_content(&self, new_content: String) -> Result<()> {
        info!("Updating shared file content: {} bytes", new_content.len());

        // Update local content
        {
            let mut content = self.content.write().await;
            *content = new_content.clone();
        }

        // Sync to swarm (will broadcast to all peers)
        self.swarm.put(&self.file_key, new_content.as_bytes())
            .await
            .map_err(|e| crate::error::AppError::Other(format!("Failed to sync to swarm: {}", e)))?;

        // Save to local file
        if let Err(e) = tokio::fs::write(&self.local_path, new_content).await {
            error!("Failed to write to local file: {}", e);
        }

        Ok(())
    }

    /// Get node information
    pub async fn node_info(&self) -> NodeInfo {
        NodeInfo {
            node_id: self.swarm.node_id().await,
            is_leader: self.swarm.is_leader().await,
            leader_id: self.swarm.leader_id().await,
            connected_peers: self.swarm.connected_peers().await,
        }
    }

    /// Shutdown the shared file and cleanup
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down shared file");
        self.swarm.shutdown()
            .await
            .map_err(|e| crate::error::AppError::Other(format!("Failed to shutdown swarm: {}", e)))?;
        Ok(())
    }
}

/// Information about the current node in the swarm
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub node_id: String,
    pub is_leader: bool,
    pub leader_id: Option<String>,
    pub connected_peers: Vec<String>,
}
