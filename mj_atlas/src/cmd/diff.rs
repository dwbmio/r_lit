//! `mj_atlas diff` — semantic diff between two manifests.
//!
//! Categories per sprite:
//!   - `added`     present only in B
//!   - `removed`   present only in A
//!   - `pixels`    same name, different content_hash, same trimmed dims
//!   - `resized`   same name, different content_hash, different trimmed dims
//!   - `moved`     same name + content_hash but different (x, y, rotated, atlas)
//!                 — this signals a UV-stability break (a full repack happened)
//!   - `tags`      same content but tags / attribution / source_url changed
//!   - `unchanged` everything matches
//!
//! Atlas-level: PNG image_hash matches? layout/size changed? options_hash
//! mismatch (which guarantees a full repack happened between A and B)?

use crate::error::Result;
use crate::pack::manifest::{self, Manifest, SpriteEntry};
use std::path::Path;

pub fn run(a: &Path, b: &Path, json: bool) -> Result<()> {
    let path_a = manifest::resolve_manifest_path(a)?;
    let path_b = manifest::resolve_manifest_path(b)?;
    let manifest_a = Manifest::try_load(&path_a)?
        .ok_or_else(|| crate::error::AppError::Custom(format!("{} not loadable", path_a.display())))?;
    let manifest_b = Manifest::try_load(&path_b)?
        .ok_or_else(|| crate::error::AppError::Custom(format!("{} not loadable", path_b.display())))?;

    let report = compute(&manifest_a, &manifest_b);

    if json {
        println!("{}", serde_json::to_string_pretty(&report.to_json())?);
    } else {
        report.print_human(&path_a, &path_b);
    }
    Ok(())
}

#[derive(Debug)]
pub struct DiffReport {
    pub options_hash_changed: bool,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub pixel_change: Vec<String>,
    pub resized: Vec<ResizedSprite>,
    pub moved: Vec<MovedSprite>,
    pub tags_changed: Vec<TagDiff>,
    pub unchanged: usize,
    pub uv_stable: bool,
    pub atlas_size_changes: Vec<AtlasSizeChange>,
}

#[derive(Debug)]
pub struct ResizedSprite {
    pub name: String,
    pub from: [u32; 2],
    pub to: [u32; 2],
}

#[derive(Debug)]
pub struct MovedSprite {
    pub name: String,
    pub from_atlas: usize,
    pub to_atlas: usize,
    pub from_pos: [u32; 2],
    pub to_pos: [u32; 2],
    pub from_rotated: bool,
    pub to_rotated: bool,
}

#[derive(Debug)]
pub struct TagDiff {
    pub name: String,
    pub tags_added: Vec<String>,
    pub tags_removed: Vec<String>,
    pub attribution_changed: bool,
    pub source_url_changed: bool,
}

#[derive(Debug)]
pub struct AtlasSizeChange {
    pub atlas_idx: usize,
    pub from: [u32; 2],
    pub to: [u32; 2],
}

pub fn compute(a: &Manifest, b: &Manifest) -> DiffReport {
    let mut added: Vec<String> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut pixel_change: Vec<String> = Vec::new();
    let mut resized: Vec<ResizedSprite> = Vec::new();
    let mut moved: Vec<MovedSprite> = Vec::new();
    let mut tags_changed: Vec<TagDiff> = Vec::new();
    let mut unchanged = 0usize;

    for name in a.sprites.keys() {
        if !b.sprites.contains_key(name) {
            removed.push(name.clone());
        }
    }

    for (name, entry_b) in &b.sprites {
        let entry_a = match a.sprites.get(name) {
            Some(e) => e,
            None => {
                added.push(name.clone());
                continue;
            }
        };

        // Aliases inherit content; classify them as unchanged unless the
        // canonical pointer itself changed.
        if entry_a.content_hash != entry_b.content_hash {
            if entry_a.trimmed_size != entry_b.trimmed_size {
                resized.push(ResizedSprite {
                    name: name.clone(),
                    from: entry_a.trimmed_size,
                    to: entry_b.trimmed_size,
                });
            } else {
                pixel_change.push(name.clone());
            }
            continue;
        }

        // Same content — check UV stability.
        let position_changed = entry_a.atlas_idx != entry_b.atlas_idx
            || entry_a.content_x != entry_b.content_x
            || entry_a.content_y != entry_b.content_y
            || entry_a.rotated != entry_b.rotated;
        if position_changed {
            moved.push(MovedSprite {
                name: name.clone(),
                from_atlas: entry_a.atlas_idx,
                to_atlas: entry_b.atlas_idx,
                from_pos: [entry_a.content_x, entry_a.content_y],
                to_pos: [entry_b.content_x, entry_b.content_y],
                from_rotated: entry_a.rotated,
                to_rotated: entry_b.rotated,
            });
            // Note: a `moved` sprite implicitly means UV broke for that sprite,
            // even though the pixels are identical.
        }

        if let Some(td) = tag_diff(name, entry_a, entry_b) {
            tags_changed.push(td);
        }

        if !position_changed {
            unchanged += 1;
        }
    }

    let uv_stable = moved.is_empty();

    let mut atlas_size_changes = Vec::new();
    let common_atlases = a.atlases.len().min(b.atlases.len());
    for i in 0..common_atlases {
        let aa = &a.atlases[i];
        let bb = &b.atlases[i];
        if (aa.width, aa.height) != (bb.width, bb.height) {
            atlas_size_changes.push(AtlasSizeChange {
                atlas_idx: i,
                from: [aa.width, aa.height],
                to: [bb.width, bb.height],
            });
        }
    }

    added.sort();
    removed.sort();
    pixel_change.sort();
    resized.sort_by(|a, b| a.name.cmp(&b.name));
    moved.sort_by(|a, b| a.name.cmp(&b.name));
    tags_changed.sort_by(|a, b| a.name.cmp(&b.name));

    DiffReport {
        options_hash_changed: a.options_hash != b.options_hash,
        added,
        removed,
        pixel_change,
        resized,
        moved,
        tags_changed,
        unchanged,
        uv_stable,
        atlas_size_changes,
    }
}

fn tag_diff(name: &str, a: &SpriteEntry, b: &SpriteEntry) -> Option<TagDiff> {
    let set_a: std::collections::HashSet<&str> = a.tags.iter().map(|s| s.as_str()).collect();
    let set_b: std::collections::HashSet<&str> = b.tags.iter().map(|s| s.as_str()).collect();
    let added: Vec<String> = set_b
        .difference(&set_a)
        .map(|s| s.to_string())
        .collect();
    let removed: Vec<String> = set_a
        .difference(&set_b)
        .map(|s| s.to_string())
        .collect();
    let attr_changed = a.attribution != b.attribution;
    let url_changed = a.source_url != b.source_url;

    if added.is_empty() && removed.is_empty() && !attr_changed && !url_changed {
        None
    } else {
        let mut added = added;
        let mut removed = removed;
        added.sort();
        removed.sort();
        Some(TagDiff {
            name: name.to_string(),
            tags_added: added,
            tags_removed: removed,
            attribution_changed: attr_changed,
            source_url_changed: url_changed,
        })
    }
}

impl DiffReport {
    fn print_human(&self, a: &Path, b: &Path) {
        println!("Diff:");
        println!("  A = {}", a.display());
        println!("  B = {}", b.display());
        println!();
        println!(
            "  Options hash:    {}",
            if self.options_hash_changed {
                "CHANGED — full repack happened between A and B"
            } else {
                "match (incremental was effective)"
            }
        );
        println!(
            "  UV stability:    {}",
            if self.uv_stable {
                "STABLE — every unchanged sprite kept (x, y, rotated)"
            } else {
                "BROKEN — some sprites were relocated (see `moved`)"
            }
        );
        println!();
        println!("  Added:           {}", self.added.len());
        for n in &self.added {
            println!("    + {}", n);
        }
        println!("  Removed:         {}", self.removed.len());
        for n in &self.removed {
            println!("    - {}", n);
        }
        println!("  Pixel-changed:   {}", self.pixel_change.len());
        for n in &self.pixel_change {
            println!("    ~ {}", n);
        }
        println!("  Resized:         {}", self.resized.len());
        for r in &self.resized {
            println!(
                "    ~ {}  ({}x{} → {}x{})",
                r.name, r.from[0], r.from[1], r.to[0], r.to[1]
            );
        }
        println!("  Moved (UV break): {}", self.moved.len());
        for m in &self.moved {
            println!(
                "    ↳ {}  atlas={}({},{}{}) → atlas={}({},{}{})",
                m.name,
                m.from_atlas,
                m.from_pos[0],
                m.from_pos[1],
                if m.from_rotated { ",R" } else { "" },
                m.to_atlas,
                m.to_pos[0],
                m.to_pos[1],
                if m.to_rotated { ",R" } else { "" }
            );
        }
        println!("  Tag changes:     {}", self.tags_changed.len());
        for td in &self.tags_changed {
            let mut bits: Vec<String> = Vec::new();
            if !td.tags_added.is_empty() {
                bits.push(format!("+tags: {}", td.tags_added.join(",")));
            }
            if !td.tags_removed.is_empty() {
                bits.push(format!("-tags: {}", td.tags_removed.join(",")));
            }
            if td.attribution_changed {
                bits.push("attribution".into());
            }
            if td.source_url_changed {
                bits.push("source_url".into());
            }
            println!("    @ {}  {}", td.name, bits.join("  "));
        }
        println!("  Unchanged:       {}", self.unchanged);
        if !self.atlas_size_changes.is_empty() {
            println!();
            println!("  Atlas size changes:");
            for c in &self.atlas_size_changes {
                println!(
                    "    [{}] {}x{} → {}x{}",
                    c.atlas_idx, c.from[0], c.from[1], c.to[0], c.to[1]
                );
            }
        }
    }

    fn to_json(&self) -> serde_json::Value {
        let mut tag_changes: Vec<serde_json::Value> = Vec::new();
        for td in &self.tags_changed {
            tag_changes.push(serde_json::json!({
                "name": td.name,
                "tags_added": td.tags_added,
                "tags_removed": td.tags_removed,
                "attribution_changed": td.attribution_changed,
                "source_url_changed": td.source_url_changed,
            }));
        }
        let mut moved: Vec<serde_json::Value> = Vec::new();
        for m in &self.moved {
            moved.push(serde_json::json!({
                "name": m.name,
                "from": {"atlas": m.from_atlas, "x": m.from_pos[0], "y": m.from_pos[1], "rotated": m.from_rotated},
                "to":   {"atlas": m.to_atlas,   "x": m.to_pos[0],   "y": m.to_pos[1],   "rotated": m.to_rotated},
            }));
        }
        let mut resized: Vec<serde_json::Value> = Vec::new();
        for r in &self.resized {
            resized.push(serde_json::json!({
                "name": r.name,
                "from": {"w": r.from[0], "h": r.from[1]},
                "to":   {"w": r.to[0],   "h": r.to[1]},
            }));
        }
        let mut atlas_size: Vec<serde_json::Value> = Vec::new();
        for c in &self.atlas_size_changes {
            atlas_size.push(serde_json::json!({
                "atlas_idx": c.atlas_idx,
                "from": {"w": c.from[0], "h": c.from[1]},
                "to":   {"w": c.to[0],   "h": c.to[1]},
            }));
        }

        serde_json::json!({
            "options_hash_changed": self.options_hash_changed,
            "uv_stable": self.uv_stable,
            "added": self.added,
            "removed": self.removed,
            "pixel_change": self.pixel_change,
            "resized": resized,
            "moved": moved,
            "tags_changed": tag_changes,
            "unchanged": self.unchanged,
            "atlas_size_changes": atlas_size,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::manifest::{AtlasEntry, MANIFEST_VERSION};
    use std::collections::BTreeMap;

    fn entry(name: &str, hash: &str, atlas: usize, x: u32, y: u32, dims: [u32; 2]) -> SpriteEntry {
        SpriteEntry {
            rel_path: name.into(),
            file_size: 0, mtime: 0, content_hash: hash.into(),
            trim_offset: [0, 0], trimmed_size: dims, source_size: dims,
            polygon_hash: None, atlas_idx: atlas, content_x: x, content_y: y,
            rotated: false, alias_of: None,
            tags: vec![], attribution: None, source_url: None,
        }
    }

    fn manifest_with(entries: Vec<SpriteEntry>) -> Manifest {
        let mut sprites = BTreeMap::new();
        for e in entries {
            sprites.insert(e.rel_path.clone(), e);
        }
        Manifest {
            version: MANIFEST_VERSION,
            tool: "test".into(),
            options_hash: "h".into(),
            input_root: "/".into(),
            sprites,
            atlases: vec![AtlasEntry {
                image_filename: "atlas.png".into(),
                data_filename: "atlas".into(),
                width: 256, height: 256, image_hash: "".into(), format: "json".into(),
                used_rects: vec![], free_rects: vec![],
            }],
        }
    }

    #[test]
    fn detects_added_and_removed() {
        let a = manifest_with(vec![entry("x.png", "h1", 0, 0, 0, [10, 10])]);
        let b = manifest_with(vec![entry("y.png", "h2", 0, 0, 0, [10, 10])]);
        let r = compute(&a, &b);
        assert_eq!(r.added, vec!["y.png".to_string()]);
        assert_eq!(r.removed, vec!["x.png".to_string()]);
    }

    #[test]
    fn pixel_change_keeps_same_dims() {
        let a = manifest_with(vec![entry("x.png", "old", 0, 0, 0, [10, 10])]);
        let b = manifest_with(vec![entry("x.png", "new", 0, 0, 0, [10, 10])]);
        let r = compute(&a, &b);
        assert_eq!(r.pixel_change, vec!["x.png".to_string()]);
        assert!(r.resized.is_empty());
        assert!(r.uv_stable, "in-place pixel change keeps UV stable");
    }

    #[test]
    fn moved_breaks_uv_stability() {
        let a = manifest_with(vec![entry("x.png", "h", 0, 0, 0, [10, 10])]);
        let b = manifest_with(vec![entry("x.png", "h", 0, 64, 32, [10, 10])]);
        let r = compute(&a, &b);
        assert_eq!(r.moved.len(), 1);
        assert!(!r.uv_stable);
    }

    #[test]
    fn resized_classified_as_resized_not_pixel_change() {
        let a = manifest_with(vec![entry("x.png", "h1", 0, 0, 0, [10, 10])]);
        let b = manifest_with(vec![entry("x.png", "h2", 0, 0, 0, [12, 14])]);
        let r = compute(&a, &b);
        assert!(r.pixel_change.is_empty());
        assert_eq!(r.resized.len(), 1);
        assert_eq!(r.resized[0].from, [10, 10]);
        assert_eq!(r.resized[0].to, [12, 14]);
    }

    #[test]
    fn tags_only_change() {
        let mut a_e = entry("x.png", "h", 0, 0, 0, [10, 10]);
        a_e.tags = vec!["ui".into()];
        let mut b_e = entry("x.png", "h", 0, 0, 0, [10, 10]);
        b_e.tags = vec!["ui".into(), "icon".into()];
        b_e.attribution = Some("CC0".into());
        let r = compute(&manifest_with(vec![a_e]), &manifest_with(vec![b_e]));
        assert_eq!(r.tags_changed.len(), 1);
        assert_eq!(r.tags_changed[0].tags_added, vec!["icon".to_string()]);
        assert!(r.tags_changed[0].attribution_changed);
        assert_eq!(r.unchanged, 1);
    }
}
