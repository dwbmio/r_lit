use artifact_inspect::{
    InspectError, Limits, flush_runlog, inspect_path, validate_output_dir, write_reports,
};
use clap::Parser;
use rlit_cli::{CommandSpec, Contract, Effects, ErrorKind, Io, runlog};
use std::path::{Path, PathBuf};

static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[CommandSpec::new("", Effects::Pure)
        .io(&[("input", Io::InputPath), ("output_dir", Io::OutputDir)])
        .note("Offline bounded inspection. Writes artifact-inspect.json, artifact-inspect.html, and artifact-inspect.log only to --output-dir; never follows input symlinks or writes inside a directory input. Nested ZIPs are detected by name/signature at any supported path. Totals are per container, not cross-layer transfer estimates.")],
};

#[derive(Parser)]
#[command(
    name = "artifact-inspect",
    version,
    about = "Inspect build directories, ZIP, and TAR.GZ offline / 离线检查构建目录、ZIP 和 TAR.GZ",
    long_about = "Produce stable per-file inventories with independent outer/nested-container accounting and safe bounded previews. No archived code is executed; no network access is used. ZIP compressed bytes are entry payload sizes, not network transfer sizes. Godot/Unity formats are classified but not parsed.\n\n生成稳定的逐文件清单，外层与嵌套容器独立计数，并提供有界安全预览。不执行产物代码、不访问网络。ZIP 压缩字节是条目数据量，不是网络传输量；Godot/Unity 专有格式只做分类，不做解析。",
    after_help = "Examples:\n  artifact-inspect dist/ --output-dir inspection/\n  artifact-inspect game.zip -o inspection/ --json\n  artifact-inspect package.tar.gz -o inspection/ --max-depth 4\n\nMachine contract: --json emits one rlit_cli envelope; __schema exports CLI JSON Schema.\nExit codes: 0 ok, 1 runtime, 2 usage, 3 input."
)]
struct Cli {
    /// Print one machine-readable JSON envelope / 输出机器调用 JSON 信封
    #[arg(long, global = true)]
    json: bool,

    /// Build directory, ZIP, TAR.GZ, or TGZ input / 构建目录、ZIP、TAR.GZ 或 TGZ 输入
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Controlled report output directory; must be outside a directory input / 受控报告目录，不得位于目录输入内
    #[arg(short, long, value_name = "DIR")]
    output_dir: PathBuf,

    /// Maximum regular-file entries across all layers / 所有层普通文件条目上限
    #[arg(long, default_value_t = 100_000)]
    max_entries: usize,

    /// Maximum sum of raw bytes processed across layers / 各层处理原始字节工作量上限
    #[arg(long, default_value_t = 2_000_000_000)]
    max_total_raw_bytes: u64,

    /// Maximum uncompressed bytes for one nested ZIP / 单个嵌套 ZIP 最大原始字节
    #[arg(long, default_value_t = 67_108_864)]
    max_nested_bytes: u64,

    /// Maximum nested ZIP depth; 0 disables nested inspection / 嵌套 ZIP 最大深度，0 表示不展开
    #[arg(long, default_value_t = 4)]
    max_depth: usize,

    /// Maximum source bytes embedded per preview / 单个预览最多嵌入的源字节
    #[arg(long, default_value_t = 65_536)]
    max_preview_bytes: u64,

    /// Maximum source bytes embedded across all previews / 全部预览最多嵌入的源字节
    #[arg(long, default_value_t = 8_388_608)]
    max_total_preview_bytes: u64,

    /// Maximum bitmap dimensions in pixels / 位图预览最大像素数
    #[arg(long, default_value_t = 16_777_216)]
    max_image_pixels: u64,
}

fn main() {
    runlog::init();
    let cli: Cli = CONTRACT.parse();
    let validated_output = validate_output_dir(&cli.input, &cli.output_dir);
    let log_path = validated_output
        .as_ref()
        .ok()
        .map(|dir| dir.join("artifact-inspect.log"));
    let mut result = match validated_output {
        Ok(output_dir) => run(&cli, &output_dir),
        Err(error) => Err(error),
    };
    if let Err(error) = &result {
        log::error!("{error}");
    }
    if let Some(log_path) = &log_path {
        match flush_runlog(log_path, &runlog::standard_header()) {
            Ok(()) => rlit_cli::add_artifact(log_path, "log"),
            Err(error) => {
                eprintln!("[artifact-inspect ERROR] {error}");
                if result.is_ok() {
                    result = Err(error);
                }
            }
        }
    }
    rlit_cli::finish(result, |error| {
        if error.is_usage() {
            ErrorKind::Usage
        } else if error.is_input() {
            ErrorKind::Input
        } else {
            ErrorKind::Runtime
        }
    });
}

fn run(cli: &Cli, output_dir: &Path) -> Result<(), InspectError> {
    let limits = Limits {
        max_entries: cli.max_entries,
        max_total_raw_bytes: cli.max_total_raw_bytes,
        max_nested_bytes: cli.max_nested_bytes,
        max_depth: cli.max_depth,
        max_preview_bytes: cli.max_preview_bytes,
        max_total_preview_bytes: cli.max_total_preview_bytes,
        max_image_pixels: cli.max_image_pixels,
    };
    log::info!(
        "inspecting {} offline; no artifact code will be executed",
        cli.input.display()
    );
    let report = inspect_path(&cli.input, &limits)?;
    let (json_path, html_path) = write_reports(&report, output_dir)?;
    rlit_cli::add_artifact(&json_path, "inventory-json");
    rlit_cli::add_artifact(&html_path, "inventory-html");
    let issue_count: usize = report.containers.iter().map(|c| c.issues.len()).sum();
    rlit_cli::set_data(serde_json::json!({
        "schema_version": report.schema_version,
        "input": report.input,
        "report_json": json_path,
        "report_html": html_path,
        "issue_count": issue_count,
        "containers": report.containers.iter().map(|container| serde_json::json!({
            "id": container.id,
            "path": container.path,
            "kind": container.kind,
            "depth": container.depth,
            "summary": container.summary,
        })).collect::<Vec<_>>(),
        "size_semantics": "raw bytes are uncompressed file sizes; ZIP compressed bytes are entry payload bytes, not transfer bytes; container totals are independent",
        "engine_formats": "Godot/Unity formats are classified but not parsed; no asset provenance is claimed"
    }));
    if issue_count > 0 {
        rlit_cli::warn(format!(
            "{issue_count} skipped/error issue(s) are recorded in the reports"
        ));
    }
    log::info!("wrote {} and {}", json_path.display(), html_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_contract_is_complete() {
        let problems = rlit_cli::check_contract(&CONTRACT, &Cli::command());
        assert!(
            problems.is_empty(),
            "contract problems:\n{}",
            problems.join("\n")
        );
    }

    #[test]
    fn rejects_output_inside_directory_input_before_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let input = tmp.path().join("dist");
        std::fs::create_dir(&input).unwrap();
        let error = validate_output_dir(&input, &input.join("reports")).unwrap_err();
        assert!(error.is_usage());
        assert!(!input.join("reports").exists());
    }
}
