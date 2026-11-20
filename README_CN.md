# R_LIT

面向中长期演进的 Rust 独立 CLI 工具与配套桌面工程集合仓库。每个子目录是独立的 Cargo crate（根 `Cargo.toml` 仅作为 `deskpet` 嵌入清单存在），所有 CLI binary 统一向共享底座 [`crates/rlit_cli`](crates/rlit_cli/)（`--json`、`__schema`、退出码、`runlog`）对齐。

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)

## 目录

| 目录 | 说明 |
|------|------|
| [artifact-inspect](artifact-inspect/README_CN.md) | 离线目录/ZIP/TAR.GZ 清单；按层 JSON 与 HTML 元数据/图像预览；通过本地 macOS arm64 和 Linux x86_64 试点验证 |
| [audiogen](audiogen/) | 纯 Rust 游戏音频管线（程序化音效、MP3/WAV/FLAC 编码、MiniMax 异步任务） |
| [bulk_upload](bulk_upload/) | 从 JSON 批量提取 URL 并上传至 S3 兼容存储 |
| [deskpet](deskpet/) | 无边框透明置顶 3D/Spine 桌面宠物（Bevy + 系统托盘 / 菜单栏常驻） |
| [gamereel](gamereel/) | 把游戏协议回放渲染成短视频（Cargo workspace；CUDA + FFmpeg） |
| [group_vibe_workbench](group_vibe_workbench/) | 桌面协作工作台（GPUI + Murmur P2P） |
| [img_resize](img_resize/) | 图片缩放与压缩 |
| [looplog](looplog/) | 本地日志摄入 / 查询工具，服务 AI 辅助联调（见 [微信 AI 调试指南](looplog/WECHAT_AI_DEBUG_CN.md)） |
| [mailctl](mailctl/) | 多邮箱 IMAP/SMTP 分发器：按邮箱身份收发、白名单、发前确认、审计 |
| [maquette](maquette/) | Kit 风格 low-poly 建模 + glTF 导出（GUI + headless CLI） |
| [mini_reg](mini_reg/) | 自家小程序半自动注册辅助；Rust 原生连接企业浏览器、SQLite 断点续办与人工确认 |
| [mj_atlas](mj_atlas/) | 精灵图集 / 打包工具，支持增量构建与多边形 mesh |
| [mpreview](mpreview/) | Godot 模型预览与多角度相机截图工具 |
| [phone_haul](phone_haul/) | 手机照片/视频迁移：Android（adb + 设备端 agent）与 iOS（AFC / Photos.sqlite），排重 + `yyyy/MM` 归档 |
| [textexture](textexture/) | 艺术字图片生成（阴影 / 描边 / 渐变 / 发光 / 霓虹） |
| [ui-trim](ui-trim/) | 清理 AI 生成 UI 素材的伪透明背景并裁成 tight PNG |
| [crates/murmur](crates/murmur/) | P2P 协作库（`iroh-net` + Automerge CRDT） |
| [crates/rlit_cli](crates/rlit_cli/) | 共享 CLI 机器调用契约（`--json` envelope、`__schema`、退出码、`runlog`） |

详细使用说明请进入各子目录查看对应 README。

## 安装预编译二进制

已登记且已发布的工具通过内网 Jenkins 交叉编译服务使用以下位置。`artifact-inspect` 可[从源码构建](artifact-inspect/README_CN.md#构建)；Linux x86_64 试点制品不等于已登记的正式发布或安装器。

已发布工具的制品位置：
- **Cloudflare R2（稳定 URL，主通道）** — `https://r2.gamesci-lite.com/r_lit/<tool>/`
- **HFrog 制品中心** — `https://hfrog.gamesci-lite.com/api/release/softwares/<tool>`

一行安装（自动识别 Linux / macOS / Windows-Git-Bash）：

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/<tool>/install.sh | bash
```

示例：

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/bulk_upload/install.sh | bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/img_resize/install.sh  | INSTALL_DIR=$HOME/.local/bin bash
```

GUI 应用（`maquette`、`group_vibe_workbench`、`deskpet`）按平台以原始二进制 tar 包发布（Linux/macOS），安装方式相同。

## 构建

```bash
cd <tool_dir> && cargo build --release
```

也可在仓库根目录通过 `just` 执行：

```bash
just build <tool> release
just check-repo
```

### Binary 目录约定

- `target/` 仅存放可删除的 Cargo 中间产物，不作为发布目录。
- 明确需要保留的本地 binary 统一放在 `<tool>/out/<target-triple>/<binary>`。
- 两个目录都不提交 Git；正式发布制品由 Jenkins 上传 R2/HFrog。

## 发布

唯一发布通道是内网 **Jenkins 交叉编译服务**（`ci-all-in-one/task/ci/pipeline/r_lit/Jenkinsfile.binary-build`），已无 GitHub Actions 发布路径。手动触发该任务、选 `TOOL_NAME` 与平台后，它会：

1. 按平台构建（Linux x86_64 原生 / macOS aarch64 原生 / Windows x86_64 经 `cargo-xwin` 交叉编译到 `x86_64-pc-windows-msvc`）。
2. 打包成 `<tool>-<target>.tar.gz`（Windows 为 `.zip`）。
3. 上传 R2（`s3://prod-hfrog/r_lit/<tool>/v<ver>/`）并刷新 `r_lit/<tool>/install.sh`。
4. 把 `software / version / release / platform` 全套记录写入 HFrog，带上真实的 `file_size`、`checksum_sha256`、`source_type`、`install_script_url`。

工具清单的事实源是 Jenkinsfile 的 `toolMap`。完整流程图见 [`docs/release.md`](docs/release.md)。

## License

See LICENSE file in each project.
