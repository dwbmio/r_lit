//! Raw TAR headers deliberately preserve spelling, including unsafe paths.
use artifact_inspect::{Limits, inspect_path, normalize_relative_path};
use flate2::{Compression, write::GzEncoder};
use std::{fs, io::Write, path::Path};

fn archive(path: &Path, entries: &[(&[u8], tar::EntryType, &[u8])]) {
    let encoder = GzEncoder::new(
        fs::File::create(path).expect("create fixture"),
        Compression::default(),
    );
    let mut builder = tar::Builder::new(encoder);
    for (name, kind, data) in entries {
        let mut header = tar::Header::new_gnu();
        header.as_mut_bytes()[..100].fill(0);
        header.as_mut_bytes()[..name.len()].copy_from_slice(name);
        header.set_entry_type(*kind);
        header.set_mode(0o755);
        header.set_size(data.len() as u64);
        if kind.is_symlink() || kind.is_hard_link() {
            header.set_link_name("outside.txt").expect("link target");
        }
        header.set_cksum();
        builder.append(&header, *data).expect("append raw member");
    }
    builder
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip");
}

#[test]
fn canonical_tar_root_directories_are_not_inventory_rows() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for root in [b".".as_slice(), b"./"] {
        let path = tmp.path().join("root.tar.gz");
        archive(
            &path,
            &[
                (root, tar::EntryType::Directory, b""),
                (b"./assets/", tar::EntryType::Directory, b""),
                (b"./assets/a.txt", tar::EntryType::Regular, b"abc"),
            ],
        );
        let report =
            inspect_path(&path, &Limits::default()).expect("standard root marker accepted");
        let container = &report.containers[0];
        assert_eq!(container.summary.file_count, 1);
        assert_eq!(container.summary.raw_bytes, 3);
        assert!(container.issues.is_empty());
        assert_eq!(container.files[0].relative_path, "assets/a.txt");
        assert_eq!(container.files[0].zip_compressed_bytes, None);
    }
}

#[test]
fn nul_and_zip_directory_paths_do_not_gain_tar_root_exemption() {
    for path in [
        "",
        ".",
        "./",
        "bad\0path",
        "/absolute",
        "../escape",
        "C:/escape",
    ] {
        assert!(
            normalize_relative_path(Path::new(path))
                .expect_err("unsafe or empty path rejected")
                .is_input()
        );
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    for name in ["./", "../escape/", "/absolute/", "C:/escape/"] {
        let path = tmp.path().join("unsafe.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path).expect("create zip"));
        writer
            .add_directory(name, zip::write::SimpleFileOptions::default())
            .expect("raw ZIP directory");
        writer.finish().expect("finish zip");
        assert!(
            inspect_path(&path, &Limits::default())
                .expect_err("ZIP directory validation unchanged")
                .is_input()
        );
    }
}

#[test]
fn tar_without_root_marker_keeps_identical_file_metadata() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("no-root.tar.gz");
    archive(
        &path,
        &[
            (b"assets/", tar::EntryType::Directory, b""),
            (b"assets/a.txt", tar::EntryType::Regular, b"abc"),
        ],
    );
    let report = inspect_path(&path, &Limits::default()).expect("no marker required");
    assert_eq!(report.containers[0].summary.file_count, 1);
    assert_eq!(report.containers[0].files[0].relative_path, "assets/a.txt");
    assert_eq!(report.containers[0].files[0].raw_bytes, 3);
}

#[test]
fn tar_root_exception_does_not_bypass_other_path_or_type_validation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    for name in [
        b"".as_slice(),
        b"/",
        b"/absolute",
        b"../escape",
        b"./../escape",
        b"C:/escape",
        b"C:\\escape",
        b"././",
        b".\\",
    ] {
        let path = tmp.path().join("unsafe.tar.gz");
        archive(
            &path,
            &[
                (b".", tar::EntryType::Directory, b""),
                (name, tar::EntryType::Directory, b""),
            ],
        );
        let error = inspect_path(&path, &Limits::default()).expect_err("unsafe directory rejected");
        assert!(error.is_input(), "{name:?}: {error}");
    }
    for name in [b"".as_slice(), b".", b"./"] {
        for kind in [
            tar::EntryType::Regular,
            tar::EntryType::Symlink,
            tar::EntryType::Link,
        ] {
            let path = tmp.path().join("not-directory.tar.gz");
            archive(&path, &[(name, kind, b"")]);
            assert!(
                inspect_path(&path, &Limits::default())
                    .expect_err("empty normalized non-directory rejected")
                    .is_input()
            );
        }
    }
}

#[test]
fn tar_root_marker_keeps_links_limits_and_gzip_validation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("links.tar.gz");
    archive(
        &path,
        &[
            (b"./", tar::EntryType::Directory, b""),
            (b"soft", tar::EntryType::Symlink, b""),
            (b"hard", tar::EntryType::Link, b""),
            (b"a", tar::EntryType::Regular, b"abc"),
            (b"b", tar::EntryType::Regular, b"def"),
        ],
    );
    let report = inspect_path(&path, &Limits::default()).expect("links skipped not followed");
    assert_eq!(report.containers[0].summary.file_count, 2);
    assert_eq!(report.containers[0].issues.len(), 2);
    assert!(
        report.containers[0]
            .issues
            .iter()
            .all(|issue| issue.kind == "link_skipped")
    );
    for limits in [
        Limits {
            max_entries: 1,
            ..Limits::default()
        },
        Limits {
            max_total_raw_bytes: 5,
            ..Limits::default()
        },
    ] {
        assert!(
            inspect_path(&path, &limits)
                .expect_err("limit enforced")
                .is_input()
        );
    }
    let mut bytes = fs::read(&path).expect("read gzip");
    bytes.truncate(bytes.len() - 8);
    fs::File::create(&path)
        .expect("truncate fixture")
        .write_all(&bytes)
        .expect("write truncated gzip");
    assert!(
        inspect_path(&path, &Limits::default())
            .expect_err("gzip trailer validated")
            .is_input()
    );
}
