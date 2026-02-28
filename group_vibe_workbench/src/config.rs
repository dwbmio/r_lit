use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Data directory for storing user data, swarm data, etc.
    pub data_dir: PathBuf,

    /// Network port for P2P communication
    pub network_port: u16,

    /// mDNS service name for local discovery
    pub mdns_service_name: String,

    /// Shared file path for collaboration
    pub shared_file_path: PathBuf,

    /// Window width
    pub window_width: u32,

    /// Window height
    pub window_height: u32,

    /// Log level
    pub log_level: String,

    /// Enable JSON output
    pub json_output: bool,
}

impl Default for Config {
    fn default() -> Self {
        // In debug mode: use process ID to allow multiple instances for testing
        // In release mode: use fixed path for single instance
        let data_dir = if cfg!(debug_assertions) {
            let pid = std::process::id();
            PathBuf::from(format!("./workbench_data_{}", pid))
        } else {
            PathBuf::from("./workbench_data")
        };

        Self {
            data_dir,
            network_port: 9000,
            mdns_service_name: "_murmur._tcp".to_string(),
            shared_file_path: PathBuf::from("../chat.ctx"),
            window_width: 1280,
            window_height: 720,
            log_level: if cfg!(debug_assertions) {
                "debug".to_string()
            } else {
                "info".to_string()
            },
            json_output: false,
        }
    }
}

impl Config {
    /// Load configuration from environment variables and .env file
    pub fn load() -> crate::error::Result<Self> {
        // Load .env file if it exists (but don't fail if it doesn't)
        let _ = dotenv::dotenv();

        let mut config = Self::default();

        // Override with environment variables
        if let Ok(data_dir) = std::env::var("DATA_DIR") {
            config.data_dir = PathBuf::from(data_dir);
        }
        // Also check WORKBENCH_DATA_DIR for backward compatibility
        if let Ok(data_dir) = std::env::var("WORKBENCH_DATA_DIR") {
            config.data_dir = PathBuf::from(data_dir);
        }

        if let Ok(port) = std::env::var("NETWORK_PORT") {
            config.network_port = port.parse().unwrap_or(9000);
        }
        // Also check WORKBENCH_PORT for backward compatibility
        if let Ok(port) = std::env::var("WORKBENCH_PORT") {
            config.network_port = port.parse().unwrap_or(9000);
        }

        if let Ok(service_name) = std::env::var("MDNS_SERVICE_NAME") {
            config.mdns_service_name = service_name;
        }

        if let Ok(file_path) = std::env::var("SHARED_FILE_PATH") {
            config.shared_file_path = PathBuf::from(file_path);
        }

        if let Ok(width) = std::env::var("WINDOW_WIDTH") {
            config.window_width = width.parse().unwrap_or(1280);
        }

        if let Ok(height) = std::env::var("WINDOW_HEIGHT") {
            config.window_height = height.parse().unwrap_or(720);
        }

        if let Ok(log_level) = std::env::var("LOG_LEVEL") {
            config.log_level = log_level;
        }

        if let Ok(json_output) = std::env::var("JSON_OUTPUT") {
            config.json_output = json_output.eq_ignore_ascii_case("true") || json_output == "1";
        }

        Ok(config)
    }

    /// Get the user database path
    pub fn user_db_path(&self) -> PathBuf {
        self.data_dir.join("user.db")
    }

    /// Get the swarm storage path for a user
    pub fn swarm_path(&self, user_id: &str) -> PathBuf {
        self.data_dir.join("swarm").join(user_id)
    }

    /// Ensure all necessary directories exist
    pub fn ensure_dirs(&self) -> crate::error::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(self.data_dir.join("swarm"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        // Data dir should contain process ID
        assert!(config.data_dir.to_string_lossy().starts_with("./workbench_data_"));
        assert_eq!(config.network_port, 9000);
    }

    #[test]
    fn test_user_db_path() {
        let config = Config::default();
        let user_db = config.user_db_path();
        // Should be in the data_dir
        assert!(user_db.to_string_lossy().contains("workbench_data_"));
        assert!(user_db.to_string_lossy().ends_with("user.db"));
    }

    #[test]
    fn test_swarm_path() {
        let config = Config::default();
        let swarm_path = config.swarm_path("test-user-id");
        // Should be in the data_dir/swarm/user-id
        assert!(swarm_path.to_string_lossy().contains("workbench_data_"));
        assert!(swarm_path.to_string_lossy().contains("swarm"));
        assert!(swarm_path.to_string_lossy().ends_with("test-user-id"));
    }
}
