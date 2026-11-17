# rlit_cli

`r_lit` 短时运行 CLI 工具的共享机器调用契约与双 sink 边车日志库。

完整契约规范见根目录 [`docs/cli-contract.md`](../../docs/cli-contract.md)。

## 核心能力

- **全局 `--json` Envelope**：无论成功、失败还是参数错误，确保 stdout 只输出单个 JSON 对象（`contract`、`ok`、`tool`、`version`、`command`、`data`、`artifacts`、`error`、可选 `warnings`）。
- **固定退出码**：`0` 成功、`1` 运行时、`2` 参数错误、`3` 输入错误、`4` 外部服务失败、`5` 批处理部分失败。
- **`__schema` 自描述导出**：隐藏子命令导出每个叶子命令的 JSON Schema（draft 2020-12）与 `x-cli` / `x-effects` / `x-env` / `x-stdin` 元数据。
- **产物回执**：`add_artifact(path, role)` 在退出时自动补充绝对路径、字节大小与 SHA-256 摘要。
- **双 sink 边车日志（`rlit_cli::runlog`）**：内存全量捕获 `DEBUG+` 日志，控制台按级别输出（`--json` 模式下自动切到 stderr），结束时 flush 到 `<output>.log`。

## 接入方式

```toml
[dependencies]
rlit_cli = { path = "../crates/rlit_cli" }
```

```rust
use rlit_cli::{runlog, CommandSpec, Contract, Effects, Io};

static CONTRACT: Contract = Contract {
    tool: env!("CARGO_PKG_NAME"),
    version: env!("CARGO_PKG_VERSION"),
    commands: &[CommandSpec::new("run", Effects::Pure).io(&[("output", Io::OutputFile)])],
};

fn main() {
    runlog::init();
    let cli: Cli = CONTRACT.parse();
    let result = execute(&cli);
    runlog::flush(&log_path, &runlog::standard_header());
    rlit_cli::finish(result, classify_error);
}
```
