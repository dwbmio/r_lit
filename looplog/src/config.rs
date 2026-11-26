//! Single-file configuration: `~/.config/looplog/config.toml`.
//!
//! Everything the viewer needs is here — which streams to follow, which columns
//! to show, how long a window to load. The file is the single source of truth
//! for the GUI; CLI flags only override it for one run.
//!
//! Secrets are deliberately *not* stored here: `serve --token` stays a CLI
//! argument (and the viewer only needs it if you expose the intake on a LAN).

use crate::error::{Error, Result};
use crate::source::SourceSpec;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub view: ViewConfig,
    #[serde(default)]
    pub serve: ServeConfig,
    #[serde(default)]
    pub source: Vec<SourceConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            view: ViewConfig::default(),
            serve: ServeConfig::default(),
            source: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewConfig {
    /// Default window, e.g. `15m` / `1h`.
    #[serde(default = "default_since")]
    pub since: String,
    #[serde(default)]
    pub columns: Vec<Column>,
    /// Start enabled sources when the viewer opens.
    #[serde(default = "default_true")]
    pub autostart: bool,
    /// Per-level colors (CSS hex, e.g. `#3fb950`). Defaults to classic gray/green/orange/red.
    #[serde(default)]
    pub colors: LevelColors,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            since: default_since(),
            columns: Vec::new(),
            autostart: true,
            colors: LevelColors::default(),
        }
    }
}

/// Per-level log colors in `looplog view` (classic defaults: gray / green / orange / red).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelColors {
    #[serde(default = "default_color_debug")]
    pub debug: String,
    #[serde(default = "default_color_info")]
    pub info: String,
    #[serde(default = "default_color_warn")]
    pub warn: String,
    #[serde(default = "default_color_error")]
    pub error: String,
}

impl Default for LevelColors {
    fn default() -> Self {
        Self {
            debug: default_color_debug(),
            info: default_color_info(),
            warn: default_color_warn(),
            error: default_color_error(),
        }
    }
}

fn default_color_debug() -> String {
    "#8b949e".to_string()
}
fn default_color_info() -> String {
    "#3fb950".to_string()
}
fn default_color_warn() -> String {
    "#f0883e".to_string()
}
fn default_color_error() -> String {
    "#f85149".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub label: String,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServeConfig {
    #[serde(default = "default_addr")]
    pub addr: String,
    #[serde(default)]
    pub allow_lan: bool,
    #[serde(default)]
    pub allow_cidrs: Vec<String>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            addr: default_addr(),
            allow_lan: false,
            allow_cidrs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Unique id, used by `POST /api/sources/{name}/start`.
    pub name: String,
    /// Tag written to `runs.tag`, i.e. the column name in the viewer.
    pub tag: String,
    /// journald | nginx | file | cmd | logcat
    #[serde(rename = "from")]
    pub from: String,
    #[serde(default)]
    pub enabled: bool,
    pub remote: Option<String>,
    pub unit: Option<String>,
    pub path: Option<String>,
    /// Android device/emulator serial passed to `adb -s`.
    pub serial: Option<String>,
    pub since: Option<String>,
    pub lines: Option<usize>,
    #[serde(default)]
    pub sudo: bool,
    #[serde(default = "default_true")]
    pub reconnect: bool,
    pub kind: Option<String>,
    #[serde(default)]
    pub command: Vec<String>,
}

impl SourceConfig {
    pub fn to_spec(&self) -> Result<SourceSpec> {
        let lines = self.lines.unwrap_or(500);
        Ok(match self.from.as_str() {
            "cmd" => SourceSpec::Cmd {
                command: self.command.clone(),
            },
            "logcat" => SourceSpec::Logcat {
                serial: self.serial.clone(),
                lines,
            },
            "journald" => SourceSpec::Journald {
                unit: self.require(&self.unit, "--unit")?,
                remote: self.remote.clone(),
                since: self.since.clone(),
                lines,
                sudo: self.sudo,
            },
            "nginx" => SourceSpec::Nginx {
                path: self.require(&self.path, "--path")?,
                remote: self.remote.clone(),
                lines,
                sudo: self.sudo,
                follow: true,
            },
            "file" => SourceSpec::File {
                path: self.require(&self.path, "--path")?,
                lines,
                follow: true,
            },
            other => {
                return Err(Error::Message(format!(
                    "source `{}`: unknown from `{other}` (journald|nginx|file|cmd|logcat)",
                    self.name
                )))
            }
        })
    }

    /// Short human description for the config panel.
    pub fn target(&self) -> String {
        match self.from.as_str() {
            "journald" => format!(
                "{}{}",
                self.unit.clone().unwrap_or_default(),
                self.remote
                    .as_ref()
                    .map(|h| format!(" @{h}"))
                    .unwrap_or_default()
            ),
            "nginx" | "file" => format!(
                "{}{}",
                self.path.clone().unwrap_or_default(),
                self.remote
                    .as_ref()
                    .map(|h| format!(" @{h}"))
                    .unwrap_or_default()
            ),
            "cmd" => self.command.join(" "),
            "logcat" => self
                .serial
                .as_deref()
                .map(|serial| format!("adb -s {serial} logcat"))
                .unwrap_or_else(|| "adb logcat".to_string()),
            _ => String::new(),
        }
    }

    fn require(&self, value: &Option<String>, what: &str) -> Result<String> {
        value.clone().filter(|v| !v.trim().is_empty()).ok_or_else(|| {
            Error::Message(format!("source `{}` requires {what}", self.name))
        })
    }
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.source.is_empty() {
            return Ok(());
        }
        let mut seen = std::collections::HashSet::new();
        for source in &self.source {
            if source.name.trim().is_empty() {
                return Err(Error::Message("source.name must not be empty".into()));
            }
            if !seen.insert(source.name.clone()) {
                return Err(Error::Message(format!(
                    "duplicate source name `{}`",
                    source.name
                )));
            }
            if source.tag.trim().is_empty() {
                return Err(Error::Message(format!(
                    "source `{}`: tag must not be empty",
                    source.name
                )));
            }
            source.to_spec()?;
        }
        Ok(())
    }

    /// Merge CLI overrides for one run without touching the file.
    pub fn with_overrides(
        &self,
        since: Option<String>,
        columns: Option<Vec<Column>>,
    ) -> Config {
        let mut cfg = self.clone();
        if let Some(since) = since {
            cfg.view.since = since;
        }
        if let Some(columns) = columns {
            if !columns.is_empty() {
                cfg.view.columns = columns;
            }
        }
        cfg
    }

    pub fn enabled_sources(&self) -> impl Iterator<Item = &SourceConfig> {
        self.source.iter().filter(|s| s.enabled)
    }
}

pub fn load(path: &Path) -> Result<Config> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let cfg: Config = toml::from_str(&text).map_err(|e| {
                Error::Message(format!("invalid config {}: {e}", path.display()))
            })?;
            cfg.validate()?;
            Ok(cfg)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.into()),
    }
}

/// Atomic write: temp file + rename, plus a `.bak` of the previous contents.
pub fn save(path: &Path, cfg: &Config) -> Result<()> {
    cfg.validate()?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let text = toml::to_string_pretty(cfg)
        .map_err(|e| Error::Message(format!("could not serialize config: {e}")))?;
    if path.exists() {
        let _ = fs::copy(path, path.with_extension("toml.bak"));
    }
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn default_config_path() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("looplog").join("config.toml");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("looplog")
        .join("config.toml")
}

fn default_version() -> u32 {
    CONFIG_VERSION
}
fn default_since() -> String {
    "15m".to_string()
}
fn default_addr() -> String {
    "127.0.0.1:3768".to_string()
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Config {
        Config {
            source: vec![
                SourceConfig {
                    name: "api".into(),
                    tag: "picboo-api".into(),
                    from: "journald".into(),
                    enabled: true,
                    remote: Some("picboo-ali".into()),
                    unit: Some("picboo-api".into()),
                    path: None,
                    serial: None,
                    since: Some("15m".into()),
                    lines: None,
                    sudo: true,
                    reconnect: true,
                    kind: None,
                    command: vec![],
                },
                SourceConfig {
                    name: "edge".into(),
                    tag: "picboo-edge".into(),
                    from: "nginx".into(),
                    enabled: false,
                    remote: Some("picboo-ali".into()),
                    unit: None,
                    path: Some("/var/log/access.log".into()),
                    serial: None,
                    since: None,
                    lines: Some(200),
                    sudo: true,
                    reconnect: true,
                    kind: None,
                    command: vec![],
                },
            ],
            view: ViewConfig {
                since: "30m".into(),
                columns: vec![Column {
                    label: "api".into(),
                    tag: "picboo-api".into(),
                }],
                autostart: false,
                colors: LevelColors::default(),
            },
            ..Config::default()
        }
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = sample();
        save(&path, &cfg).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.source.len(), 2);
        assert_eq!(loaded.view.since, "30m");
        assert_eq!(loaded.view.columns[0].tag, "picboo-api");
        assert!(loaded.source[0].to_spec().is_ok());
        assert!(!loaded.source[1].enabled);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(cfg.view.since, "15m");
        assert!(cfg.view.autostart);
        assert!(cfg.source.is_empty());
    }

    #[test]
    fn logcat_source_carries_device_serial() {
        let mut cfg = sample();
        let source = &mut cfg.source[0];
        source.from = "logcat".into();
        source.serial = Some("emulator-5554".into());
        source.lines = Some(200);
        let spec = source.to_spec().unwrap();
        assert!(matches!(
            spec,
            SourceSpec::Logcat {
                serial: Some(serial),
                lines: 200
            } if serial == "emulator-5554"
        ));
        assert_eq!(source.target(), "adb -s emulator-5554 logcat");
    }

    #[test]
    fn rejects_duplicate_names_and_missing_fields() {
        let mut cfg = sample();
        cfg.source[1].name = "api".into();
        assert!(cfg.validate().is_err());

        let mut cfg = sample();
        cfg.source[0].unit = None;
        let err = cfg.validate().unwrap_err().to_string();
        assert!(err.contains("requires --unit"), "{err}");
    }

    #[test]
    fn writes_backup_before_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        save(&path, &Config::default()).unwrap();
        let mut cfg = Config::default();
        cfg.view.since = "5m".into();
        save(&path, &cfg).unwrap();
        assert!(path.with_extension("toml.bak").exists());
        assert_eq!(load(&path).unwrap().view.since, "5m");
    }

    #[test]
    fn cli_overrides_do_not_touch_source_list() {
        let cfg = sample();
        let merged = cfg.with_overrides(
            Some("1h".into()),
            Some(vec![Column {
                label: "x".into(),
                tag: "y".into(),
            }]),
        );
        assert_eq!(merged.view.since, "1h");
        assert_eq!(merged.view.columns.len(), 1);
        assert_eq!(merged.source.len(), 2);
    }

    #[test]
    fn level_colors_default_to_classic_and_round_trip() {
        let def = Config::default();
        assert_eq!(def.view.colors.debug, "#8b949e");
        assert_eq!(def.view.colors.info, "#3fb950");
        assert_eq!(def.view.colors.warn, "#f0883e");
        assert_eq!(def.view.colors.error, "#f85149");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("colors.toml");
        let mut cfg = Config::default();
        cfg.view.colors.info = "#22cc66".into();
        save(&path, &cfg).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.view.colors.info, "#22cc66");
        assert_eq!(loaded.view.colors.warn, "#f0883e");
    }
}
