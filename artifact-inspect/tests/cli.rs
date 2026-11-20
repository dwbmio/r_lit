use flate2::{Compression, write::GzEncoder};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::Command;

fn make_tar_gz(path: &std::path::Path) {
    let encoder = GzEncoder::new(fs::File::create(path).unwrap(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let bytes = b"ordinary fixture";
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "out/readme.txt", &bytes[..])
        .unwrap();
    builder.into_inner().unwrap().finish().unwrap();
}

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

    fs::write(input.join("hello.txt"), b"hello again").unwrap();
    let repeated = Command::new(binary())
        .arg(&input)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        repeated.status.success(),
        "ordinary reports must remain replaceable: stderr={}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    let repeated_envelope: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert_eq!(repeated_envelope["ok"], true);
    let repeated_report: Value =
        serde_json::from_slice(&fs::read(output_dir.join("artifact-inspect.json")).unwrap())
            .unwrap();
    assert_eq!(
        repeated_report["containers"][0]["files"][0]["raw_bytes"],
        11
    );
    assert!(output_dir.join("artifact-inspect.log").is_file());
}

#[test]
fn truncated_tar_gz_exits_input_and_emits_no_success_reports() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("truncated.tar.gz");
    make_tar_gz(&input);
    let healthy = fs::read(&input).unwrap();
    fs::write(&input, &healthy[..healthy.len() - 8]).unwrap();
    let output_dir = tmp.path().join("inspection");
    let output = Command::new(binary())
        .arg(&input)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["kind"], "input");
    assert!(!output_dir.join("artifact-inspect.json").exists());
    assert!(!output_dir.join("artifact-inspect.html").exists());
    let log = fs::read_to_string(output_dir.join("artifact-inspect.log")).unwrap();
    assert!(log.contains(" ERROR"), "{log}");
    assert!(log.to_ascii_lowercase().contains("gzip"), "{log}");
}

#[cfg(unix)]
#[test]
fn existing_output_symlinks_and_hardlinks_cannot_modify_input_sentinels() {
    use std::os::unix::fs::symlink;

    for output_name in [
        "artifact-inspect.json",
        "artifact-inspect.html",
        "artifact-inspect.log",
    ] {
        for link_kind in ["symlink", "hardlink"] {
            let tmp = tempfile::tempdir().unwrap();
            let input = tmp.path().join("dist");
            fs::create_dir(&input).unwrap();
            let sentinel = input.join("sentinel.txt");
            let original = b"ordinary input sentinel";
            fs::write(&sentinel, original).unwrap();
            let output_dir = tmp.path().join("inspection");
            fs::create_dir(&output_dir).unwrap();
            let output_path = output_dir.join(output_name);
            if link_kind == "symlink" {
                symlink(&sentinel, &output_path).unwrap();
            } else {
                fs::hard_link(&sentinel, &output_path).unwrap();
            }

            let output = Command::new(binary())
                .arg(&input)
                .arg("--output-dir")
                .arg(&output_dir)
                .arg("--json")
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{output_name} {link_kind}: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(envelope["ok"], false);
            assert_eq!(envelope["error"]["kind"], "usage");
            assert_eq!(fs::read(&sentinel).unwrap(), original);
            assert_eq!(
                fs::read_dir(&output_dir).unwrap().count(),
                1,
                "no sibling output may be written after a target conflict"
            );
        }
    }
}

#[test]
fn archive_input_cannot_alias_a_report_destination() {
    let tmp = tempfile::tempdir().unwrap();
    let output_dir = tmp.path().join("inspection");
    fs::create_dir(&output_dir).unwrap();
    let input = output_dir.join("artifact-inspect.json");
    let file = fs::File::create(&input).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("hello.txt", options).unwrap();
    writer.write_all(b"hello").unwrap();
    writer.finish().unwrap();
    let original = fs::read(&input).unwrap();

    let output = Command::new(binary())
        .arg(&input)
        .arg("--output-dir")
        .arg(&output_dir)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(envelope["error"]["kind"], "usage");
    assert_eq!(fs::read(&input).unwrap(), original);
}
