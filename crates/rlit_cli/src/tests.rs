use super::*;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "demo", about = "Demo tool")]
struct Demo {
    /// JSON output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Pack things
    Pack {
        /// Input directory
        input: std::path::PathBuf,
        /// Max size
        #[arg(long, default_value_t = 4096)]
        max_size: u32,
        /// Ratio
        #[arg(long, default_value_t = 1.5)]
        ratio: f32,
        /// Trim
        #[arg(long)]
        trim: bool,
        /// Effects
        #[arg(short, long = "effect")]
        effects: Vec<String>,
        /// Mode
        #[arg(long, value_enum, default_value = "fast")]
        mode: Mode,
    },
    /// Hidden
    #[command(hide = true)]
    Secret,
}

#[derive(Clone, clap::ValueEnum)]
enum Mode {
    Fast,
    Slow,
}

static C: Contract = Contract {
    tool: "demo",
    version: "1.2.3",
    commands: &[CommandSpec::new("pack", Effects::Pure).io(&[("input", Io::InputDir)])],
};

#[test]
fn schema_shape() {
    use clap::CommandFactory;
    let s = schema_for(&C, &Demo::command());
    assert_eq!(s["contract"], 1);
    let cmds = s["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1, "hidden subcommand must be skipped");
    let p = &cmds[0]["input_schema"]["properties"];
    assert!(p.get("json").is_none());
    assert_eq!(p["input"]["type"], "string");
    assert_eq!(p["input"]["x-cli"]["positional"], 1);
    assert_eq!(p["input"]["x-cli"]["io"], "input-dir");
    assert_eq!(p["max_size"]["type"], "integer");
    assert_eq!(p["max_size"]["default"], 4096);
    assert_eq!(p["ratio"]["type"], "number");
    assert_eq!(p["trim"]["type"], "boolean");
    assert_eq!(p["effects"]["type"], "array");
    assert_eq!(p["effects"]["x-cli"]["long"], "--effect");
    assert_eq!(p["mode"]["enum"], serde_json::json!(["fast", "slow"]));
    assert_eq!(cmds[0]["input_schema"]["required"], serde_json::json!(["input"]));
    assert_eq!(cmds[0]["x-effects"], "pure");
    assert!(check_contract(&C, &Demo::command()).is_empty());
}

#[test]
fn envelope_success_and_failure() {
    let _: Demo = C.parse_from(["demo", "pack", "in", "--json"]);
    assert!(json_mode());
    set_data(serde_json::json!({"n": 1}));
    let tmp = std::env::temp_dir().join(format!("rlit_cli_art_{}", std::process::id()));
    std::fs::write(&tmp, b"abc").unwrap();
    add_artifact(&tmp, "out");
    let (env, code) = build_envelope();
    assert_eq!(code, 0);
    assert_eq!(env["ok"], true);
    assert_eq!(env["tool"], "demo");
    assert_eq!(env["command"], "pack");
    assert_eq!(env["data"]["n"], 1);
    assert_eq!(env["artifacts"][0]["bytes"], 3);
    assert_eq!(
        env["artifacts"][0]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    set_failure(ErrorKind::Partial, "1 of 2 failed");
    let (env, code) = build_envelope();
    assert_eq!(code, 5);
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "partial");
    assert_eq!(env["data"]["n"], 1, "data survives partial failure");
    std::fs::remove_file(tmp).ok();
}
