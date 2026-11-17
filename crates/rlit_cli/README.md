# rlit_cli

Shared machine-call contract and dual-sink sidecar logging crate for `r_lit` short-running CLI tools.

See [`docs/cli-contract.md`](../../docs/cli-contract.md) for the full contract specification.

## Capabilities

- **Global `--json` envelope**: ensures stdout emits exactly one structured JSON object (`contract`, `ok`, `tool`, `version`, `command`, `data`, `artifacts`, `error`, optional `warnings`) on both success and failure.
- **Standardized exit codes**: `0` ok, `1` runtime, `2` usage, `3` input, `4` external, `5` partial.
- **`__schema` introspection**: hidden subcommand exporting JSON Schema (draft 2020-12) plus `x-cli`, `x-effects`, `x-env`, and `x-stdin` annotations for every leaf subcommand.
- **Artifact receipts**: `add_artifact(path, role)` records absolute path, byte size, and SHA-256 digest at exit.
- **Dual-sink sidecar logging (`rlit_cli::runlog`)**: buffers full `DEBUG+` logs in memory and echoes `INFO+` (`DEBUG+` in debug builds) to stdout (or stderr under `--json`), flushing to `<output>.log` on completion.

## Usage

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
