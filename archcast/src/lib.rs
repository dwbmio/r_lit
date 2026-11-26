//! `archcast` — AI-first deterministic architecture & flow report compiler.
//!
//! Compiles concise YAML/JSON single-source-of-truth specs into:
//! - 1:1 fixed-style standalone `HTML + SVG` executive/review reports (with WebGPU/Canvas2D motion),
//! - standalone `.svg` vector diagrams right in the render root,
//! - optional raster `.png` and animated flow-pulse `.gif` (via `resvg` + Rayon / `wgpu` WGSL compute),
//! - canonical `Markdown + Mermaid` engineering documents for AI context & wikis.
//!
//! Also reverse-extracts existing `HTML + SVG` overviews back into canonical YAML + Markdown.

pub mod backend;
pub mod error;
pub mod extract;
pub mod layout;
pub mod raster;
pub mod render_html;
pub mod render_md;
pub mod schema;
pub mod simd_ops;
pub mod theme;
pub mod validate;

use crate::backend::RenderBackend;
use crate::error::{ArchcastError, Result};
use crate::extract::extract_html_file;
use crate::raster::{GifLayerMetadata, GifOptions, encode_animated_gif, encode_png};
use crate::render_html::{render_html_from_rendered, render_standalone_svg};
use crate::render_md::{render_markdown_document, render_markdown_document_with_theme};
use crate::schema::ArchSpec;
use crate::theme::resolve_theme;
use crate::validate::{ValidationReport, validate_spec};
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct RenderFormats {
    pub html: bool,
    pub svg: bool,
    pub markdown: bool,
    pub png: bool,
    pub gif: bool,
    pub motion: bool,
    pub theme: Option<String>,
    pub themes_dir: Option<PathBuf>,
    pub png_scale: f32,
    pub gif_opts: GifOptions,
}

impl Default for RenderFormats {
    fn default() -> Self {
        Self {
            html: true,
            svg: true,
            markdown: true,
            png: false,
            gif: false,
            motion: true,
            theme: None,
            themes_dir: None,
            png_scale: 1.0,
            gif_opts: GifOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GifOutputInfo {
    pub path: PathBuf,
    pub layer: GifLayerMetadata,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderItemResult {
    pub input: PathBuf,
    pub ok: bool,
    pub html_output: Option<PathBuf>,
    pub svg_outputs: Vec<PathBuf>,
    pub markdown_output: Option<PathBuf>,
    pub png_outputs: Vec<PathBuf>,
    pub gif_outputs: Vec<GifOutputInfo>,
    pub diagram_count: usize,
    pub flow_row_count: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractItemResult {
    pub input: PathBuf,
    pub ok: bool,
    pub yaml_output: Option<PathBuf>,
    pub markdown_output: Option<PathBuf>,
    pub diagram_count: usize,
    pub flow_row_count: usize,
    pub error: Option<String>,
}

/// Load an [`ArchSpec`] from a `.yaml`, `.yml`, or `.json` file.
pub fn load_spec(path: &Path) -> Result<ArchSpec> {
    let raw = std::fs::read_to_string(path).map_err(|source| ArchcastError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    if ext == "json" {
        serde_json::from_str(&raw).map_err(|source| ArchcastError::JsonParse {
            path: path.to_path_buf(),
            source,
        })
    } else {
        serde_yaml_ng::from_str(&raw).map_err(|source| ArchcastError::YamlParse {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// Serialize an [`ArchSpec`] to clean YAML.
pub fn spec_to_yaml(spec: &ArchSpec) -> Result<String> {
    Ok(serde_yaml_ng::to_string(spec)?)
}

/// Collect input files (single file or directory scanned recursively).
pub fn collect_inputs(path: &Path, allowed_exts: &[&str]) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Err(ArchcastError::ReadInput {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "path does not exist"),
        });
    }
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = WalkDir::new(path)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .filter(|p| {
            p.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| {
                    let low = ext.to_ascii_lowercase();
                    allowed_exts.iter().any(|a| *a == low)
                })
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(ArchcastError::NoInputs(path.to_path_buf()));
    }
    Ok(files)
}

/// Compile one or more YAML/JSON specs into HTML, standalone SVG(s), Markdown, PNG, and/or animated GIF.
pub fn render_inputs(
    input_path: &Path,
    output_dir: &Path,
    formats: &RenderFormats,
    strict: bool,
    backend: &dyn RenderBackend,
) -> Result<Vec<RenderItemResult>> {
    let inputs = collect_inputs(input_path, &["yaml", "yml", "json"])?;
    std::fs::create_dir_all(output_dir).map_err(|source| ArchcastError::WriteOutput {
        path: output_dir.to_path_buf(),
        source,
    })?;

    let process_one = |file: &PathBuf| -> RenderItemResult {
        match render_single(file, output_dir, formats, strict, backend) {
            Ok(item) => item,
            Err(e) => RenderItemResult {
                input: file.clone(),
                ok: false,
                html_output: None,
                svg_outputs: Vec::new(),
                markdown_output: None,
                png_outputs: Vec::new(),
                gif_outputs: Vec::new(),
                diagram_count: 0,
                flow_row_count: 0,
                error: Some(e.to_string()),
            },
        }
    };

    let results = if inputs.len() <= 1 {
        inputs.iter().map(process_one).collect()
    } else {
        inputs.par_iter().map(process_one).collect()
    };
    Ok(results)
}

fn diagram_output_stem(file_stem: &str, diag_id: &str, total_diagrams: usize) -> String {
    if total_diagrams <= 1 {
        file_stem.to_string()
    } else {
        let safe_id: String = diag_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{file_stem}-{safe_id}")
    }
}

fn render_single(
    file: &Path,
    output_dir: &Path,
    formats: &RenderFormats,
    strict: bool,
    backend: &dyn RenderBackend,
) -> Result<RenderItemResult> {
    let spec = load_spec(file)?;
    if strict {
        let rep = validate_spec(&spec);
        if !rep.valid {
            return Err(ArchcastError::ValidationFailed {
                count: rep.error_count,
            });
        }
    }

    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("overview");

    let theme_ref = formats.theme.as_deref().or(spec.theme.as_deref());
    let active_theme = resolve_theme(theme_ref, formats.themes_dir.as_deref())?;

    let needs_svg_layout = formats.html || formats.svg || formats.png || formats.gif;
    let rendered_diagrams = if needs_svg_layout {
        backend.render_diagrams(&spec.diagrams, &active_theme)
    } else {
        Vec::new()
    };

    let mut html_output = None;
    let mut svg_outputs = Vec::new();
    let mut markdown_output = None;
    let mut png_outputs = Vec::new();
    let mut gif_outputs = Vec::new();

    if formats.html {
        let html = render_html_from_rendered(&spec, &rendered_diagrams, formats.motion, &active_theme);
        let out_path = output_dir.join(format!("{stem}.html"));
        write_atomic(&out_path, html.as_bytes())?;
        html_output = Some(out_path);
    }

    let total_diags = rendered_diagrams.len();
    if formats.svg {
        for (diag, svg_body) in &rendered_diagrams {
            let d_stem = diagram_output_stem(stem, &diag.id, total_diags);
            let out_path = output_dir.join(format!("{d_stem}.svg"));
            let standalone = render_standalone_svg(svg_body);
            write_atomic(&out_path, standalone.as_bytes())?;
            svg_outputs.push(out_path);
        }
    }

    if formats.png {
        let png_results: Result<Vec<PathBuf>> = rendered_diagrams
            .par_iter()
            .map(|(diag, svg_body)| {
                let d_stem = diagram_output_stem(stem, &diag.id, total_diags);
                let out_path = output_dir.join(format!("{d_stem}.png"));
                let bytes = encode_png(svg_body, formats.png_scale, &active_theme)?;
                write_atomic(&out_path, &bytes)?;
                Ok(out_path)
            })
            .collect();
        png_outputs = png_results?;
    }

    if formats.gif {
        let use_gpu = backend.is_gpu();
        // When GPU is active, encode diagrams sequentially to avoid contending on a single GPU queue;
        // on CPU, frame synthesis inside each diagram is already Rayon-parallelized.
        for (diag, svg_body) in &rendered_diagrams {
            let d_stem = diagram_output_stem(stem, &diag.id, total_diags);
            let out_path = output_dir.join(format!("{d_stem}.gif"));
            let encoded = encode_animated_gif(diag, svg_body, formats.gif_opts, use_gpu, &active_theme)?;
            write_atomic(&out_path, &encoded.bytes)?;
            gif_outputs.push(GifOutputInfo {
                path: out_path,
                layer: encoded.meta,
            });
        }
    }

    if formats.markdown {
        let md = render_markdown_document_with_theme(&spec, &active_theme);
        let out_path = output_dir.join(format!("{stem}.md"));
        write_atomic(&out_path, md.as_bytes())?;
        markdown_output = Some(out_path);
    }

    let flow_row_count = spec
        .flow_table
        .as_ref()
        .map(|t| t.groups.iter().map(|g| g.rows.len()).sum())
        .unwrap_or(0);

    Ok(RenderItemResult {
        input: file.to_path_buf(),
        ok: true,
        html_output,
        svg_outputs,
        markdown_output,
        png_outputs,
        gif_outputs,
        diagram_count: spec.diagrams.len(),
        flow_row_count,
        error: None,
    })
}

/// Reverse-extract one or more HTML+SVG files into canonical YAML + Markdown using Rayon.
pub fn extract_inputs(
    input_path: &Path,
    output_dir: &Path,
    emit_markdown: bool,
) -> Result<Vec<ExtractItemResult>> {
    let inputs = collect_inputs(input_path, &["html", "htm"])?;
    std::fs::create_dir_all(output_dir).map_err(|source| ArchcastError::WriteOutput {
        path: output_dir.to_path_buf(),
        source,
    })?;

    let process_one = |file: &PathBuf| -> ExtractItemResult {
        match extract_single(file, output_dir, emit_markdown) {
            Ok(item) => item,
            Err(e) => ExtractItemResult {
                input: file.clone(),
                ok: false,
                yaml_output: None,
                markdown_output: None,
                diagram_count: 0,
                flow_row_count: 0,
                error: Some(e.to_string()),
            },
        }
    };

    let results = if inputs.len() <= 1 {
        inputs.iter().map(process_one).collect()
    } else {
        inputs.par_iter().map(process_one).collect()
    };
    Ok(results)
}

fn extract_single(
    file: &Path,
    output_dir: &Path,
    emit_markdown: bool,
) -> Result<ExtractItemResult> {
    let spec = extract_html_file(file)?;
    let stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("overview");

    let yaml = spec_to_yaml(&spec)?;
    let yaml_path = output_dir.join(format!("{stem}.yaml"));
    write_atomic(&yaml_path, yaml.as_bytes())?;

    let mut markdown_output = None;
    if emit_markdown {
        let md = render_markdown_document(&spec);
        let md_path = output_dir.join(format!("{stem}.md"));
        write_atomic(&md_path, md.as_bytes())?;
        markdown_output = Some(md_path);
    }

    let flow_row_count = spec
        .flow_table
        .as_ref()
        .map(|t| t.groups.iter().map(|g| g.rows.len()).sum())
        .unwrap_or(0);

    Ok(ExtractItemResult {
        input: file.to_path_buf(),
        ok: true,
        yaml_output: Some(yaml_path),
        markdown_output,
        diagram_count: spec.diagrams.len(),
        flow_row_count,
        error: None,
    })
}

/// Validate a YAML/JSON spec file and return the structured report.
pub fn validate_file(path: &Path) -> Result<ValidationReport> {
    let spec = load_spec(path)?;
    Ok(validate_spec(&spec))
}

/// Write a file atomically via a temporary sibling file + rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|source| ArchcastError::WriteOutput {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp.{}",
        path.extension().and_then(|e| e.to_str()).unwrap_or("out"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).map_err(|source| ArchcastError::WriteOutput {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        ArchcastError::WriteOutput {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(())
}
