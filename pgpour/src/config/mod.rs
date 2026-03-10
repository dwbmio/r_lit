mod model;

pub use model::*;
pub use model::validate_kafka_targets;

/// Pre-parse `--config` / `CONFIG_PATH` before clap runs, so YAML
/// values become env vars that clap's `env = "..."` can resolve.
/// Returns `(config_path, FileConfig)` when a config file is present.
pub fn preload() -> Result<Option<(String, FileConfig)>, Box<dyn std::error::Error>> {
    let path = std::env::args()
        .zip(std::env::args().skip(1))
        .find(|(a, _)| a == "--config")
        .map(|(_, v)| v)
        .or_else(|| std::env::var("CONFIG_PATH").ok());

    if let Some(p) = path {
        let cfg = FileConfig::load(&p)?;
        cfg.apply_to_env();
        return Ok(Some((p, cfg)));
    }
    Ok(None)
}
