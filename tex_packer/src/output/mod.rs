pub mod godot;
pub mod json;

use crate::error::Result;
use crate::pack::{AtlasResult, PackOptions};

#[derive(Debug, Clone, Copy)]
pub enum Format {
    JsonHash,
    JsonArray,
    /// Godot .tpsheet (TexturePacker plugin compatible JSON)
    GodotTpsheet,
    /// Godot native .tres AtlasTexture (zero plugin)
    GodotTres,
}

pub fn write_output(atlas: &AtlasResult, format: Format, opts: &PackOptions) -> Result<()> {
    match format {
        Format::JsonHash => {
            let path = atlas.data_path.with_extension("json");
            let content = json::to_json_hash(atlas, opts)?;
            std::fs::write(&path, content)?;
            log::info!("Saved JSON (hash): {}", path.display());
        }
        Format::JsonArray => {
            let path = atlas.data_path.with_extension("json");
            let content = json::to_json_array(atlas, opts)?;
            std::fs::write(&path, content)?;
            log::info!("Saved JSON (array): {}", path.display());
        }
        Format::GodotTpsheet => {
            let path = atlas.data_path.with_extension("tpsheet");
            let content = godot::to_tpsheet(atlas, opts)?;
            std::fs::write(&path, content)?;
            log::info!("Saved Godot .tpsheet: {}", path.display());
        }
        Format::GodotTres => {
            godot::write_tres_bundle(atlas, opts)?;
        }
    }
    Ok(())
}
