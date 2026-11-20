//! Opt-in local acceptance against the Lead's independently generated C4 inventory.
//! Run with C4_INPUT and C4_REFERENCE set; no package content is modified.

use artifact_inspect::{Limits, inspect_path};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
#[ignore = "requires the frozen local C4 package and independent reference JSON"]
fn matches_independent_c4_reference_path_for_path_and_byte_for_byte() {
    let input = PathBuf::from(std::env::var_os("C4_INPUT").expect("C4_INPUT is required"));
    let reference =
        PathBuf::from(std::env::var_os("C4_REFERENCE").expect("C4_REFERENCE is required"));
    let reference_json: Value =
        serde_json::from_slice(&std::fs::read(&reference).unwrap()).unwrap();
    let expected: BTreeMap<(String, String), (u64, Option<u64>)> = reference_json["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| {
            (
                (
                    file["layer"].as_str().unwrap().to_string(),
                    file["path"].as_str().unwrap().to_string(),
                ),
                (
                    file["bytes"].as_u64().unwrap(),
                    file["zip_compressed_bytes"].as_u64(),
                ),
            )
        })
        .collect();

    let report = inspect_path(&input, &Limits::default()).unwrap();
    let actual: BTreeMap<(String, String), (u64, Option<u64>)> = report
        .containers
        .iter()
        .flat_map(|container| {
            container.files.iter().map(move |file| {
                (
                    (
                        if container.kind == "tar.gz" {
                            "tar".to_string()
                        } else {
                            container.kind.clone()
                        },
                        file.relative_path.clone(),
                    ),
                    (file.raw_bytes, file.zip_compressed_bytes),
                )
            })
        })
        .collect();

    assert_eq!(actual.len(), 164, "actual file rows");
    assert_eq!(
        expected.len(),
        actual.len(),
        "reference and actual row counts"
    );
    if actual != expected {
        let missing: Vec<_> = expected
            .keys()
            .filter(|key| !actual.contains_key(*key))
            .collect();
        let extra: Vec<_> = actual
            .keys()
            .filter(|key| !expected.contains_key(*key))
            .collect();
        let size_mismatches: Vec<_> = expected
            .iter()
            .filter_map(|(key, expected_sizes)| {
                actual.get(key).and_then(|actual_sizes| {
                    (actual_sizes != expected_sizes).then_some((key, expected_sizes, actual_sizes))
                })
            })
            .collect();
        panic!(
            "C4 mismatch: missing={missing:?}, extra={extra:?}, size_mismatches={size_mismatches:?}"
        );
    }
    assert_eq!(report.containers[0].summary.file_count, 25);
    assert_eq!(report.containers[0].summary.raw_bytes, 7_696_698);
    assert_eq!(report.containers[0].summary.zip_compressed_bytes, None);
    assert_eq!(report.containers[1].summary.file_count, 139);
    assert_eq!(report.containers[1].summary.raw_bytes, 2_093_130);
    assert_eq!(
        report.containers[1].summary.zip_compressed_bytes,
        Some(1_979_382)
    );

    for path in ["out/project.private.config.json", "env/runtime.env"] {
        let private = report
            .containers
            .iter()
            .flat_map(|container| container.files.iter())
            .find(|file| file.relative_path == path)
            .unwrap();
        assert_eq!(private.preview.kind, "suppressed_sensitive");
        assert!(private.preview.text.is_none());
        assert!(private.preview.data_uri.is_none());
        assert!(private.preview.media_type.is_none());
        println!(
            "C4 private row {path}: kind={}, text_present=false, image_present=false",
            private.preview.kind
        );
    }
    println!("C4 reference match: 164/164 paths, raw bytes, and ZIP compressed-byte values");
    println!("C4 layer totals: outer=25/7696698 B; nested=139/2093130 B raw/1979382 B ZIP payload");
}
