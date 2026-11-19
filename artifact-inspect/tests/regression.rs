use artifact_inspect::{Limits, inspect_path, render_html};
use flate2::{Compression, write::GzEncoder};
use std::fs;
use std::io::Write;
use std::path::Path;
use zip::{CompressionMethod, write::SimpleFileOptions};

fn zip_file(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn nested_zip_has_independent_stable_counts_and_size_semantics() {
    let tmp = tempfile::tempdir().unwrap();
    let inner = tmp.path().join("inner.zip");
    zip_file(&inner, &[("z-last.txt", b"12345"), ("a-first.bin", b"abc")]);
    let inner_bytes = fs::read(&inner).unwrap();
    let outer = tmp.path().join("outer.zip");
    zip_file(
        &outer,
        &[
            ("z-outer.txt", b"outer"),
            ("assets/nested.zip", &inner_bytes),
        ],
    );

    let report = inspect_path(&outer, &Limits::default()).unwrap();
    assert_eq!(report.containers.len(), 2);
    assert_eq!(report.containers[0].kind, "zip");
    assert_eq!(report.containers[0].summary.file_count, 2);
    assert_eq!(report.containers[1].kind, "zip");
    assert_eq!(report.containers[1].summary.file_count, 2);
    assert_eq!(report.containers[1].summary.raw_bytes, 8);
    assert!(report.containers[1].summary.zip_compressed_bytes.is_some());
    assert_eq!(
        report.containers[0].files[0].relative_path,
        "assets/nested.zip"
    );
    assert_eq!(report.containers[0].files[1].relative_path, "z-outer.txt");
    assert_eq!(report.containers[1].files[0].relative_path, "a-first.bin");
    assert_eq!(report.containers[1].files[1].relative_path, "z-last.txt");
    assert!(
        report.containers[0]
            .files
            .iter()
            .all(|f| f.zip_compressed_bytes.is_some())
    );
}

#[test]
fn directory_inputs_also_discover_nested_zips_by_signature() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("dist");
    fs::create_dir(&input).unwrap();
    let nested = tmp.path().join("nested.zip");
    zip_file(&nested, &[("inside.txt", b"inside")]);
    fs::copy(&nested, input.join("engine.bin")).unwrap();
    let report = inspect_path(&input, &Limits::default()).unwrap();
    assert_eq!(report.containers.len(), 2);
    assert_eq!(report.containers[0].kind, "directory");
    assert_eq!(report.containers[1].files[0].relative_path, "inside.txt");
    assert_eq!(report.containers[1].files[0].raw_bytes, 6);
}

#[test]
fn directory_and_tar_gz_are_supported_without_per_member_compression_fiction() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("dist");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("b.txt"), b"bb").unwrap();
    fs::write(input.join("a.txt"), b"a").unwrap();
    let dir_report = inspect_path(&input, &Limits::default()).unwrap();
    assert_eq!(dir_report.containers[0].kind, "directory");
    assert_eq!(dir_report.containers[0].summary.raw_bytes, 3);
    assert!(
        dir_report.containers[0]
            .files
            .iter()
            .all(|f| f.zip_compressed_bytes.is_none())
    );

    let tar_path = tmp.path().join("dist.tar.gz");
    let encoder = GzEncoder::new(fs::File::create(&tar_path).unwrap(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for name in ["b.txt", "a.txt"] {
        let bytes = fs::read(input.join(name)).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("out/{name}"), &bytes[..])
            .unwrap();
    }
    builder.into_inner().unwrap().finish().unwrap();
    let tar_report = inspect_path(&tar_path, &Limits::default()).unwrap();
    assert_eq!(tar_report.containers[0].kind, "tar.gz");
    assert_eq!(tar_report.containers[0].summary.file_count, 2);
    assert_eq!(tar_report.containers[0].summary.raw_bytes, 3);
    assert_eq!(tar_report.containers[0].summary.zip_compressed_bytes, None);
    assert!(
        tar_report.containers[0]
            .files
            .iter()
            .all(|f| f.zip_compressed_bytes.is_none())
    );
}

#[cfg(unix)]
#[test]
fn directory_symlinks_are_not_followed() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("dist");
    fs::create_dir(&input).unwrap();
    fs::write(tmp.path().join("outside.txt"), b"outside").unwrap();
    symlink(tmp.path().join("outside.txt"), input.join("linked.txt")).unwrap();
    let report = inspect_path(&input, &Limits::default()).unwrap();
    assert_eq!(report.containers[0].summary.file_count, 0);
    assert_eq!(report.containers[0].issues.len(), 1);
    assert_eq!(report.containers[0].issues[0].kind, "symlink_skipped");
}

#[test]
fn html_is_offline_escaped_and_sensitive_content_is_not_previewed() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("dist");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("index.html"), b"<script>alert('html')</script>").unwrap();
    fs::write(
        input.join("note.txt"),
        b"</pre><script>alert('text')</script>",
    )
    .unwrap();
    fs::write(
        input.join("project.private.config.json"),
        b"{\"private_token\":\"DO_NOT_EMBED\"}",
    )
    .unwrap();
    let mut image = image::RgbImage::new(1, 1);
    image.put_pixel(0, 0, image::Rgb([255, 0, 0]));
    image.save(input.join("pixel.png")).unwrap();

    let report = inspect_path(&input, &Limits::default()).unwrap();
    let html = render_html(&report).unwrap();
    assert!(!html.contains("<script"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("DO_NOT_EMBED"));
    assert!(html.contains("default-src 'none'"));
    assert!(html.contains("data:image/png;base64,"));
    let sensitive = report.containers[0]
        .files
        .iter()
        .find(|f| f.relative_path == "project.private.config.json")
        .unwrap();
    assert_eq!(sensitive.preview.kind, "suppressed_sensitive");
    assert_eq!(sensitive.raw_bytes, 32);
}

#[test]
fn corrupt_input_is_an_input_error_not_a_fake_empty_report() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("broken.zip");
    fs::write(&path, b"PK\x03\x04not a complete zip").unwrap();
    let error = inspect_path(&path, &Limits::default()).unwrap_err();
    assert!(error.is_input());
    assert!(error.to_string().contains("ZIP"));
}

#[test]
fn archive_and_preview_limits_are_enforced() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("entries.zip");
    zip_file(&path, &[("a.txt", b"a"), ("b.txt", b"b")]);
    let error = inspect_path(
        &path,
        &Limits {
            max_entries: 1,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(error.is_input());
    assert!(error.to_string().contains("entry limit"));
}

#[test]
fn godot_and_unity_formats_are_explicitly_not_parsed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("game.zip");
    zip_file(
        &path,
        &[("game.pck", b"GDPC"), ("scene.unity", b"%YAML 1.1")],
    );
    let report = inspect_path(&path, &Limits::default()).unwrap();
    let files = &report.containers[0].files;
    assert_eq!(files[0].kind, "godot_package");
    assert!(files[0].preview.limitation.contains("not parsed"));
    assert_eq!(files[1].kind, "unity_scene");
    assert!(files[1].preview.limitation.contains("not parsed"));
}
