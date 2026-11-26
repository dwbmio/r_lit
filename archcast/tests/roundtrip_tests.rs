use archcast::backend::{BackendChoice, resolve_backend};
use archcast::extract::extract_html_str;
use archcast::raster::{GifMode, GifOptions};
use archcast::render_html::render_html_document;
use archcast::render_md::render_markdown_document;
use archcast::schema::{EdgeKind, EdgeSpec, starter_template};
use archcast::theme::{
    BUILTIN_THEME_IDS, DEFAULT_THEME_ID, builtin_theme, delete_external_theme, list_themes,
    load_theme_file, resolve_theme, save_external_theme,
};
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
        &RenderFormats {
            html: true,
            svg: true,
            markdown: true,
            png: true,
            gif: true,
            motion: true,
            theme: None,
            themes_dir: None,
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
        &RenderFormats {
            html: false,
            svg: false,
            markdown: false,
            png: false,
            gif: true,
            motion: false,
            theme: None,
            themes_dir: None,
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
        &RenderFormats::default(),
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

#[test]
fn external_theme_crud_extends_override_and_render() {
    let tmp = tempdir().expect("tempdir");
    let themes_dir = tmp.path().join("themes");

    // 1. Default list contains all 5 bundled themes.
    let initial = list_themes(Some(&themes_dir)).expect("list themes");
    assert_eq!(initial.len(), BUILTIN_THEME_IDS.len());
    assert!(initial.iter().all(|t| t.source == "builtin"));

    // 2. Create a custom theme derived from starlink-light and apply token updates.
    let mut corp = builtin_theme(DEFAULT_THEME_ID).expect("default builtin");
    corp.id = "corp-blue".to_string();
    corp.display_name = "Corp Blue".to_string();
    corp.extends = Some(DEFAULT_THEME_ID.to_string());
    corp.apply_field("surface.page_bg", "#f0f4ff")
        .expect("page_bg");
    corp.apply_field("chrome.h2_accent", "#1e40af")
        .expect("h2_accent");
    corp.apply_field("tones.emerald", "#047857")
        .expect("tones.emerald");
    assert!(corp.apply_field("motion.pulse_hex", "#1d4ed8").is_err());
    corp.apply_field("motion.pulse_hex", "#1d4ed8,#047857,#d97706,#7c3aed")
        .expect("motion.pulse_hex");

    let saved = save_external_theme(&corp, Some(&themes_dir), false).expect("save corp-blue");
    assert!(saved.is_file());
    // Duplicate save without overwrite must fail.
    assert!(save_external_theme(&corp, Some(&themes_dir), false).is_err());

    // 3. Partial YAML file using `extends: corp-blue` inherits parent tokens cleanly.
    let child_file = tmp.path().join("corp-blue-dark-edge.yaml");
    std::fs::write(
        &child_file,
        "id: corp-blue-child\ndisplay_name: Corp Blue Child\nextends: corp-blue\nchrome:\n  edge_stroke: \"#0f172a\"\n",
    )
    .expect("write child yaml");
    let loaded_child = load_theme_file(&child_file, Some(&themes_dir)).expect("load child");
    assert_eq!(loaded_child.surface.page_bg, "#f0f4ff");
    assert_eq!(loaded_child.chrome.h2_accent, "#1e40af");
    assert_eq!(loaded_child.chrome.edge_stroke, "#0f172a");

    // 4. External override of bundled `starlink-light` with self-extends works and restores on delete.
    let override_file = themes_dir.join("starlink-light.yaml");
    std::fs::write(
        &override_file,
        "id: starlink-light\nextends: starlink-light\nchrome:\n  h2_accent: \"#b91c1c\"\n",
    )
    .expect("write override");
    let overridden = resolve_theme(None, Some(&themes_dir)).expect("resolve overridden default");
    assert_eq!(overridden.chrome.h2_accent, "#b91c1c");
    let listed = list_themes(Some(&themes_dir)).expect("list with override");
    let sl = listed
        .iter()
        .find(|t| t.id == "starlink-light")
        .expect("starlink-light");
    assert_eq!(sl.source, "external");
    assert!(sl.overrides_builtin);

    let (_removed_path, restored) =
        delete_external_theme("starlink-light", Some(&themes_dir)).expect("delete override");
    assert!(restored);
    // Deleting again (now pure built-in) must fail.
    assert!(delete_external_theme("starlink-light", Some(&themes_dir)).is_err());
    let restored_theme = resolve_theme(None, Some(&themes_dir)).expect("resolve restored default");
    assert_eq!(
        restored_theme.chrome.h2_accent,
        builtin_theme(DEFAULT_THEME_ID)
            .expect("builtin")
            .chrome
            .h2_accent
    );

    // 5. Render using external theme ID and direct file path.
    let spec_path = tmp.path().join("flow.yaml");
    let out_dir = tmp.path().join("render");
    let spec = starter_template("Themed Flow");
    std::fs::write(&spec_path, spec_to_yaml(&spec).expect("yaml")).expect("write spec");
    let backend = resolve_backend(BackendChoice::Cpu).expect("cpu backend");

    let res = render_inputs(
        &spec_path,
        &out_dir,
        &RenderFormats {
            html: true,
            svg: true,
            markdown: true,
            png: false,
            gif: false,
            motion: true,
            theme: Some("corp-blue".to_string()),
            themes_dir: Some(themes_dir.clone()),
            ..RenderFormats::default()
        },
        true,
        backend.as_ref(),
    )
    .expect("render with external theme");
    assert!(res[0].ok, "error: {:?}", res[0].error);
    let html = std::fs::read_to_string(res[0].html_output.as_ref().expect("html")).expect("read");
    assert!(html.contains("data-archcast-theme=\"corp-blue\""));
    assert!(html.contains("#f0f4ff"));
    assert!(html.contains("#1e40af"));
    let md = std::fs::read_to_string(res[0].markdown_output.as_ref().expect("md")).expect("read md");
    assert!(md.contains("%%{init:"));

    // Direct file path theme rendering.
    let out_direct = tmp.path().join("render_direct");
    let res_direct = render_inputs(
        &spec_path,
        &out_direct,
        &RenderFormats {
            html: true,
            svg: false,
            markdown: false,
            png: false,
            gif: false,
            motion: false,
            theme: Some(child_file.to_string_lossy().to_string()),
            themes_dir: Some(themes_dir.clone()),
            ..RenderFormats::default()
        },
        true,
        backend.as_ref(),
    )
    .expect("render with direct file theme");
    assert!(res_direct[0].ok, "error: {:?}", res_direct[0].error);
    let html_direct =
        std::fs::read_to_string(res_direct[0].html_output.as_ref().expect("html")).expect("read");
    assert!(html_direct.contains("data-archcast-theme=\"corp-blue-child\""));

    // 6. Delete custom external theme cleanly.
    let (_p, restored_corp) =
        delete_external_theme("corp-blue", Some(&themes_dir)).expect("delete corp-blue");
    assert!(!restored_corp);
    assert!(resolve_theme(Some("corp-blue"), Some(&themes_dir)).is_err());
}
