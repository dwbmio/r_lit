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

// =====================================================================
// v0.9 A — autosave sidecar behaviour from the CLI side
// =====================================================================

/// The CLI treats a `.maq.swap` path exactly like any other project
/// file — pass it in, it runs. This is intentional: recovery tooling
/// should not need a dedicated verb.
#[test]
fn cli_reads_swap_file_like_a_regular_project() {
    let tmp = tempfile::tempdir().unwrap();
    let project_path = fixture_project(tmp.path(), "swap_read");
    // Promote the fixture's grid into the swap sidecar by copying
    // the bytes verbatim — same format.
    let swap_path = project::swap_path(&project_path);
    std::fs::copy(&project_path, &swap_path).unwrap();

    let out = tmp.path().join("from_swap.glb");
    let status = Command::new(cli_bin())
        .arg("export")
        .arg(&swap_path)
        .arg("--out")
        .arg(&out)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "CLI should export from a .maq.swap file the same as from a .maq"
    );
    assert!(out.exists(), "export target should be written");
}

/// When the CLI is given a `.maq`, it must read **that file** — never
/// silently redirect to a newer sibling `.maq.swap`. This matters for
/// repeatable CI builds: a stale editor swap on a developer's machine
/// must not alter what `maquette-cli export foo.maq` produces.
#[test]
fn cli_export_ignores_sibling_swap_file() {
    let tmp = tempfile::tempdir().unwrap();

    // The `.maq` has one painted cell.
    let project_path = tmp.path().join("quiet.maq");
    let mut project_grid = Grid::with_size(4, 4);
    project_grid.paint(0, 0, 0, 1);
    project::write_project(&project_path, &project_grid, &Palette::default()).unwrap();

    // The `.maq.swap` has *many* painted cells — distinguishable in
    // the exported geometry. If the CLI mis-reads the swap, the
    // exported glTF's mesh count will be larger than expected.
    let mut swap_grid = Grid::with_size(4, 4);
    for x in 0..4 {
        for y in 0..4 {
            swap_grid.paint(x, y, 0, 1);
        }
    }
    project::write_swap(&project_path, &swap_grid, &Palette::default()).unwrap();
    assert_eq!(
        project::swap_is_newer(&project_path),
        Some(true),
        "swap should be newer than the .maq we just wrote earlier"
    );

    let out_path = tmp.path().join("quiet.glb");
    let status = Command::new(cli_bin())
        .arg("export")
        .arg(&project_path)
        .arg("--out")
        .arg(&out_path)
        .status()
        .unwrap();
    assert!(status.success(), "export failed: status = {status}");

    // The expected cell count for the `.maq` path (1 painted cell)
    // yields exactly one color bucket after greedy meshing. The swap
    // (16 painted cells) would also be one bucket at the same color,
    // so a bucket-count assertion isn't enough — compare vertex
    // counts instead: one cell → 8 unique vertices for a culled
    // single cube; 16 cells packed flat → many more.
    // Lower-bounding instead of equality: the export includes an
    // inverted-hull outline, so the glTF's vertex count is a
    // multiple of the raw mesh vertex count. What distinguishes the
    // two inputs cleanly is scale — the 16-cell swap produces way
    // more geometry than the 1-cell .maq under any meshing or
    // outline choice.
    let one_cell_vertices: usize = build_color_buckets(&project_grid)
        .iter()
        .map(|(_, b)| b.positions.len())
        .sum();
    let sixteen_cell_vertices: usize = build_color_buckets_culled(&swap_grid)
        .iter()
        .map(|(_, b)| b.positions.len())
        .sum();
    assert!(
        sixteen_cell_vertices > one_cell_vertices * 4,
        "test fixtures should be far apart in size: one={one_cell_vertices}, \
         swap={sixteen_cell_vertices}"
    );

    let bytes = std::fs::read(&out_path).unwrap();
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    let actual_vertices: usize = gltf
        .meshes()
        .flat_map(|m| m.primitives())
        .filter_map(|p| p.get(&gltf::Semantic::Positions).map(|a| a.count()))
        .sum();

    // Exported vertices track the `.maq`'s scale (with a small
    // constant factor for outline extrusion), not the swap's. Any
    // value within 3× of the one-cell baseline is "clearly not the
    // swap"; the swap would be an order of magnitude larger.
    let ceiling = one_cell_vertices * 3;
    assert!(
        actual_vertices <= ceiling,
        "CLI appears to have read the swap instead of the .maq: \
         actual_vertices={actual_vertices}, ceiling (3× one-cell baseline)={ceiling}, \
         swap's raw vertex count would be {sixteen_cell_vertices}"
    );
}

// ---------------------------------------------------------------------
// `maquette-cli texture gen` (v0.10 A — Mock provider only)
// ---------------------------------------------------------------------

/// Same prompt + same seed = byte-identical PNG. This is the
/// foundation of the disk cache and of the GUI's "tweak prompt
/// without re-spending money" loop, so we want a regression here
/// even though the underlying unit tests already cover it — the
/// CLI argument plumbing is its own failure surface.
#[test]
fn cli_texture_gen_mock_is_deterministic() {
    let tmp = tempfile::tempdir().unwrap();
    let a = tmp.path().join("a.png");
    let b = tmp.path().join("b.png");

    // Use --no-cache so the second run actually re-generates rather
    // than serving from cache; that way we're really asserting on
    // provider determinism, not on cache plumbing.
    for out in [&a, &b] {
        let status = Command::new(cli_bin())
            .arg("texture")
            .arg("gen")
            .arg("--prompt")
            .arg("isometric stone tile, low-poly")
            .arg("--seed")
            .arg("12345")
            .arg("--width")
            .arg("32")
            .arg("--height")
            .arg("32")
            .arg("--no-cache")
            .arg("--out")
            .arg(out)
            .status()
            .expect("failed to invoke maquette-cli texture gen");
        assert!(status.success(), "texture gen failed for {}", out.display());
    }

    let bytes_a = std::fs::read(&a).unwrap();
    let bytes_b = std::fs::read(&b).unwrap();
    assert!(!bytes_a.is_empty(), "PNG should be non-empty");
    assert_eq!(
        bytes_a, bytes_b,
        "Mock provider must be deterministic for identical requests"
    );

    // Sanity: actually a PNG header.
    assert_eq!(&bytes_a[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
}

/// Different prompt → different bytes. Catches the "we accidentally
/// wired prompt to a no-op argument" class of bug.
#[test]
fn cli_texture_gen_diverges_on_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let stone = tmp.path().join("stone.png");
    let grass = tmp.path().join("grass.png");

    for (prompt, out) in [("stone tile", &stone), ("grass tile", &grass)] {
        let status = Command::new(cli_bin())
            .arg("texture")
            .arg("gen")
            .arg("--prompt")
            .arg(prompt)
            .arg("--seed")
            .arg("0")
            .arg("--width")
            .arg("32")
            .arg("--height")
            .arg("32")
            .arg("--no-cache")
            .arg("--out")
            .arg(out)
            .status()
            .unwrap();
        assert!(status.success());
    }

    assert_ne!(
        std::fs::read(&stone).unwrap(),
        std::fs::read(&grass).unwrap(),
        "different prompts must yield different PNG bytes"
    );
}

/// The disk cache must be re-entrant: a second run with the same
/// request and the cache enabled has to produce the same PNG and
/// must not re-invoke the provider. Phase A doesn't bill, so we
/// can't observe billing; but we can at least observe identical
/// output files (and on a future Fal provider this same test
/// becomes "second run is free").
#[test]
fn cli_texture_gen_cache_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp.path().join("cache");
    let out1 = tmp.path().join("first.png");
    let out2 = tmp.path().join("second.png");

    // Point the cache at a tempdir via XDG_CACHE_HOME so we don't
    // touch the user's real ~/.cache during testing.
    let run = |out: &Path| {
        let status = Command::new(cli_bin())
            .env("XDG_CACHE_HOME", &cache)
            .arg("texture")
            .arg("gen")
            .arg("--prompt")
            .arg("dirt block")
            .arg("--seed")
            .arg("99")
            .arg("--width")
            .arg("32")
            .arg("--height")
            .arg("32")
            .arg("--out")
            .arg(out)
            .status()
            .unwrap();
        assert!(status.success(), "texture gen failed");
    };

    run(&out1);
    run(&out2);

    assert_eq!(
        std::fs::read(&out1).unwrap(),
        std::fs::read(&out2).unwrap(),
        "cached re-run must produce identical bytes"
    );
    let cached_textures = cache.join("maquette").join("textures");
    assert!(
        cached_textures.exists(),
        "cache directory should be created at {}",
        cached_textures.display()
    );
    let entries: Vec<_> = std::fs::read_dir(&cached_textures)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "cache should have exactly one entry (one unique request), got {entries:?}"
    );
}

// ---------------------------------------------------------------------
// `maquette-cli texture {revoke,purge}` (v0.10 B — Rustyme producer)
// ---------------------------------------------------------------------

/// Without `--admin-url` and without the fallback env var, `revoke`
/// must fail fast with an actionable message, not try to talk to
/// some default URL. This is the contract users rely on when they
/// typo the flag name.
#[test]
fn cli_texture_revoke_requires_admin_url() {
    let out = Command::new(cli_bin())
        .env_remove("MAQUETTE_RUSTYME_ADMIN_URL")
        .args(["texture", "revoke", "00000000-0000-0000-0000-000000000000"])
        .output()
        .expect("failed to spawn");
    assert!(
        !out.status.success(),
        "revoke without admin url must exit non-zero, got {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--admin-url") && stderr.contains("MAQUETTE_RUSTYME_ADMIN_URL"),
        "stderr should point the user at both the flag and the env var, got: {stderr}"
    );
}

/// Same contract for `purge` — a typo here otherwise silently
/// talks to nothing useful.
#[test]
fn cli_texture_purge_requires_admin_url() {
    let out = Command::new(cli_bin())
        .env_remove("MAQUETTE_RUSTYME_ADMIN_URL")
        .args(["texture", "purge", "texgen"])
        .output()
        .expect("failed to spawn");
    assert!(
        !out.status.success(),
        "purge without admin url must exit non-zero, got {out:?}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--admin-url") && stderr.contains("MAQUETTE_RUSTYME_ADMIN_URL"),
        "stderr should point the user at both the flag and the env var, got: {stderr}"
    );
}

/// Selecting `--provider rustyme` without `MAQUETTE_RUSTYME_REDIS_URL`
/// must fail with an actionable error pointing at the docs; otherwise
/// the user sees an opaque "redis connect" error that looks like a
/// network problem.
#[test]
fn cli_texture_gen_rustyme_requires_redis_url() {
    let tmp = tempfile::tempdir().unwrap();
    let out_path = tmp.path().join("x.png");
    let output = Command::new(cli_bin())
        .env_remove("MAQUETTE_RUSTYME_REDIS_URL")
        .args([
            "texture", "gen",
            "--provider", "rustyme",
            "--prompt", "stone",
            "--no-cache",
            "--out",
        ])
        .arg(&out_path)
        .output()
        .expect("failed to spawn");
    assert!(
        !output.status.success(),
        "rustyme provider without redis url must error out"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("MAQUETTE_RUSTYME_REDIS_URL"),
        "stderr should name the missing env var, got: {stderr}"
    );
}
