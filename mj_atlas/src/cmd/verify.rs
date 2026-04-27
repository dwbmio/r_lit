//! `mj_atlas verify` — re-hash on-disk artifacts and report divergence from the manifest.
//!
//! Verifies, per atlas:
//!   - PNG file exists and its SHA256 matches the manifest's `image_hash`
//!
//! Verifies, per sprite (when `--check-sources` is set and the source file
//! is reachable from `input_root`):
//!   - File still exists
//!   - SHA256 of decoded RGBA pixels matches `content_hash`
//!
//! Exit code is 0 only when everything matches; otherwise 1 (with a JSON or
//! human-readable summary of issues).

use crate::error::{AppError, Result};
use crate::pack::manifest::{self, Manifest};
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub atlas_issues: Vec<AtlasIssue>,
    pub sprite_issues: Vec<SpriteIssue>,
    pub atlases_ok: usize,
    pub sprites_ok: usize,
    pub sprites_skipped: usize,
}

#[derive(Debug)]
pub struct AtlasIssue {
    pub atlas_idx: usize,
    pub image_filename: String,
    pub kind: AtlasIssueKind,
}

#[derive(Debug)]
pub enum AtlasIssueKind {
    Missing,
    HashMismatch { expected: String, got: String },
}

#[derive(Debug)]
pub struct SpriteIssue {
    pub name: String,
    pub kind: SpriteIssueKind,
}

#[derive(Debug)]
pub enum SpriteIssueKind {
    Missing,
    HashMismatch { expected: String, got: String },
    DimsMismatch { expected: [u32; 2], got: [u32; 2] },
    DecodeFailed(String),
}

pub fn run(input: &Path, check_sources: bool, json: bool) -> Result<()> {
    let manifest_path = manifest::resolve_manifest_path(input)?;
    let m = Manifest::try_load(&manifest_path)?
        .ok_or_else(|| AppError::Custom(format!("{} not loadable", manifest_path.display())))?;

    let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));
    let mut report = VerifyReport::default();

    // Atlas PNG checks.
    for (idx, atlas) in m.atlases.iter().enumerate() {
        let atlas_path = manifest_dir.join(&atlas.image_filename);
        if !atlas_path.is_file() {
            report.atlas_issues.push(AtlasIssue {
                atlas_idx: idx,
                image_filename: atlas.image_filename.clone(),
                kind: AtlasIssueKind::Missing,
            });
            continue;
        }
        let on_disk = manifest::hash_file(&atlas_path)?;
        if on_disk != atlas.image_hash {
            report.atlas_issues.push(AtlasIssue {
                atlas_idx: idx,
                image_filename: atlas.image_filename.clone(),
                kind: AtlasIssueKind::HashMismatch {
                    expected: atlas.image_hash.clone(),
                    got: on_disk,
                },
            });
        } else {
            report.atlases_ok += 1;
        }
    }

    // Sprite source checks (optional).
    if check_sources {
        let input_root = PathBuf::from(&m.input_root);
        for (name, entry) in &m.sprites {
            if entry.alias_of.is_some() {
                report.sprites_skipped += 1;
                continue;
            }
            let sprite_path = input_root.join(name);
            if !sprite_path.is_file() {
                report.sprite_issues.push(SpriteIssue {
                    name: name.clone(),
                    kind: SpriteIssueKind::Missing,
                });
                continue;
            }
            let img = match image::open(&sprite_path) {
                Ok(i) => i.into_rgba8(),
                Err(e) => {
                    report.sprite_issues.push(SpriteIssue {
                        name: name.clone(),
                        kind: SpriteIssueKind::DecodeFailed(e.to_string()),
                    });
                    continue;
                }
            };
            let dims = [img.width(), img.height()];
            if dims != entry.source_size {
                report.sprite_issues.push(SpriteIssue {
                    name: name.clone(),
                    kind: SpriteIssueKind::DimsMismatch {
                        expected: entry.source_size,
                        got: dims,
                    },
                });
                continue;
            }
            let h = manifest::hash_pixels(&img);
            if h != entry.content_hash {
                report.sprite_issues.push(SpriteIssue {
                    name: name.clone(),
                    kind: SpriteIssueKind::HashMismatch {
                        expected: entry.content_hash.clone(),
                        got: h,
                    },
                });
            } else {
                report.sprites_ok += 1;
            }
        }
    } else {
        report.sprites_skipped = m.sprites.len();
    }

    let any_issues = !report.atlas_issues.is_empty() || !report.sprite_issues.is_empty();

    if json {
        println!("{}", serde_json::to_string_pretty(&report.to_json(any_issues))?);
    } else {
        report.print_human(&manifest_path, any_issues);
    }

    if any_issues {
        std::process::exit(1);
    }
    Ok(())
}

impl VerifyReport {
    fn print_human(&self, path: &Path, any_issues: bool) {
        println!("Verify: {}", path.display());
        println!(
            "  atlases: {} ok, {} issues",
            self.atlases_ok,
            self.atlas_issues.len()
        );
        println!(
            "  sprites: {} ok, {} issues, {} skipped",
            self.sprites_ok,
            self.sprite_issues.len(),
            self.sprites_skipped
        );
        for issue in &self.atlas_issues {
            match &issue.kind {
                AtlasIssueKind::Missing => {
                    println!(
                        "  ✗ atlas[{}] {} — missing on disk",
                        issue.atlas_idx, issue.image_filename
                    );
                }
                AtlasIssueKind::HashMismatch { expected, got } => {
                    println!(
                        "  ✗ atlas[{}] {} — image_hash drift",
                        issue.atlas_idx, issue.image_filename
                    );
                    println!("      expected {}", expected);
                    println!("      got      {}", got);
                }
            }
        }
        for issue in &self.sprite_issues {
            match &issue.kind {
                SpriteIssueKind::Missing => {
                    println!("  ✗ sprite {} — missing source", issue.name);
                }
                SpriteIssueKind::HashMismatch { expected, got } => {
                    println!("  ✗ sprite {} — content_hash drift", issue.name);
                    println!("      expected {}", expected);
                    println!("      got      {}", got);
                }
                SpriteIssueKind::DimsMismatch { expected, got } => {
                    println!(
                        "  ✗ sprite {} — source dims changed: {}x{} → {}x{}",
                        issue.name, expected[0], expected[1], got[0], got[1]
                    );
                }
                SpriteIssueKind::DecodeFailed(e) => {
                    println!("  ✗ sprite {} — decode failed: {}", issue.name, e);
                }
            }
        }
        if !any_issues {
            println!();
            println!("  All artifacts match the manifest ✓");
        }
    }

    fn to_json(&self, any_issues: bool) -> serde_json::Value {
        let atlas_issues: Vec<serde_json::Value> = self
            .atlas_issues
            .iter()
            .map(|i| match &i.kind {
                AtlasIssueKind::Missing => serde_json::json!({
                    "atlas_idx": i.atlas_idx,
                    "image_filename": i.image_filename,
                    "kind": "missing"
                }),
                AtlasIssueKind::HashMismatch { expected, got } => serde_json::json!({
                    "atlas_idx": i.atlas_idx,
                    "image_filename": i.image_filename,
                    "kind": "hash_mismatch",
                    "expected": expected,
                    "got": got,
                }),
            })
            .collect();
        let sprite_issues: Vec<serde_json::Value> = self
            .sprite_issues
            .iter()
            .map(|i| match &i.kind {
                SpriteIssueKind::Missing => {
                    serde_json::json!({"name": i.name, "kind": "missing"})
                }
                SpriteIssueKind::HashMismatch { expected, got } => serde_json::json!({
                    "name": i.name, "kind": "hash_mismatch",
                    "expected": expected, "got": got,
                }),
                SpriteIssueKind::DimsMismatch { expected, got } => serde_json::json!({
                    "name": i.name, "kind": "dims_mismatch",
                    "expected": {"w": expected[0], "h": expected[1]},
                    "got":      {"w": got[0],      "h": got[1]},
                }),
                SpriteIssueKind::DecodeFailed(e) => serde_json::json!({
                    "name": i.name, "kind": "decode_failed", "error": e
                }),
            })
            .collect();
        serde_json::json!({
            "status": if any_issues { "error" } else { "ok" },
            "atlases_ok": self.atlases_ok,
            "atlas_issues": atlas_issues,
            "sprites_ok": self.sprites_ok,
            "sprites_skipped": self.sprites_skipped,
            "sprite_issues": sprite_issues,
        })
    }
}
