# artifact-inspect

`artifact-inspect` 是用于游戏构建产物离线盘点的本地 Rust CLI。输入可以是目录、ZIP、TAR.GZ 或 TGZ，输出带版本的 JSON 报告、可独立打开的 HTML 报告和单份运行日志。工具会按文件名或文件签名发现任意路径下的嵌套 ZIP，包括 `out/engine/godot.zip`，没有写死 C4 专用路径。

工具只列出文件，不运行游戏代码、不执行归档脚本、不向输入目录抽取文件、不访问网络，也不声称能够追溯 Godot/Unity 引擎资产来源。

## 构建

这是独立 crate，必须在本目录构建，不要在仓库根目录构建全部工具：

```bash
cd artifact-inspect
cargo build --release --locked
```

需要支持 edition 2024 的 Rust 和 Cargo。[Cargo.toml](Cargo.toml) 声明版本 0.1.0，独立的 [Cargo.lock](Cargo.lock) 锁定依赖；根 workspace 已排除此 crate。只能在获准且规格足够的开发/CI worker 构建，禁止在 Jenkins controller、生产/服务机或不超过 2 核/4 GiB 内存的主机编译。

Cargo 输出为 `target/release/artifact-inspect`；下方示例需用该路径调用，或先将其所在目录加入 `PATH`。保留的本地二进制放在 `out/<target-triple>/artifact-inspect`，`out/` 不入库。目前仅 macOS arm64 已在本地验收（2026-11-20），Linux worker 制品尚未交付，也不声称 Windows 已验收。不声称已有正式 Jenkins/R2/HFrog 发布或安装器。

根 `.justfile` 的通用路由会将 `just build artifact-inspect release`、`just install_loc artifact-inspect release` 委派到 Cargo（不显式带 `--locked`）；固定的 `test-cli`/`lint-cli` 清单尚不包含本工具。通用路由不等于发布登记，不要假设存在 R2 安装 URL。Jenkins hook 留待后续评估，不是已实现的整合。

## 使用

```bash
artifact-inspect dist/ --output-dir inspection/
artifact-inspect game.zip -o inspection/ --json
artifact-inspect package.tar.gz -o inspection/ --max-depth 4
artifact-inspect __schema
```

输出目录不能是输入目录本身或其后代。既存 JSON/HTML/log 目标必须是普通文件：符号链接、硬链接（Unix）、非普通文件以及任何与输入文件相同的目标都会在检查前以退出码 2 拒绝。普通报告在重复运行时原子替换；绝不跟随目标链接，也绝不修改输入文件。

### 输出文件

| 文件 | 用途 |
|---|---|
| `artifact-inspect.json` | 完整报告，含 `schema_version: 1`、输入、实际限额、容器、逐文件记录、预览、问题和限制说明 |
| `artifact-inspect.html` | 离线逐行浏览页面，不含 JavaScript、外部资源或激活的归档 HTML |
| `artifact-inspect.log` | 本次运行的单份 `rlit_cli::runlog` 边车日志；输出位置校验通过后的失败也会落盘 |

使用 `--json` 时，标准输出遵循 [`rlit_cli` 信封](../docs/cli-contract.md)（`contract: 1`，运行时 `command: ""`）。`data` 包含容器汇总与报告路径，`artifacts` 包含 JSON、HTML、日志的大小和 SHA-256。完整清单按层放在 `containers[].files[]`，不是顶层扁平文件数组，也不在标准输出重复。`__schema` 导出 CLI 参数而非报告结构；唯一叶子为 `name: "artifact-inspect"`、`argv_prefix: []`，必填 `input`/`output_dir`，`x-effects: "pure"` 表示写声明的输出而不是完全不写文件。

HTML 提供分层元数据、有界转义文本和支持的图像预览，不是交互式引擎查看器。输出校验失败及参数解析错误没有边车日志，后续失败会尝试落盘 `artifact-inspect.log`；JSON 模式的人类日志进入 stderr。排障先读日志。三种输出逐文件替换，不是三文件事务；重跑失败不会删除旧 JSON/HTML，应使用新输出目录或核对本次退出码/信封与产物回执，不把陈旧报告视作本次成功。日志时间戳和绝对输入/输出路径不保证跨机器确定性。容器内文件按相对路径字典序而非大小降序排列；待处理 ZIP 子项在各父容器内排序后按广度优先排队，不是跨父容器全局排序。

### 文件与大小口径

每条文件记录包含：

- `container_id` 和 `container`：外层输入或逻辑嵌套容器路径（`parent.zip!nested.zip`）。
- `relative_path`：规范化后的归档成员路径或目录相对路径。每个容器内按路径字典序排列，嵌套容器按排序后的广度优先顺序处理。
- `kind`：识别出的位图、文本、归档、二进制、Godot、Unity 或其他类型。
- `raw_bytes`：未压缩文件字节数。
- `zip_compressed_bytes`：ZIP 中央目录记录的条目压缩数据量。目录文件和 TAR.GZ 成员为 `null`，因为 TAR.GZ 没有可靠的逐成员压缩字节口径。
- `preview`：预览类型、限制、截断状态，以及可选的有界文本或位图数据。

每个容器分别汇总 `file_count`、`raw_bytes`，ZIP 另有 `zip_compressed_bytes`。报告刻意不提供跨层字节合计：嵌套 ZIP 本身已经是父层文件，再把成员加进父层会重复计算存储量。ZIP 条目压缩数据不含 ZIP 文件头和中央目录开销，也不等于网络传输量。

Godot 包/资源和 Unity 场景/资产只做类型分类，不做专有格式解析。报告不推断引擎资产来源、导入依赖或真实运行时传输量。

## 预览安全与限额

- PNG、JPEG、GIF、WebP 在预览前检查尺寸、验证解码并限制像素数，最终以惰性 `data:` URI 嵌入。
- UTF-8 文本受字节限额约束并进行 HTML 转义。HTML、JavaScript、SVG 只显示转义后的文本，绝不激活。
- 文件名或路径疑似 private/secret/credential/token 配置，任意 `*.env`、`.env.*`、`.envrc`，以及私钥、密钥库等敏感文件，仍列出路径和字节，但 `preview.kind = "suppressed_sensitive"`，不含文本或图像内容。敏感嵌套 ZIP 的成员继承抑制，同时保留各自路径和大小口径。
- HTML 的内容安全策略为 `default-src 'none'`，仅允许 `data:` 图像和报告内联样式。
- 不跟随输入符号链接或归档链接。特殊文件会跳过并记录问题，危险归档路径会被拒绝。
- 嵌套 ZIP 只在有界内存中处理，不抽取到输入旁。每个普通 ZIP 成员都用固定大小缓冲区读取到末尾，在预览前缀之外继续验证解压、声明大小和 CRC。损坏的嵌套 ZIP 记录为 `nested_zip_invalid`；损坏的外层 ZIP 以输入错误退出，不生成成功报告。
- 读取 TAR 成员后，继续用固定大小缓冲区排空 gzip 解码器直至 gzip 尾部，避免 TAR 提前结束标记掩盖截断或校验和错误。损坏的外层 TAR.GZ 以退出码 3 和 `error.kind = "input"` 失败，不写 JSON/HTML 成功报告，并保留错误日志。

默认限额：

| 参数 | 默认值 | 含义 |
|---|---:|---|
| `--max-entries` | 100000 | 各层处理的普通文件总数 |
| `--max-total-raw-bytes` | 2000000000 | 跨层处理工作量上限，不是报告汇总口径 |
| `--max-nested-bytes` | 67108864 | 展开前单个嵌套 ZIP 文件自身的最大字节数 |
| `--max-depth` | 4 | 嵌套 ZIP 深度；0 表示不展开 |
| `--max-preview-bytes` | 65536 | 单个预览最多嵌入的源字节 |
| `--max-total-preview-bytes` | 8388608 | 所有预览最多嵌入的源字节 |
| `--max-image-pixels` | 16777216 | 位图预览像素上限 |

超过限额或跳过内容会明确表现为错误或容器级问题，不生成替代数据。`max_entries` 还限制目录遍历条目（含根与目录）及外层/每个嵌套 ZIP 中央目录条目数（含目录）。普通文件计数和原始字节工作量跨层共享。`max_nested_bytes` 限制待展开的嵌套 ZIP 文件自身字节，不是成员字节总和。条目、总原始字节、总预览字节、像素限额必须为正；深度、嵌套文件字节、单预览字节限额可为零。schema 中整数最小值为零，仍须遵守上述运行期额外检查。

只递归展开 ZIP；嵌套 TAR/GZIP/Brotli 及引擎专有容器不展开。ZIP 支持受已提交 `zip` 依赖特性限制（仅启用 `deflate`，禁用默认特性）；不承诺加密或其他压缩格式支持。

敏感抑制仅为路径/名称启发式，不是内容保密扫描或保密保证。报告仍包含路径、大小及未被抑制的文本/图像，外发前须人工审阅，工具不能证明报告可安全公开。使用稳定的本地输入和受控输出目录，不把恶意并发修改的文件系统视为安全沙箱。

## 退出码

- `0`：成功；非致命跳过项（如有）写入报告和信封警告。
- `1`：其余运行期、序列化或 I/O 错误（并非所有输出写入错误都归此类）。
- `2`：参数/限额/输出位置非法、输入为符号链接或输出目标被拒绝。
- `3`：外层输入缺失、不可读、不支持、损坏、路径危险或超限。I/O 的 `NotFound`、`PermissionDenied`、`InvalidData`、`InvalidInput`、`UnexpectedEof` 也归此类，即使发生在输出/日志写入阶段。
- `4` / `5`：共享契约保留值；本离线单输入工具通常不产生这两类退出码。

## 测试

```bash
cd artifact-inspect
cargo test
cargo clippy --all-targets -- -D warnings
```

隔离测试覆盖目录、ZIP、TAR.GZ、按签名发现嵌套 ZIP、分层大小口径、gzip 尾部截断、预览范围之外的 ZIP CRC 损坏、显式嵌套 ZIP 失败、符号链接与硬链接输出目标、普通输出替换、输出位置、限额、离线转义预览、环境文件与敏感容器抑制、CLI 信封和 `__schema`。

对冻结的本地 Zuma C4 做回归时，将两个占位路径换成操作者提供的现存输入，并在 `artifact-inspect/` 执行。不附带 C4 样本或私密清单。以下是与 Lead 独立清单比较的可选开发测试，不是普通 CLI 使用步骤：

```bash
C4_INPUT="/path/to/frozen-c4.tar.gz" \
C4_REFERENCE="/path/to/independent-c4-inventory.json" \
cargo test --test c4_reference -- --ignored --nocapture
```

以下基线来自 2026-11-20 本地 macOS arm64 历史回归，不是本轮跨平台实测。该测试逐项比较 `(layer, path)`、原始字节和 ZIP 压缩字节，并核对独立基线：外层 25 文件 / 7,696,698 B，内层 139 文件 / 2,093,130 B 原始 / 1,979,382 B ZIP 条目数据。它还验证 `out/project.private.config.json` 与 `env/runtime.env` 都为 `suppressed_sensitive`，没有文本、媒体类型或图像内容；只打印这些元数据状态，绝不打印值。
