use image::GenericImageView;
use std::process::Command;
use ui_trim::make_synthetic_ui_asset;

#[test]
fn cli_smoke_trims_synthetic_png_and_prints_ai_friendly_json() {
    let dir = std::env::temp_dir().join(format!(
        "ui_trim_cli_smoke_{}_{}",
        std::process::id(),
        unique_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let input = dir.join("raw.png");
    let output = dir.join("clean.png");
    make_synthetic_ui_asset(320, 240)
        .save(&input)
        .expect("save input");

    let bin = env!("CARGO_BIN_EXE_ui-trim");
    let out = Command::new(bin)
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--padding")
        .arg("4")
        .arg("--feather")
        .arg("1")
        .arg("--remove-red-guides")
        .arg("--json")
        .output()
        .expect("run ui-trim");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(output.exists(), "output png should exist");

    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json stdout");
    // r_lit CLI contract envelope.
    assert_eq!(meta["ok"], true);
    assert_eq!(meta["tool"], "ui-trim");
    assert_eq!(meta["artifacts"][0]["role"], "image");
    assert_eq!(meta["artifacts"][0]["sha256"].as_str().unwrap().len(), 64);
    let report = &meta["data"];
    assert_eq!(report["options"]["implementation"], "pure_rust_cpu");
    assert!(report["timings_ms"]["total"].as_f64().unwrap() >= 0.0);
    assert!(report["throughput_mp_s"].as_f64().unwrap() >= 0.0);

    let img = image::open(&output).expect("read output");
    let (w, h) = img.dimensions();
    assert!(w < 320 && h < 240, "output should be tightly cropped");

    let _ = std::fs::remove_dir_all(&dir);
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos()
}

#[test]
fn cli_missing_input_reports_json_error_with_exit_3() {
    let bin = env!("CARGO_BIN_EXE_ui-trim");
    let out = Command::new(bin)
        .args(["-i", "/nonexistent/raw.png", "-o", "/tmp/never.png", "--json"])
        .output()
        .expect("run ui-trim");
    assert_eq!(out.status.code(), Some(3));
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json stdout");
    assert_eq!(meta["ok"], false);
    assert_eq!(meta["error"]["code"], "input");
}

#[test]
fn cli_schema_describes_arguments() {
    let bin = env!("CARGO_BIN_EXE_ui-trim");
    let out = Command::new(bin).arg("__schema").output().expect("run ui-trim");
    assert!(out.status.success());
    let schema: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json schema");
    let props = &schema["commands"][0]["input_schema"]["properties"];
    assert_eq!(props["input"]["x-cli"]["io"], "input-file");
    assert_eq!(props["padding"]["type"], "integer");
    assert_eq!(schema["commands"][0]["x-effects"], "pure");
}
