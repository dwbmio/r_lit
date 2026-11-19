use serde_json::Value;
use std::fs;
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_artifact-inspect")
}

#[test]
fn schema_and_corrupt_input_follow_the_machine_contract() {
    let schema = Command::new(binary()).arg("__schema").output().unwrap();
    assert!(schema.status.success());
    let schema_json: Value = serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(schema_json["contract"], 1);
    assert_eq!(schema_json["tool"], "artifact-inspect");
    assert_eq!(schema_json["commands"][0]["x-effects"], "pure");

    let tmp = tempfile::tempdir().unwrap();
    let broken = tmp.path().join("broken.zip");
    fs::write(&broken, b"PK\x03\x04broken").unwrap();
    let output = Command::new(binary())
        .arg(&broken)
        .arg("--output-dir")
        .arg(tmp.path().join("report"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["kind"], "input");
    assert!(!tmp.path().join("report/artifact-inspect.json").exists());
    assert!(tmp.path().join("report/artifact-inspect.log").is_file());
}

#[test]
fn rejected_output_inside_input_writes_nothing_there() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("dist");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("hello.txt"), b"hello").unwrap();
    let forbidden = input.join("reports");
    let output = Command::new(binary())
        .arg(&input)
        .arg("--output-dir")
        .arg(&forbidden)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["error"]["kind"], "usage");
    assert!(
        !forbidden.exists(),
        "even a sidecar log is forbidden inside the input"
    );
}

#[test]
fn successful_cli_writes_json_html_and_log_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("dist");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("hello.txt"), b"hello").unwrap();
    let output_dir = tmp.path().join("inspection");
    let output = Command::new(binary())
        .arg(&input)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["ok"], true);
    assert_eq!(
        envelope["data"]["containers"][0]["summary"]["file_count"],
        1
    );
    let roles: Vec<_> = envelope["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["role"].as_str().unwrap())
        .collect();
    assert!(roles.contains(&"inventory-json"));
    assert!(roles.contains(&"inventory-html"));
    assert!(roles.contains(&"log"));
    let report: Value =
        serde_json::from_slice(&fs::read(output_dir.join("artifact-inspect.json")).unwrap())
            .unwrap();
    assert_eq!(report["schema_version"], 1);
    assert!(
        fs::read_to_string(output_dir.join("artifact-inspect.html"))
            .unwrap()
            .contains("hello.txt")
    );
}
