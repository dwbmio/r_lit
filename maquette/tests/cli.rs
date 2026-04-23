//! End-to-end CLI integration tests.
//!
//! These drive the `maquette-cli` binary the way a CI pipeline or a
//! game build script would: parse args, read a `.maq` file, emit a
//! glTF/GLB artefact, assert its structure. The assertions
//! deliberately poke at the *output file*, not at the lib internals
//! — that way these tests catch regressions even if the lib's public
//! API stays the same but its wiring to the CLI breaks.
//!
//! No `assert_cmd` / `predicates` dependency on purpose (keeps the
//! dev-dep surface boring and CI-lean). We shell out to the binary
//! Cargo built for us via `CARGO_BIN_EXE_maquette-cli`.

use std::path::{Path, PathBuf};
use std::process::Command;

use maquette::grid::{Grid, Palette};
use maquette::mesher::{build_color_buckets, build_color_buckets_culled};
use maquette::project;

fn cli_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maquette-cli"))
}

fn fixture_project(dir: &Path, name: &str) -> PathBuf {
    let mut grid = Grid::with_size(4, 4);
    grid.paint(0, 0, 0, 1);
    grid.paint(1, 0, 0, 2);
    grid.paint(2, 0, 3, 1);
    grid.paint(2, 1, 3, 1);
    let palette = Palette::default();
    let path = dir.join(format!("{name}.maq"));
    project::write_project(&path, &grid, &palette).unwrap();
    path
}

#[test]
fn cli_export_glb_writes_valid_file() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "cross");
    let out = tmp.path().join("cross.glb");

    let status = Command::new(cli_bin())
        .arg("export")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .status()
        .expect("failed to invoke maquette-cli");
    assert!(status.success(), "export failed: status = {status}");

    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.len() > 12, "GLB should have at least a 12-byte header");
    assert_eq!(&bytes[..4], b"glTF", "missing GLB magic");
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    assert_eq!(version, 2, "expected glTF v2");

    // Round-trip through the canonical parser — same check the lib
    // unit tests do, but via the real CLI path.
    let g = gltf::Gltf::from_slice(&bytes).expect("canonical gltf parser rejected our output");
    assert!(g.meshes().next().is_some(), "no meshes in exported GLB");
}

#[test]
fn cli_export_gltf_writes_sibling_bin() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "tree");
    let out = tmp.path().join("tree.gltf");

    let status = Command::new(cli_bin())
        .arg("export")
        .arg(&input)
        .arg("--out")
        .arg(&out)
        .status()
        .expect("failed to invoke maquette-cli");
    assert!(status.success());

    assert!(out.exists(), ".gltf missing");
    let bin = out.with_extension("bin");
    assert!(bin.exists(), "sibling .bin missing next to .gltf");

    let text = std::fs::read_to_string(&out).unwrap();
    assert!(
        text.contains("\"asset\""),
        ".gltf does not look like glTF text: {text}"
    );
}

#[test]
fn cli_export_no_outline_produces_fewer_primitives() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "house");

    let with_outline = tmp.path().join("with.glb");
    let no_outline = tmp.path().join("no.glb");

    run_ok([
        "export".as_ref(),
        input.as_os_str(),
        "--out".as_ref(),
        with_outline.as_os_str(),
    ]);
    run_ok([
        "export".as_ref(),
        input.as_os_str(),
        "--out".as_ref(),
        no_outline.as_os_str(),
        "--no-outline".as_ref(),
    ]);

    let with_gltf = gltf::Gltf::from_slice(&std::fs::read(&with_outline).unwrap()).unwrap();
    let no_gltf = gltf::Gltf::from_slice(&std::fs::read(&no_outline).unwrap()).unwrap();

    assert!(
        with_gltf.meshes().count() > no_gltf.meshes().count(),
        "outline disable should remove at least one mesh (body+outline → body)"
    );
}

#[test]
fn cli_validate_ok_on_good_file() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "ok");

    let out = Command::new(cli_bin())
        .arg("validate")
        .arg(&input)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn cli_validate_fails_on_bad_file() {
    let tmp = tempfile::tempdir().unwrap();
    let bad = tmp.path().join("bad.maq");
    std::fs::write(&bad, "{ not json").unwrap();

    let out = Command::new(cli_bin())
        .arg("validate")
        .arg(&bad)
        .output()
        .unwrap();
    assert!(!out.status.success(), "validate should fail on garbage");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("error"),
        "expected an error message, got: {stderr}"
    );
}

#[test]
fn cli_info_text_and_json() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "info");

    let text = Command::new(cli_bin())
        .arg("info")
        .arg(&input)
        .output()
        .unwrap();
    assert!(text.status.success());
    let txt = String::from_utf8_lossy(&text.stdout);
    assert!(txt.contains("canvas:"), "text summary missing header: {txt}");
    assert!(txt.contains("4 painted"), "text summary cell count off: {txt}");

    let json = Command::new(cli_bin())
        .arg("info")
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert!(json.status.success());
    let j = String::from_utf8_lossy(&json.stdout);
    let parsed: serde_json::Value = serde_json::from_str(j.trim()).unwrap();
    assert_eq!(parsed["cells"]["painted"], 4);
}

#[test]
fn cli_export_uses_greedy_mesher() {
    // The CLI export path must use the greedy mesher, not the culled
    // oracle. Assert by reading the accessor counts out of the
    // emitted GLB and comparing against what each mesher *would*
    // produce for the same grid. If someone accidentally wires the
    // export to `build_color_buckets_culled`, this blows up.
    let tmp = tempfile::tempdir().unwrap();

    // A flat slab maximises the greedy win — a 4×4 top face collapses
    // to a single quad instead of 16.
    let mut grid = Grid::with_size(4, 4);
    for x in 0..4 {
        for z in 0..4 {
            grid.paint(x, z, 0, 1);
        }
    }
    let palette = Palette::default();
    let project_path = tmp.path().join("slab.maq");
    project::write_project(&project_path, &grid, &palette).unwrap();
    let out = tmp.path().join("slab.glb");

    run_ok([
        "export".as_ref(),
        project_path.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--no-outline".as_ref(), // focus the test on the body mesh
    ]);

    let bytes = std::fs::read(&out).unwrap();
    let g = gltf::Gltf::from_slice(&bytes).unwrap();
    let body_triangles: usize = g
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .map(|p| {
            p.indices()
                .map(|acc| acc.count() / 3)
                .unwrap_or(0)
        })
        .sum();

    let greedy_tris: usize = build_color_buckets(&grid)
        .iter()
        .map(|(_, b)| b.indices.len() / 3)
        .sum();
    let culled_tris: usize = build_color_buckets_culled(&grid)
        .iter()
        .map(|(_, b)| b.indices.len() / 3)
        .sum();

    assert_eq!(
        body_triangles, greedy_tris,
        "CLI export triangle count should match greedy mesher"
    );
    assert!(
        greedy_tris < culled_tris,
        "greedy mesher should produce fewer triangles than culled \
         on a flat slab (greedy={greedy_tris} culled={culled_tris})"
    );
}

#[test]
fn cli_render_writes_valid_png() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "render_fixture");
    let out = tmp.path().join("render.png");

    run_ok([
        "render".as_ref(),
        input.as_os_str(),
        "--out".as_ref(),
        out.as_os_str(),
        "--width".as_ref(),
        "96".as_ref(),
        "--height".as_ref(),
        "96".as_ref(),
    ]);

    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(
        &bytes[..8],
        &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
        "missing PNG magic"
    );

    // Decode and sanity-check: right dimensions, at least one non-background pixel.
    let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
    let mut reader = decoder.read_info().unwrap();
    assert_eq!(reader.info().width, 96);
    assert_eq!(reader.info().height, 96);
    let buf_size = reader.output_buffer_size().expect("png buffer size");
    let mut pixels = vec![0u8; buf_size];
    let info = reader.next_frame(&mut pixels).unwrap();
    let stride = info.line_size;
    assert_eq!(stride, 96 * 4);
    let non_bg = pixels
        .chunks_exact(4)
        .filter(|c| !(c[0] == 0x18 && c[1] == 0x1a && c[2] == 0x1e))
        .count();
    assert!(
        non_bg >= 64,
        "rendered PNG should contain the model, got {non_bg} non-background pixels"
    );
}

#[test]
fn cli_render_rejects_empty_output_path() {
    let tmp = tempfile::tempdir().unwrap();
    let input = fixture_project(tmp.path(), "render_missing_out");
    // --out pointing at an unwritable directory — deterministic fail.
    let bogus = tmp.path().join("no_such_dir").join("render.png");

    let status = Command::new(cli_bin())
        .arg("render")
        .arg(&input)
        .arg("--out")
        .arg(&bogus)
        .status()
        .unwrap();
    assert!(!status.success(), "writing to non-existent dir should fail");
}

#[test]
fn cli_palette_export_import_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let src = fixture_project(tmp.path(), "pal_src");
    let colors = tmp.path().join("colors.json");

    // Export the palette from the source project.
    run_ok([
        "palette".as_ref(),
        "export".as_ref(),
        src.as_os_str(),
        "--out".as_ref(),
        colors.as_os_str(),
    ]);
    let raw = std::fs::read_to_string(&colors).unwrap();
    assert!(
        raw.contains("\"version\":") && raw.contains("\"colors\":"),
        "palette JSON missing expected keys: {raw}"
    );
    assert!(raw.contains('#'), "palette JSON should contain hex colors: {raw}");

    // Build a separate project with a *different* palette, then import.
    let mut dst_grid = Grid::with_size(4, 4);
    dst_grid.paint(0, 0, 0, 1);
    let mut dst_palette = Palette::default();
    if let Some(slot) = dst_palette.colors.get_mut(0) {
        *slot = Some(bevy::prelude::Color::srgb(0.12, 0.34, 0.56));
    }
    let dst_path = tmp.path().join("dst.maq");
    project::write_project(&dst_path, &dst_grid, &dst_palette).unwrap();

    let out_path = tmp.path().join("dst_reskinned.maq");
    run_ok([
        "palette".as_ref(),
        "import".as_ref(),
        dst_path.as_os_str(),
        "--from".as_ref(),
        colors.as_os_str(),
        "--out".as_ref(),
        out_path.as_os_str(),
    ]);

    // Reload both sides and compare palettes color-by-color.
    let (_, pal_src) = project::read_project(&src).unwrap();
    let (_, pal_out) = project::read_project(&out_path).unwrap();
    assert_eq!(pal_src.colors.len(), pal_out.colors.len());
    for (i, (a, b)) in pal_src.colors.iter().zip(pal_out.colors.iter()).enumerate() {
        match (a, b) {
            (None, None) => {}
            (Some(ca), Some(cb)) => {
                let sa = ca.to_srgba();
                let sb = cb.to_srgba();
                let close = (sa.red - sb.red).abs() < 1.5 / 255.0
                    && (sa.green - sb.green).abs() < 1.5 / 255.0
                    && (sa.blue - sb.blue).abs() < 1.5 / 255.0;
                assert!(close, "slot {i} drift too large: {sa:?} vs {sb:?}");
            }
            _ => panic!("slot {i} liveness mismatch after import"),
        }
    }
}

#[test]
fn cli_rejects_missing_input() {
    let out = Command::new(cli_bin())
        .arg("export")
        .arg("/does/not/exist.maq")
        .arg("--out")
        .arg("/tmp/ignored.glb")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1), "runtime error should exit 1");
}

fn run_ok<I>(args: I)
where
    I: IntoIterator,
    I::Item: AsRef<std::ffi::OsStr>,
{
    let status = Command::new(cli_bin()).args(args).status().unwrap();
    assert!(status.success(), "cli call failed: {status}");
}
