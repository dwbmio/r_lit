//! `mj_atlas tag` — read/write the user-editable metadata on a sprite entry.
//!
//! This never repacks. It just edits the manifest sidecar:
//!   - `--add foo,bar`         add tags (deduplicated, sorted)
//!   - `--remove foo`          drop tags
//!   - `--clear`               wipe all tags
//!   - `--set-attribution S`   set free-form attribution string
//!   - `--clear-attribution`   wipe attribution
//!   - `--set-source-url S`    set free-form source URL string
//!   - `--clear-source-url`    wipe source url
//!   - `--list`                read-only: print current tags + metadata
//!
//! When `<sprite>` is omitted, the operation applies to ALL sprites in the
//! manifest (useful for blanket attribution).

use crate::error::{AppError, Result};
use crate::pack::manifest::{self, Manifest, SpriteEntry};
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct TagOps {
    pub add: Vec<String>,
    pub remove: Vec<String>,
    pub clear: bool,
    pub set_attribution: Option<String>,
    pub clear_attribution: bool,
    pub set_source_url: Option<String>,
    pub clear_source_url: bool,
    pub list_only: bool,
}

impl TagOps {
    pub fn is_writeful(&self) -> bool {
        !self.add.is_empty()
            || !self.remove.is_empty()
            || self.clear
            || self.set_attribution.is_some()
            || self.clear_attribution
            || self.set_source_url.is_some()
            || self.clear_source_url
    }
}

pub fn run(input: &Path, sprite: Option<&str>, ops: TagOps, json: bool) -> Result<()> {
    let manifest_path = manifest::resolve_manifest_path(input)?;
    let mut m = Manifest::try_load(&manifest_path)?
        .ok_or_else(|| AppError::Custom(format!("{} not loadable", manifest_path.display())))?;

    // Resolve which sprites we're operating on.
    let target_names: Vec<String> = match sprite {
        Some(name) => {
            if !m.sprites.contains_key(name) {
                return Err(AppError::Custom(format!(
                    "no sprite named '{}' in manifest (run `mj_atlas inspect` to see the list)",
                    name
                )));
            }
            vec![name.to_string()]
        }
        None => m.sprites.keys().cloned().collect(),
    };

    // List-only is read-only; doesn't need ops.
    if ops.list_only || !ops.is_writeful() {
        if json {
            print_list_json(&m, &target_names)?;
        } else {
            print_list_human(&manifest_path, &m, &target_names);
        }
        return Ok(());
    }

    // Apply ops.
    let mut summary = ChangeSummary::default();
    for name in &target_names {
        if let Some(entry) = m.sprites.get_mut(name) {
            apply_ops(entry, &ops, &mut summary, name);
        }
    }

    m.save(&manifest_path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary.to_json(&manifest_path))?
        );
    } else {
        summary.print_human(&manifest_path);
    }

    Ok(())
}

#[derive(Debug, Default)]
struct ChangeSummary {
    sprites_touched: Vec<String>,
    tags_added: Vec<(String, Vec<String>)>,    // (sprite, tags)
    tags_removed: Vec<(String, Vec<String>)>,
    attribution_changes: Vec<(String, Option<String>)>,
    source_url_changes: Vec<(String, Option<String>)>,
}

fn apply_ops(entry: &mut SpriteEntry, ops: &TagOps, summary: &mut ChangeSummary, name: &str) {
    let mut touched = false;

    if ops.clear && !entry.tags.is_empty() {
        summary.tags_removed.push((name.to_string(), entry.tags.clone()));
        entry.tags.clear();
        touched = true;
    }

    if !ops.remove.is_empty() {
        let before: std::collections::HashSet<_> = entry.tags.iter().cloned().collect();
        entry.tags.retain(|t| !ops.remove.contains(t));
        let after: std::collections::HashSet<_> = entry.tags.iter().cloned().collect();
        let removed: Vec<String> = before.difference(&after).cloned().collect();
        if !removed.is_empty() {
            summary.tags_removed.push((name.to_string(), removed));
            touched = true;
        }
    }

    if !ops.add.is_empty() {
        let before: std::collections::HashSet<_> = entry.tags.iter().cloned().collect();
        for t in &ops.add {
            if !entry.tags.contains(t) {
                entry.tags.push(t.clone());
            }
        }
        entry.tags.sort();
        entry.tags.dedup();
        let after: std::collections::HashSet<_> = entry.tags.iter().cloned().collect();
        let added: Vec<String> = after.difference(&before).cloned().collect();
        if !added.is_empty() {
            summary.tags_added.push((name.to_string(), added));
            touched = true;
        }
    }

    if ops.clear_attribution && entry.attribution.is_some() {
        summary
            .attribution_changes
            .push((name.to_string(), None));
        entry.attribution = None;
        touched = true;
    }
    if let Some(value) = &ops.set_attribution {
        if entry.attribution.as_deref() != Some(value.as_str()) {
            entry.attribution = Some(value.clone());
            summary
                .attribution_changes
                .push((name.to_string(), Some(value.clone())));
            touched = true;
        }
    }

    if ops.clear_source_url && entry.source_url.is_some() {
        summary
            .source_url_changes
            .push((name.to_string(), None));
        entry.source_url = None;
        touched = true;
    }
    if let Some(value) = &ops.set_source_url {
        if entry.source_url.as_deref() != Some(value.as_str()) {
            entry.source_url = Some(value.clone());
            summary
                .source_url_changes
                .push((name.to_string(), Some(value.clone())));
            touched = true;
        }
    }

    if touched {
        summary.sprites_touched.push(name.to_string());
    }
}

impl ChangeSummary {
    fn print_human(&self, path: &Path) {
        println!("Updated manifest: {}", path.display());
        println!("  sprites touched: {}", self.sprites_touched.len());
        for (name, tags) in &self.tags_added {
            println!("  + {}  tags: {}", name, tags.join(","));
        }
        for (name, tags) in &self.tags_removed {
            println!("  - {}  tags: {}", name, tags.join(","));
        }
        for (name, val) in &self.attribution_changes {
            match val {
                Some(v) => println!("  @ {}  attribution = {:?}", name, v),
                None => println!("  @ {}  attribution cleared", name),
            }
        }
        for (name, val) in &self.source_url_changes {
            match val {
                Some(v) => println!("  @ {}  source_url = {:?}", name, v),
                None => println!("  @ {}  source_url cleared", name),
            }
        }
    }

    fn to_json(&self, path: &Path) -> serde_json::Value {
        serde_json::json!({
            "status": "ok",
            "manifest": path.display().to_string(),
            "sprites_touched": self.sprites_touched,
            "tags_added": self.tags_added.iter().map(|(n,t)| serde_json::json!({"name":n,"tags":t})).collect::<Vec<_>>(),
            "tags_removed": self.tags_removed.iter().map(|(n,t)| serde_json::json!({"name":n,"tags":t})).collect::<Vec<_>>(),
            "attribution_changes": self.attribution_changes.iter().map(|(n,v)| serde_json::json!({"name":n,"attribution":v})).collect::<Vec<_>>(),
            "source_url_changes": self.source_url_changes.iter().map(|(n,v)| serde_json::json!({"name":n,"source_url":v})).collect::<Vec<_>>(),
        })
    }
}

fn print_list_human(path: &Path, m: &Manifest, names: &[String]) {
    println!("Manifest: {}", path.display());
    for name in names {
        if let Some(e) = m.sprites.get(name) {
            println!("  {}", name);
            println!(
                "    tags:        {}",
                if e.tags.is_empty() {
                    "(none)".into()
                } else {
                    e.tags.join(", ")
                }
            );
            println!(
                "    attribution: {}",
                e.attribution.as_deref().unwrap_or("(none)")
            );
            println!(
                "    source_url:  {}",
                e.source_url.as_deref().unwrap_or("(none)")
            );
        }
    }
}

fn print_list_json(m: &Manifest, names: &[String]) -> Result<()> {
    let entries: Vec<serde_json::Value> = names
        .iter()
        .filter_map(|n| {
            m.sprites.get(n).map(|e| {
                serde_json::json!({
                    "name": n,
                    "tags": e.tags,
                    "attribution": e.attribution,
                    "source_url": e.source_url,
                })
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({"sprites": entries}))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::manifest::MANIFEST_VERSION;
    use std::collections::BTreeMap;

    fn make_entry(name: &str) -> SpriteEntry {
        SpriteEntry {
            rel_path: name.into(),
            file_size: 0, mtime: 0, content_hash: "h".into(),
            trim_offset: [0, 0], trimmed_size: [10, 10], source_size: [10, 10],
            polygon_hash: None, atlas_idx: 0, content_x: 0, content_y: 0,
            rotated: false, alias_of: None,
            tags: vec![], attribution: None, source_url: None,
        }
    }

    fn mk(entries: Vec<SpriteEntry>) -> Manifest {
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
            atlases: vec![],
        }
    }

    #[test]
    fn add_tags_dedups_and_sorts() {
        let mut m = mk(vec![make_entry("x.png")]);
        let ops = TagOps {
            add: vec!["ui".into(), "icon".into(), "ui".into()],
            ..Default::default()
        };
        let mut summary = ChangeSummary::default();
        apply_ops(m.sprites.get_mut("x.png").unwrap(), &ops, &mut summary, "x.png");
        let tags = &m.sprites.get("x.png").unwrap().tags;
        assert_eq!(tags, &vec!["icon".to_string(), "ui".to_string()]);
    }

    #[test]
    fn remove_drops_only_specified() {
        let mut m = mk(vec![make_entry("x.png")]);
        m.sprites.get_mut("x.png").unwrap().tags = vec!["a".into(), "b".into(), "c".into()];
        let ops = TagOps {
            remove: vec!["b".into()],
            ..Default::default()
        };
        let mut summary = ChangeSummary::default();
        apply_ops(m.sprites.get_mut("x.png").unwrap(), &ops, &mut summary, "x.png");
        assert_eq!(
            m.sprites.get("x.png").unwrap().tags,
            vec!["a".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn clear_wipes_all_tags() {
        let mut m = mk(vec![make_entry("x.png")]);
        m.sprites.get_mut("x.png").unwrap().tags = vec!["a".into(), "b".into()];
        let ops = TagOps {
            clear: true,
            ..Default::default()
        };
        let mut summary = ChangeSummary::default();
        apply_ops(m.sprites.get_mut("x.png").unwrap(), &ops, &mut summary, "x.png");
        assert!(m.sprites.get("x.png").unwrap().tags.is_empty());
    }

    #[test]
    fn set_attribution_records_and_idempotent() {
        let mut m = mk(vec![make_entry("x.png")]);
        let ops = TagOps {
            set_attribution: Some("CC0".into()),
            ..Default::default()
        };
        let mut summary = ChangeSummary::default();
        apply_ops(m.sprites.get_mut("x.png").unwrap(), &ops, &mut summary, "x.png");
        assert_eq!(summary.attribution_changes.len(), 1);

        // Re-apply same value — must NOT register as a change.
        let mut summary2 = ChangeSummary::default();
        apply_ops(m.sprites.get_mut("x.png").unwrap(), &ops, &mut summary2, "x.png");
        assert!(summary2.attribution_changes.is_empty());
    }
}
