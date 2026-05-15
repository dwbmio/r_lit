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
    assert_eq!(meta["ok"], true);
    assert_eq!(meta["options"]["implementation"], "pure_rust_cpu");
    assert!(meta["timings_ms"]["total"].as_f64().unwrap() >= 0.0);
    assert!(meta["throughput_mp_s"].as_f64().unwrap() >= 0.0);

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
