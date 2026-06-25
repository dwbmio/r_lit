use std::path::PathBuf;
use ui_trim::{default_options, make_synthetic_ui_asset, trim_file};

fn main() -> ui_trim::Result<()> {
    let dir = std::env::temp_dir().join(format!("ui_trim_smoke_{}", std::process::id()));
    let input: PathBuf = dir.join("raw.png");
    let output: PathBuf = dir.join("clean.png");
    std::fs::create_dir_all(&dir).expect("create smoke dir");

    make_synthetic_ui_asset(512, 384)
        .save(&input)
        .expect("write synthetic input");

    let report = trim_file(&input, &output, &default_options())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&report).expect("serialize report")
    );

    assert!(output.exists(), "smoke output should exist");
    assert!(
        report.output_width < report.input_width && report.output_height < report.input_height,
        "synthetic asset should be trimmed"
    );
    Ok(())
}
