# r_lit CLI 机器调用契约 (contract v1)

r_lit 下所有短时 CLI 对 AI / 执行器（titan-wall、CI、脚本）的调用约定。
实现在 `crates/rlit_cli`，各工具以 path 依赖接入：

```toml
rlit_cli = { path = "../crates/rlit_cli" }
```

已接入：`mj_atlas` `img_resize` `ui-trim` `textexture` `bulk_upload`。

## 1. 调用方式

- `--json` 是全局参数，任何子命令都可用。
- 带 `--json` 时 **stdout 只输出一个 JSON 文档**（下面的 envelope），成功、失败、参数错误都一样；人类可读日志全部走 stderr。
- 所有命令非交互：不弹提示、不读 TTY。需要 stdin 的命令在 schema 里有 `x-stdin` 声明，stdin 是终端时直接报 usage 错误。
- `<tool> __schema`（隐藏子命令）输出机器可读的参数说明，见第 4 节。

## 2. Envelope

```json
{
  "contract": 1,
  "ok": true,
  "tool": "mj_atlas",
  "version": "0.6.0",
  "command": "pack",
  "data": { "...": "命令自己的结果" },
  "artifacts": [
    { "path": "/abs/out/atlas.png", "role": "atlas-image", "bytes": 1234, "sha256": "..." }
  ],
  "error": null,
  "warnings": ["可选，非致命提示"]
}
```

| 字段 | 说明 |
|------|------|
| `contract` | 契约版本，破坏性变更时 +1 |
| `ok` | 成功为 `true`；任何失败（含 partial）为 `false` |
| `tool` / `version` | crate 名与版本（`CARGO_PKG_*`） |
| `command` | 子命令路径，空格分隔；无子命令的工具为 `""` |
| `data` | 命令结果；失败时也可能有（如 verify 的问题清单、批处理逐项结果） |
| `artifacts` | 本次实际写出的全部文件，绝对路径 + 大小 + sha256。执行器据此上传和算回执哈希，不必解析 `data` |
| `error` | 失败时 `{"code", "kind", "message"}`；`kind` 恒为下表之一，`code` 通常等于 `kind`，个别命令给更具体的值（如 `verify_failed`） |
| `warnings` | 可选 |

`artifacts[].role` 由工具自定，常见值：`image` `atlas-image` `metadata` `manifest` `log` `bmfont-fnt` `bmfont-atlas`。`role = "log"` 的文件含时间戳，**不参与确定性比较**。

## 3. 退出码

| 退出码 | `error.kind` | 含义 |
|-------|-------------|------|
| 0 | — | 成功 |
| 1 | `runtime` | 运行期意外错误 |
| 2 | `usage` | 参数错误 / 参数组合不合法 |
| 3 | `input` | 输入不存在、不可读或格式不对（也用于 `mj_atlas verify` 校验不一致） |
| 4 | `external` | 外部服务失败（网络、鉴权、对象存储） |
| 5 | `partial` | 批处理部分失败：`ok=false`、`error.code="partial"`，`data` 照常完整给出 |

## 4. `__schema`

```bash
mj_atlas __schema
```

输出：

```json
{
  "contract": 1, "tool": "mj_atlas", "version": "0.6.0",
  "invocation": { "binary": "mj_atlas", "json_flag": "--json", "schema_command": "__schema", "interactive": false },
  "exit_codes": { "0": "ok", "1": "runtime", "2": "usage", "3": "input", "4": "external", "5": "partial" },
  "commands": [
    {
      "name": "pack",
      "argv_prefix": ["pack"],
      "about": "...", "long_about": "...",
      "x-effects": "external",
      "x-env": ["MJ_ATLAS_NO_MIRROR"],
      "x-note": "给 agent 的一句话注意事项",
      "input_schema": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
          "input": {
            "type": "string", "format": "path", "description": "...",
            "x-cli": { "positional": 1, "action": "set", "value_name": "INPUT_DIR", "io": "input-dir" }
          },
          "max_size": {
            "type": "integer", "minimum": 0, "default": 4096,
            "x-cli": { "long": "--max-size", "action": "set" }
          }
        },
        "required": ["input"],
        "additionalProperties": false
      }
    }
  ]
}
```

只列可调用的叶子子命令；隐藏命令（如已禁用的 `img_resize tinyfy`）不出现。`json` `help` `version` 不在 `input_schema` 里，执行器统一追加 `--json`。

### JSON 参数还原成 argv

`argv = [binary] + argv_prefix + 选项 + 位置参数 + ["--json"]`，逐个属性按 `x-cli`：

| `x-cli.action` | 还原方式 |
|----------------|---------|
| `flag` | 值为 `true` 时追加 `long`；`false` 不传 |
| `set` | 追加 `long` 和值（布尔型 `set` 如 `--recursive false` 也按值传） |
| `append` | 数组每个元素各追加一次 `long 值` |
| `count` | 重复 `long` N 次 |
| 有 `positional` | 不带 flag，按 `positional` 序号（从 1 开始）排列 |

`x-cli.conflicts_with` 列出互斥参数；`x-cli.env` 表示该参数也可以由环境变量提供。

### 命令级扩展字段

| 字段 | 取值 | 用途 |
|------|------|------|
| `x-effects` | `read-only` / `pure` / `local-write` / `external` | 风险等级与可缓存性。`pure` = 同输入同参数同输出、只写声明的输出路径；`local-write` = 原地改文件或依赖本机状态（已装字体等）；`external` = 访问网络/远端服务 |
| `x-env` | 环境变量名数组 | 命令会读的环境变量（凭证、开关），执行器负责注入 |
| `x-stdin` | 描述字符串 | 该命令可从 stdin 读主输入 |
| `x-note` | 字符串 | 前置条件、坑点 |

### 参数级 `x-cli.io`

| 值 | 含义 | 执行器处理 |
|----|------|-----------|
| `input-file` / `input-dir` / `input-path` | 读取的文件 / 目录 / 二者皆可 | 远端 URL 先下载进隔离工作目录再改写路径 |
| `output-file` / `output-dir` | 写出的文件 / 目录 | 改写到工作目录内 |
| `output-stem` | 不带扩展名的输出基名，工具会派生多个文件 | 同上，实际文件以 `artifacts` 为准 |
| `inout-file` | 原地读写 | 先复制进工作目录 |

## 5. 新工具接入清单

1. `Cargo.toml` 加 `rlit_cli` path 依赖，并把目录加进根 `Cargo.toml` 的 `workspace.exclude`。
2. `main.rs` 声明 `static CONTRACT: rlit_cli::Contract`，每个叶子子命令一条 `CommandSpec`（effects、io、env、stdin、note）。
3. `let cli: Cli = CONTRACT.parse();` 代替 `Cli::parse()`。
4. 结果用 `rlit_cli::set_data(..)`，写出的文件用 `rlit_cli::add_artifact(path, role)`，批处理部分失败用 `rlit_cli::set_failure(ErrorKind::Partial, ..)`；**不要再自己 `println!` JSON 或 `process::exit`**。
5. `main` 结尾 `rlit_cli::finish(result, |e| 分类)`，把错误枚举映射到上表的 kind。
6. 有 runlog 的，console 回显在 `rlit_cli::json_mode()` 时改走 stderr（参考 `mj_atlas/src/runlog.rs`）。
7. 加单测：`assert!(rlit_cli::check_contract(&CONTRACT, &Cli::command()).is_empty())`。它会检查每个叶子命令都有 `CommandSpec` 和 about、每个参数都有 help、`io` 引用的参数真实存在。
8. 在 `llms.txt` / `llms_cn.txt` 里写一节 Machine Call Contract（参考已接入工具）。
