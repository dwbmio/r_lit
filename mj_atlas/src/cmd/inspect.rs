//! `mj_atlas inspect` — pretty-print a manifest for humans, or emit JSON.
//!
//! Resolves the manifest from any path the user might reasonably point at:
//! the manifest itself, the atlas PNG, the JSON/.tpsheet/.tres sidecar, or
//! the directory containing them.

use crate::error::Result;
use crate::pack::manifest::{self, Manifest};
use std::collections::BTreeMap;
use std::path::Path;

pub fn run(input: &Path, json: bool) -> Result<()> {
    let manifest_path = manifest::resolve_manifest_path(input)?;
    let m = match Manifest::try_load(&manifest_path)? {
        Some(m) => m,
        None => {
            return Err(crate::error::AppError::Custom(format!(
                "manifest at {} is empty or version-mismatched",
                manifest_path.display()
            )));
        }
    };

    if json {
        print_json(&manifest_path, &m)
    } else {
        print_human(&manifest_path, &m)
    }
}

fn print_human(path: &Path, m: &Manifest) -> Result<()> {
    let total_sprites = m.sprites.len();
    let alias_count = m.sprites.values().filter(|e| e.alias_of.is_some()).count();
    let unique_sprites = total_sprites - alias_count;
    let total_atlas_pixels: u64 = m
        .atlases
        .iter()
        .map(|a| a.width as u64 * a.height as u64)
        .sum();
    let used_pixels: u64 = m
        .atlases
        .iter()
        .flat_map(|a| a.used_rects.iter())
        .map(|r| r.w as u64 * r.h as u64)
        .sum();
    let occupancy = if total_atlas_pixels > 0 {
        100.0 * used_pixels as f64 / total_atlas_pixels as f64
    } else {
        0.0
    };

    println!("Manifest: {}", path.display());
    println!("Tool:     {}", m.tool);
    println!("Inputs:   {}", m.input_root);
    println!(
        "Sprites:  {}  ({} unique, {} aliases)",
        total_sprites, unique_sprites, alias_count
    );
    println!(
        "Atlases:  {}  ({:.1}% occupancy across {} atlas px)",
        m.atlases.len(),
        occupancy,
        total_atlas_pixels
    );
    println!("Options:  options_hash = {}", short(&m.options_hash));
    println!();

    // Per-atlas breakdown.
    for (idx, a) in m.atlases.iter().enumerate() {
        let atlas_used: u64 = a.used_rects.iter().map(|r| r.w as u64 * r.h as u64).sum();
        let atlas_total = a.width as u64 * a.height as u64;
        let atlas_occ = if atlas_total > 0 {
            100.0 * atlas_used as f64 / atlas_total as f64
        } else {
            0.0
        };
        println!(
            "[{}] {}  {}x{}  {} sprites  occupancy {:.1}%  {} free rects  format={}",
            idx,
            a.image_filename,
            a.width,
            a.height,
            a.used_rects.len(),
            atlas_occ,
            a.free_rects.len(),
            a.format
        );
    }

    // Tag aggregation — useful for "what's in this atlas semantically".
    let mut tag_counts: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in m.sprites.values() {
        for tag in &entry.tags {
            *tag_counts.entry(tag.as_str()).or_default() += 1;
        }
    }
    if !tag_counts.is_empty() {
        println!();
        println!("Tags:");
        for (tag, n) in tag_counts {
            println!("  {:>4}  {}", n, tag);
        }
    }

    // Animation groups (recomputed-style summary).
    if total_sprites <= 64 {
        // Avoid spamming for huge atlases — they should pipe through --json.
        println!();
        println!("Sprites:");
        for (name, e) in &m.sprites {
            let tag_str = if e.tags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", e.tags.join(","))
            };
            let alias = match &e.alias_of {
                Some(canonical) => format!("  → {}", canonical),
                None => String::new(),
            };
            let rot = if e.rotated { " R" } else { "" };
            println!(
                "  {}  atlas={} pos=({},{}) size={}x{}{}{}{}",
                name,
                e.atlas_idx,
                e.content_x,
                e.content_y,
                e.trimmed_size[0],
                e.trimmed_size[1],
                rot,
                tag_str,
                alias
            );
        }
    } else {
        println!();
        println!("({} sprites — pipe to --json or `mj_atlas inspect --json | jq` for full list)", total_sprites);
    }

    Ok(())
}

fn print_json(path: &Path, m: &Manifest) -> Result<()> {
    // Re-emit the manifest verbatim plus a small `summary` block for
    // dashboard consumption. This keeps the surface stable: tools that just
    // want a fast overview parse `summary`; tools that want the full layout
    // parse the rest.
    let total_sprites = m.sprites.len();
    let alias_count = m.sprites.values().filter(|e| e.alias_of.is_some()).count();
    let total_atlas_pixels: u64 = m
        .atlases
        .iter()
        .map(|a| a.width as u64 * a.height as u64)
        .sum();
    let used_pixels: u64 = m
        .atlases
        .iter()
        .flat_map(|a| a.used_rects.iter())
        .map(|r| r.w as u64 * r.h as u64)
        .sum();

    let mut tag_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in m.sprites.values() {
        for tag in &entry.tags {
            *tag_counts.entry(tag.clone()).or_default() += 1;
        }
    }

    let summary = serde_json::json!({
        "manifest_path": path.display().to_string(),
        "tool": m.tool,
        "input_root": m.input_root,
        "options_hash": m.options_hash,
        "atlases": m.atlases.len(),
        "sprites_total": total_sprites,
        "sprites_unique": total_sprites - alias_count,
        "aliases": alias_count,
        "atlas_pixels": total_atlas_pixels,
        "used_pixels": used_pixels,
        "occupancy": if total_atlas_pixels > 0 {
            used_pixels as f64 / total_atlas_pixels as f64
        } else { 0.0 },
        "tags": tag_counts,
    });

    let combined = serde_json::json!({
        "summary": summary,
        "manifest": m,
    });
    println!("{}", serde_json::to_string_pretty(&combined)?);
    Ok(())
}

fn short(hash: &str) -> String {
    if hash.len() > 12 {
        format!("{}…", &hash[..12])
    } else {
        hash.to_string()
    }
}
