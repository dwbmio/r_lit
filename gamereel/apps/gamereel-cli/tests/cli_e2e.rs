//! gamereel-cli end-to-end tests.
//!
//! Critically, this binary links proto-puzzle and proto-bubble — the
//! e2e assertion `list-protocols` shows "puzzle" + "bubble" is what
//! validates that `inventory::submit!` from sibling crates is actually
//! discoverable through the workspace link line.
//!
//! These tests are NOT marked `#[ignore]` (no GPU required) — they
//! catch link-time inventory regressions in regular `cargo test`.

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use tempfile::NamedTempFile;

fn cli() -> Command {
    Command::cargo_bin("gamereel").expect("gamereel binary built")
}

#[test]
fn list_protocols_shows_puzzle_and_bubble() {
    cli()
        .arg("list-protocols")
        .assert()
        .success()
        .stdout(predicate::str::contains("puzzle"))
        .stdout(predicate::str::contains("bubble"));
}

#[test]
fn list_protocols_json_is_valid() {
    let out = cli().args(["--json", "list-protocols"]).output().expect("run");
    assert!(out.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("valid JSON");
    let names: Vec<&str> = parsed["protocols"]
        .as_array()
        .expect("protocols array")
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"puzzle"));
    assert!(names.contains(&"bubble"));
    assert!(parsed["ok"].as_bool() == Some(true));
}

#[test]
fn render_unknown_protocol_returns_helpful_error() {
    let mut tmp = NamedTempFile::new().expect("tmpfile");
    tmp.write_all(b"dummy").expect("write");
    cli()
        .args(["render", "--protocol", "does-not-exist", "--input"])
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("does-not-exist"))
        .stderr(predicate::str::contains("known:"));
}

#[test]
fn render_with_known_protocol_succeeds_and_emits_metadata() {
    let mut tmp = NamedTempFile::new().expect("tmpfile");
    tmp.write_all(b"some-bytes-for-puzzle").expect("write");
    cli()
        .args(["render", "--protocol", "puzzle", "--input"])
        .arg(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("puzzle"))
        .stdout(predicate::str::contains("input bytes:"))
        .stdout(predicate::str::contains("frames:"));
}

#[test]
fn render_json_payload_is_structurally_valid() {
    let mut tmp = NamedTempFile::new().expect("tmpfile");
    tmp.write_all(b"abc").expect("write");
    let out = cli()
        .args(["--json", "render", "--protocol", "bubble", "--input"])
        .arg(tmp.path())
        .output()
        .expect("run");
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("valid JSON");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["protocol"], "bubble");
    assert_eq!(parsed["input_bytes"], 3);
    assert!(parsed["suggested_filename"].as_str().unwrap().contains("bubble"));
}
