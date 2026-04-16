use crate::error::Result;
use crate::pack::{AtlasResult, PackOptions};
use serde_json::json;
use std::path::Path;

/// Generate `.tpsheet` format — Godot TexturePacker plugin compatible.
/// This is JSON that the official texturepacker-godot-plugin can directly import,
/// auto-generating `.tres` AtlasTexture resources for every sprite.
pub fn to_tpsheet(atlas: &AtlasResult, _opts: &PackOptions) -> Result<String> {
    let image_name = file_name(&atlas.image_path);

    let sprites: Vec<serde_json::Value> = atlas
        .sprites
        .iter()
        .map(|s| {
            // margin follows TexturePacker Godot exporter convention:
            //   margin.x = trim offset x (sourceRect.x)
            //   margin.y = trim offset y (sourceRect.y)
            //   margin.w = sourceSize.w - frame.w
            //   margin.h = sourceSize.h - frame.h
            let margin_w = s.source_w.saturating_sub(s.w);
            let margin_h = s.source_h.saturating_sub(s.h);

            json!({
                "filename": s.name,
                "region": {
                    "x": s.x,
                    "y": s.y,
                    "w": s.w,
                    "h": s.h
                },
                "margin": {
                    "x": s.trim_offset_x,
                    "y": s.trim_offset_y,
                    "w": margin_w,
                    "h": margin_h
                },
                "rotated": s.rotated
            })
        })
        .collect();

    let root = json!({
        "textures": [{
            "image": image_name,
            "size": {
                "w": atlas.width,
                "h": atlas.height
            },
            "sprites": sprites
        }],
        "meta": {
            "app": "tex_packer",
            "version": env!("CARGO_PKG_VERSION")
        }
    });

    Ok(serde_json::to_string_pretty(&root)?)
}

/// Generate Godot native `.tres` AtlasTexture resources.
/// Creates one `.tres` file per sprite — zero plugin needed.
/// Also generates a `SpriteFrames` `.tres` for each animation group.
pub fn write_tres_bundle(atlas: &AtlasResult, opts: &PackOptions) -> Result<Vec<String>> {
    let sprites_dir = opts.output_dir.join(format!("{}.sprites", opts.output_name));
    std::fs::create_dir_all(&sprites_dir)?;

    // The atlas PNG path relative to the Godot project — user places files in res://
    // We write a relative reference; they'll adjust the res:// prefix in-engine.
    let image_name = file_name(&atlas.image_path);

    let mut created_files = Vec::new();

    for sprite in &atlas.sprites {
        // Sanitize sprite name for filesystem: replace / with __
        let safe_name = sprite
            .name
            .replace('/', "__")
            .replace('\\', "__");
        let safe_name = safe_name
            .strip_suffix(".png")
            .or_else(|| safe_name.strip_suffix(".jpg"))
            .or_else(|| safe_name.strip_suffix(".jpeg"))
            .or_else(|| safe_name.strip_suffix(".bmp"))
            .or_else(|| safe_name.strip_suffix(".gif"))
            .or_else(|| safe_name.strip_suffix(".webp"))
            .or_else(|| safe_name.strip_suffix(".tga"))
            .unwrap_or(&safe_name);

        let tres_path = sprites_dir.join(format!("{}.tres", safe_name));

        // margin = Rect2(offset_x, offset_y, untrimmed_w - trimmed_w, untrimmed_h - trimmed_h)
        let margin_w = sprite.source_w.saturating_sub(sprite.w);
        let margin_h = sprite.source_h.saturating_sub(sprite.h);

        let tres_content = format!(
            r#"[gd_resource type="AtlasTexture" load_steps=2 format=3]

[ext_resource type="Texture2D" path="res://{image}" id="1"]

[resource]
atlas = ExtResource("1")
region = Rect2({rx}, {ry}, {rw}, {rh})
margin = Rect2({mx}, {my}, {mw}, {mh})
"#,
            image = image_name,
            rx = sprite.x,
            ry = sprite.y,
            rw = sprite.w,
            rh = sprite.h,
            mx = sprite.trim_offset_x,
            my = sprite.trim_offset_y,
            mw = margin_w,
            mh = margin_h,
        );

        std::fs::write(&tres_path, tres_content)?;
        created_files.push(tres_path.display().to_string());
    }

    // Generate SpriteFrames .tres for each animation group
    for (anim_name, frames) in &atlas.animations {
        let sf_path = sprites_dir.join(format!("{}_frames.tres", anim_name));

        let mut sf = String::new();

        // Count sub-resources: 1 ext_resource (atlas image) + N sub_resources (AtlasTexture per frame)
        let load_steps = 2 + frames.len();
        sf.push_str(&format!(
            "[gd_resource type=\"SpriteFrames\" load_steps={} format=3]\n\n",
            load_steps
        ));
        sf.push_str(&format!(
            "[ext_resource type=\"Texture2D\" path=\"res://{}\" id=\"1\"]\n\n",
            image_name
        ));

        // Sub-resources for each frame
        for (i, frame_name) in frames.iter().enumerate() {
            let sprite = atlas.sprites.iter().find(|s| s.name == *frame_name);
            if let Some(s) = sprite {
                let margin_w = s.source_w.saturating_sub(s.w);
                let margin_h = s.source_h.saturating_sub(s.h);
                sf.push_str(&format!(
                    "[sub_resource type=\"AtlasTexture\" id=\"{id}\"]\n\
                     atlas = ExtResource(\"1\")\n\
                     region = Rect2({rx}, {ry}, {rw}, {rh})\n\
                     margin = Rect2({mx}, {my}, {mw}, {mh})\n\n",
                    id = i + 2,
                    rx = s.x,
                    ry = s.y,
                    rw = s.w,
                    rh = s.h,
                    mx = s.trim_offset_x,
                    my = s.trim_offset_y,
                    mw = margin_w,
                    mh = margin_h,
                ));
            }
        }

        // Main resource: SpriteFrames with one animation
        sf.push_str("[resource]\n");
        sf.push_str("animations = [{\n");
        sf.push_str(&format!("\"name\": &\"{}\",\n", anim_name));
        sf.push_str("\"speed\": 10.0,\n");
        sf.push_str("\"loop\": true,\n");
        sf.push_str("\"frames\": [");

        for (i, _) in frames.iter().enumerate() {
            if i > 0 {
                sf.push_str(", ");
            }
            sf.push_str(&format!(
                "{{\n\"texture\": SubResource(\"{}\"),\n\"duration\": 1.0\n}}",
                i + 2
            ));
        }

        sf.push_str("]\n}]\n");

        std::fs::write(&sf_path, sf)?;
        created_files.push(sf_path.display().to_string());
    }

    log::info!(
        "Saved {} Godot .tres resources to {}",
        created_files.len(),
        sprites_dir.display()
    );
    Ok(created_files)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default()
}
