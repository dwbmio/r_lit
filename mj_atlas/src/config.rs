//! Per-user mj_atlas configuration, persisted as TOML at the platform's
//! standard config dir (`dirs::config_dir()`):
//!
//!   • macOS:   `~/Library/Application Support/mj_atlas/config.toml`
//!   • Linux:   `~/.config/mj_atlas/config.toml`
//!   • Windows: `%APPDATA%\mj_atlas\config.toml`
//!
//! The file holds non-project state — currently only the hfrog mirror
//! settings, but the layout is intentionally extensible. Every struct uses
//! `#[serde(default)]` so a field added in a later version doesn't fail to
//! parse an older config (and vice versa).
//!
//! The CLI reads this on every invocation; the GUI loads it once on startup
//! and writes it back when the user clicks "Save settings" in the panel.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level config wrapper. New top-level sections are added as new fields,
/// always with `#[serde(default)]` so missing entries deserialize as defaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub hfrog: HfrogConfig,
}

/// Configures the optional hfrog artifact-registry mirror. When `enabled` is
/// true and `endpoint` is non-empty, mj_atlas pushes a copy of every saved
/// project / exported atlas / refreshed manifest to the hfrog instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfrogConfig {
    /// Master toggle. Disabled by default — opt-in.
    #[serde(default)]
    pub enabled: bool,
    /// Base URL of the hfrog server, e.g. `https://hfrog.example.com`. No
    /// trailing slash. Empty disables the mirror regardless of `enabled`.
    /// New configs default to our internal deployment for convenience —
    /// users on different deployments override via the GUI / TOML.
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    /// Bearer token for authenticated hfrog instances. Empty = no auth header.
    /// Stored in plain text in config.toml — appropriate for dev tokens, NOT
    /// for production secrets.
    #[serde(default)]
    pub token: String,
    /// Hfrog `runtime` field stamped on every uploaded artifact. Useful for
    /// filtering / grouping in the registry's UI. Defaults to "asset-pack".
    #[serde(default = "default_runtime")]
    pub default_runtime: String,
    /// S3 backend pool index used by the hfrog server (server-side concept).
    /// Defaults to 0 — the first registered backend.
    #[serde(default)]
    pub s3_inc_id: i64,
}

fn default_runtime() -> String {
    "asset-pack".to_string()
}

/// Pre-populated endpoint for our internal hfrog deployment. New users get
/// this filled in, but `enabled = false` keeps the mirror opt-in — flipping
/// the checkbox in the GUI (or the toml field) is what actually starts
/// pushing artifacts. Set to empty by changing the config; the value is
/// just a convenience default, not a requirement.
fn default_endpoint() -> String {
    "https://hfrog.gamesci-lite.com".to_string()
}

impl Default for HfrogConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_endpoint(),
            token: String::new(),
            default_runtime: default_runtime(),
            s3_inc_id: 0,
        }
    }
}

impl HfrogConfig {
    /// Whether the config has enough fields to actually attempt an upload.
    /// `enabled=true` alone isn't sufficient — we also need an endpoint.
    pub fn is_active(&self) -> bool {
        self.enabled && !self.endpoint.trim().is_empty()
    }
}

impl Config {
    /// Standard config path for this user. Caller may write to a different
    /// path (e.g. tests pass an explicit location) but `load`/`save` on
    /// `Config` itself always use this default.
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("mj_atlas").join("config.toml"))
    }

    /// Load the user's config from `default_path()`. Returns `Config::default()`
    /// when the file is missing — that's the expected first-run state, not an
    /// error. Parse errors fall through (caller decides whether to fall back).
    pub fn load() -> Result<Self> {
        match Self::default_path() {
            Some(path) => Self::load_from(&path),
            None => Ok(Self::default()),
        }
    }

    /// Same as `load`, but reads from an explicit path (used by tests).
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)?;
        let parsed = toml::from_str::<Config>(&content)
            .map_err(|e| AppError::Custom(format!("config parse error ({}): {}", path.display(), e)))?;
        Ok(parsed)
    }

    /// Save to `default_path()`, creating the directory tree if needed.
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path()
            .ok_or_else(|| AppError::Custom("no platform config dir available".into()))?;
        self.save_to(&path)
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(self)
            .map_err(|e| AppError::Custom(format!("config serialize error: {}", e)))?;
        std::fs::write(path, serialized)?;
        log::info!("Saved config: {}", path.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("mj_atlas_cfg_{}_{}.toml", std::process::id(), name))
    }

    #[test]
    fn missing_file_yields_defaults_no_error() {
        let path = tmp("missing");
        let _ = std::fs::remove_file(&path);
        let cfg = Config::load_from(&path).expect("missing file is not an error");
        // enabled stays opt-in; endpoint is pre-populated for convenience.
        assert!(!cfg.hfrog.enabled);
        assert_eq!(cfg.hfrog.endpoint, "https://hfrog.gamesci-lite.com");
        assert_eq!(cfg.hfrog.default_runtime, "asset-pack");
    }

    #[test]
    fn round_trip_preserves_values() {
        let path = tmp("roundtrip");
        let cfg = Config {
            hfrog: HfrogConfig {
                enabled: true,
                endpoint: "https://hfrog.test.local".into(),
                token: "deadbeef".into(),
                default_runtime: "asset-pack-test".into(),
                s3_inc_id: 7,
            },
        };
        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert!(loaded.hfrog.enabled);
        assert_eq!(loaded.hfrog.endpoint, "https://hfrog.test.local");
        assert_eq!(loaded.hfrog.token, "deadbeef");
        assert_eq!(loaded.hfrog.default_runtime, "asset-pack-test");
        assert_eq!(loaded.hfrog.s3_inc_id, 7);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn partial_toml_with_missing_fields_uses_defaults() {
        // A config written by a future / older version that only has some
        // fields must still parse. Field-level #[serde(default)] handles this.
        let path = tmp("partial");
        std::fs::write(&path, "[hfrog]\nenabled = true\n").unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert!(loaded.hfrog.enabled);
        // missing endpoint ⇒ falls back to the default (pre-populated URL),
        // NOT empty — this is what the user gets on a fresh install.
        assert_eq!(loaded.hfrog.endpoint, "https://hfrog.gamesci-lite.com");
        assert_eq!(loaded.hfrog.default_runtime, "asset-pack");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn is_active_requires_both_enabled_and_endpoint() {
        // Default has an endpoint pre-populated; explicitly clear it so we
        // exercise the "no endpoint" branch independently of `enabled`.
        let mut h = HfrogConfig::default();
        h.endpoint.clear();
        assert!(!h.is_active(), "disabled + empty endpoint must be inactive");
        h.enabled = true;
        assert!(!h.is_active(), "enabled alone (empty endpoint) is insufficient");
        h.endpoint = "https://x".into();
        assert!(h.is_active(), "enabled + endpoint ⇒ active");
        h.endpoint = "   ".into();
        assert!(!h.is_active(), "whitespace endpoint should not be active");
    }
}
