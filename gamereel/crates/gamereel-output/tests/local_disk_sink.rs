//! S1-3 sanity: LocalDiskSink writes the bytes to the path it
//! reports back, and DeliveryReceipt fields are populated correctly.

use gamereel_farm::RenderResult;
use gamereel_output::{LocalDiskSink, OutputSink};
use std::time::Duration;

fn fake_result(job_id: &str) -> RenderResult {
    RenderResult {
        job_id: job_id.into(),
        worker_id: "test".into(),
        ok: true,
        output_bytes: 0,
        wall: Duration::from_millis(100),
        render_loop: Duration::from_millis(50),
        error: None,
        tag: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn writes_bytes_returns_path_url() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let sink = LocalDiskSink::new(dir.path());

    let payload = b"fake-mp4-bytes";
    let receipt = sink
        .deliver(&fake_result("battle-001"), payload)
        .await
        .expect("deliver");

    assert_eq!(receipt.sink, "local_disk");
    assert_eq!(receipt.bytes, payload.len() as u64);
    assert_eq!(receipt.job_id, "battle-001");
    let p = std::path::PathBuf::from(&receipt.location);
    assert!(p.exists(), "expected file at {}", p.display());
    let on_disk = std::fs::read(&p).expect("read back");
    assert_eq!(on_disk, payload);
}

#[tokio::test]
async fn unsafe_chars_in_job_id_are_sanitized() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let sink = LocalDiskSink::new(dir.path());
    let receipt = sink
        .deliver(&fake_result("a/b\\c..d:e"), b"x")
        .await
        .expect("deliver");
    let fname = std::path::PathBuf::from(&receipt.location)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    // No path-separator-ish chars survived.
    assert!(!fname.contains('/') && !fname.contains('\\') && !fname.contains(':'));
    // Original alphanumerics still recognizable.
    assert!(fname.contains("a"));
    assert!(fname.contains("b"));
}
