use archcast::backend::{BackendChoice, resolve_backend};
use archcast::extract::extract_html_str;
use archcast::raster::{GifMode, GifOptions};
use archcast::render_html::render_html_document;
use archcast::render_md::render_markdown_document;
use archcast::schema::{EdgeKind, EdgeSpec, starter_template};
use archcast::validate::validate_spec;
use archcast::{RenderFormats, extract_inputs, render_inputs, spec_to_yaml};
use std::path::Path;
use tempfile::tempdir;

#[test]
fn starter_template_validates_renders_and_roundtrips_cleanly() {
    let spec = starter_template("测试链路全景");
    let report = validate_spec(&spec);
    assert!(report.valid, "issues: {:?}", report.issues);

    let backend = resolve_backend(BackendChoice::Cpu).expect("cpu backend");
    let html1 = render_html_document(&spec, backend.as_ref());
    let html2 = render_html_document(&spec, backend.as_ref());
    assert_eq!(html1, html2, "render output must be strictly deterministic");
    assert!(html1.contains("<svg"));
    assert!(html1.contains("<table class='flow'>"));
    assert!(html1.contains("navigator.gpu"), "HTML should include browser WebGPU motion layer");

    let md = render_markdown_document(&spec);
    assert!(md.contains("```mermaid"));
    assert!(md.contains("测试链路全景"));

    // Reverse extract rendered HTML back into ArchSpec.
    let extracted = extract_html_str(&html1, Path::new("starter.html")).expect("extract");
    let reval = validate_spec(&extracted);
    assert!(reval.valid, "extracted issues: {:?}", reval.issues);
    assert_eq!(extracted.diagrams.len(), spec.diagrams.len());
    assert_eq!(
        extracted.diagrams[0].columns.len(),
        spec.diagrams[0].columns.len()
    );
    assert!(extracted.flow_table.is_some());
    assert_eq!(
        extracted.flow_table.as_ref().map(|t| t.groups.len()),
        Some(1)
    );
}

#[test]
fn render_emits_standalone_svg_png_and_animated_gif_in_output_root() {
    let tmp = tempdir().expect("tempdir");
    let spec_path = tmp.path().join("flow.yaml");
    let out_dir = tmp.path().join("out");
    let spec = starter_template("Media Test Flow");
    std::fs::write(&spec_path, spec_to_yaml(&spec).expect("yaml")).expect("write");

    let backend = resolve_backend(BackendChoice::Cpu).expect("cpu backend");
    let results = render_inputs(
        &spec_path,
        &out_dir,
        RenderFormats {
            html: true,
            svg: true,
            markdown: true,
            png: true,
            gif: true,
            motion: true,
            png_scale: 0.5,
            gif_opts: GifOptions {
                mode: GifMode::Overlay,
                scale: 0.35,
                frames: 6,
                fps: 10,
            },
        },
        true,
        backend.as_ref(),
    )
    .expect("render media");

    assert_eq!(results.len(), 1);
    let r = &results[0];
    assert!(r.ok, "error: {:?}", r.error);
    assert_eq!(r.svg_outputs.len(), 1);
    assert_eq!(r.png_outputs.len(), 1);
    assert_eq!(r.gif_outputs.len(), 1);

    let svg_text = std::fs::read_to_string(&r.svg_outputs[0]).expect("read svg");
    assert!(svg_text.starts_with("<?xml version=\"1.0\""));
    assert!(svg_text.contains("<svg"));

    let png_bytes = std::fs::read(&r.png_outputs[0]).expect("read png");
    assert_eq!(&png_bytes[..8], b"\x89PNG\r\n\x1a\n");

    let gif_info = &r.gif_outputs[0];
    assert_eq!(gif_info.layer.mode, GifMode::Overlay);
    assert!(
        gif_info.layer.width < gif_info.layer.canvas_width
            && gif_info.layer.height < gif_info.layer.canvas_height,
        "overlay GIF must be cropped to minimal dynamic bounds, not full-screen"
    );
    let gif_bytes = std::fs::read(&gif_info.path).expect("read gif");
    assert_eq!(&gif_bytes[..6], b"GIF89a");

    // Also verify Delta mode (static base + minimal dirty-rect sub-frames).
    let delta_dir = tmp.path().join("delta");
    let delta_res = render_inputs(
        &spec_path,
        &delta_dir,
        RenderFormats {
            html: false,
            svg: false,
            markdown: false,
            png: false,
            gif: true,
            motion: false,
            png_scale: 0.5,
            gif_opts: GifOptions {
                mode: GifMode::Delta,
                scale: 0.35,
                frames: 6,
                fps: 10,
            },
        },
        true,
        backend.as_ref(),
    )
    .expect("render delta gif");
    assert_eq!(delta_res[0].gif_outputs[0].layer.mode, GifMode::Delta);
    let delta_bytes = std::fs::read(&delta_res[0].gif_outputs[0].path).expect("read delta gif");
    assert_eq!(&delta_bytes[..6], b"GIF89a");
}

#[test]
fn validate_catches_dangling_edge_and_duplicate_card() {
    let mut spec = starter_template("Bad Spec");
    spec.diagrams[0].edges.push(EdgeSpec {
        kind: EdgeKind::Direct,
        from: vec!["ui_create".to_string()],
        to: vec!["nonexistent_card".to_string()],
        label: None,
        bus_slot: None,
    });
    let report = validate_spec(&spec);
    assert!(!report.valid);
    assert_eq!(report.error_count, 1);
    assert!(
        report.issues[0]
            .message
            .contains("unknown target card id 'nonexistent_card'")
    );
}

#[test]
fn rayon_batch_render_and_extract_directory() {
    let tmp = tempdir().expect("tempdir");
    let in_dir = tmp.path().join("specs");
    let out_dir = tmp.path().join("out");
    let ext_dir = tmp.path().join("extracted");
    std::fs::create_dir_all(&in_dir).expect("mkdir");

    for idx in 0..6 {
        let spec = starter_template(&format!("Batch Service {idx}"));
        let yaml = spec_to_yaml(&spec).expect("yaml");
        std::fs::write(in_dir.join(format!("svc_{idx}.yaml")), yaml).expect("write");
    }

    let backend = resolve_backend(BackendChoice::Cpu).expect("cpu backend");
    let results = render_inputs(
        &in_dir,
        &out_dir,
        RenderFormats::default(),
        true,
        backend.as_ref(),
    )
    .expect("batch render");
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|r| r.ok && r.svg_outputs.len() == 1));

    let ext_results = extract_inputs(&out_dir, &ext_dir, true).expect("batch extract");
    assert_eq!(ext_results.len(), 6);
    assert!(ext_results.iter().all(|r| r.ok));
}
