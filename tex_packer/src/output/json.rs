use crate::error::Result;
use crate::pack::{AtlasResult, PackOptions};
use serde_json::{json, Map, Value};

/// TexturePacker-compatible JSON Hash format.
pub fn to_json_hash(atlas: &AtlasResult, _opts: &PackOptions) -> Result<String> {
    let mut frames = Map::new();

    for sprite in &atlas.sprites {
        let mut entry = json!({
            "frame": {
                "x": sprite.x,
                "y": sprite.y,
                "w": sprite.w,
                "h": sprite.h
            },
            "rotated": sprite.rotated,
            "trimmed": sprite.trimmed,
            "spriteSourceSize": {
                "x": sprite.trim_offset_x,
                "y": sprite.trim_offset_y,
                "w": sprite.w,
                "h": sprite.h
            },
            "sourceSize": {
                "w": sprite.source_w,
                "h": sprite.source_h
            }
        });

        if let Some(alias) = &sprite.alias_of {
            entry.as_object_mut().unwrap().insert("alias".to_string(), json!(alias));
        }

        // Polygon mesh data
        if let (Some(verts), Some(uvs), Some(tris)) =
            (&sprite.vertices, &sprite.vertices_uv, &sprite.triangles)
        {
            let obj = entry.as_object_mut().unwrap();
            obj.insert("vertices".to_string(), json!(verts));
            obj.insert("verticesUV".to_string(), json!(uvs));
            obj.insert("triangles".to_string(), json!(tris));
        }

        frames.insert(sprite.name.clone(), entry);
    }

    let image_name = atlas
        .image_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut root = json!({
        "frames": Value::Object(frames),
        "meta": {
            "app": "tex_packer",
            "version": env!("CARGO_PKG_VERSION"),
            "image": image_name,
            "format": "RGBA8888",
            "size": {
                "w": atlas.width,
                "h": atlas.height
            },
            "scale": 1
        }
    });

    // Add animations if any
    if !atlas.animations.is_empty() {
        root.as_object_mut()
            .expect("root is object")
            .insert("animations".to_string(), json!(atlas.animations));
    }

    let output = serde_json::to_string_pretty(&root)?;
    Ok(output)
}

/// TexturePacker-compatible JSON Array format.
pub fn to_json_array(atlas: &AtlasResult, _opts: &PackOptions) -> Result<String> {
    let frames: Vec<Value> = atlas
        .sprites
        .iter()
        .map(|sprite| {
            let mut entry = json!({
                "filename": sprite.name,
                "frame": {
                    "x": sprite.x,
                    "y": sprite.y,
                    "w": sprite.w,
                    "h": sprite.h
                },
                "rotated": sprite.rotated,
                "trimmed": sprite.trimmed,
                "spriteSourceSize": {
                    "x": sprite.trim_offset_x,
                    "y": sprite.trim_offset_y,
                    "w": sprite.w,
                    "h": sprite.h
                },
                "sourceSize": {
                    "w": sprite.source_w,
                    "h": sprite.source_h
                }
            });
            if let (Some(verts), Some(uvs), Some(tris)) =
                (&sprite.vertices, &sprite.vertices_uv, &sprite.triangles)
            {
                let obj = entry.as_object_mut().unwrap();
                obj.insert("vertices".to_string(), json!(verts));
                obj.insert("verticesUV".to_string(), json!(uvs));
                obj.insert("triangles".to_string(), json!(tris));
            }
            entry
        })
        .collect();

    let image_name = atlas
        .image_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut root = json!({
        "frames": frames,
        "meta": {
            "app": "tex_packer",
            "version": env!("CARGO_PKG_VERSION"),
            "image": image_name,
            "format": "RGBA8888",
            "size": {
                "w": atlas.width,
                "h": atlas.height
            },
            "scale": 1
        }
    });

    if !atlas.animations.is_empty() {
        root.as_object_mut()
            .expect("root is object")
            .insert("animations".to_string(), json!(atlas.animations));
    }

    let output = serde_json::to_string_pretty(&root)?;
    Ok(output)
}
