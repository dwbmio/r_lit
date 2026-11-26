# archcast

**面向 AI 的确定性架构与流程汇报编译器（SIMD + Rayon + WebGPU）。**

`archcast` 将 **给 AI / 研发维护的单一事实源**（精简 YAML/JSON）与 **给人评审的固定样式视觉产物**（内置 WebGPU/Canvas2D 流动动效的 HTML、根目录独立原版 `.svg`、可选光栅 `.png` 与动态流光 `.gif`）以及 **标准工程文档**（`Markdown + Mermaid`）彻底解耦；同时支持把既有的 `HTML + SVG` 总览页逆向提取回干净的 YAML 与标准 Markdown。

## 核心能力

- **1:1 固定样式与结构化主题系统（`render`、`theme`）**：将 YAML/JSON 确定性编译为四栏泳道汇报页（现状/改造后双图对比、外部系统动作子卡、自动宽度胶囊徽章、正交总线路由、底部创参/缺口卡、全宽风险警示条、分组接口/回调明细表）。随包内置 5 套主题（默认 `starlink-light`，另含 `obsidian-neon`、`paper-editorial`、`blueprint-ink`、`catppuccin-mocha`），并支持外部 YAML/JSON 结构化主题配置与完整 CRUD（`archcast theme list|show|create|update|delete|export`）、`extends` 继承覆写，HTML / SVG / PNG / GIF 与 Mermaid `%%{init:...}%%` 全链路色彩同步。
- **根目录直出独立 SVG + HTML + Markdown**：默认 `--format all` 直接在 `--output-dir` 根目录产出 `<stem>.html`、独立原版 `<stem>-<diagram_id>.svg`（单图时为 `<stem>.svg`）以及 `<stem>.md`。
- **以 HTML 为核心的分层 WebGPU 动效层**：产出的 HTML 内置零外部依赖的 WebGPU WGSL 粒子着色器叠加层（严格按 `dynamic_bounds` 最小动态范围定位，不支持 WebGPU 时自动回退 60fps Canvas2D），支持沿连线流动的数据脉冲动效、点击卡片高亮上下游链路与一键下载 SVG；传入 `--static-html` 可生成纯零 JS 静态 HTML。
- **光栅 PNG 与分层最小动态范围 GIF 导出（`--png` / `--gif` / `--format media`）**：
  - 基于纯 Rust `resvg` + `tiny-skia` 光栅化（自动加载系统 CJK 字体，无头 CI 零外部依赖）；
  - 默认 `--gif-mode overlay` 仅导出裁剪至最小动态包围盒的透明动效叠加层 GIF（绝不全屏重复编码），也支持 `--gif-mode delta` 静态底图 + 最小脏区子帧；
  - 默认模式走 CPU SIMD + Rayon 多帧并行合成；使用 `--features gpu` 构建后可走原生 `wgpu` WGSL Compute Shader（Metal / Vulkan / DX12）在 GPU 上合成动图帧。
- **逆向提取（`extract`）**：基于 `memchr` SIMD 扫描、`roxmltree` 零拷贝 XML 解析与 `wide::f64x4` 空间包围盒批量匹配，将既有 `HTML + SVG` 总览页逆向还原为结构化 YAML 和标准 Markdown。
- **AI Agent Skill**：内置 [SKILL.md](SKILL.md)，供任意 Coding Agent 读完源码直接产出规范 YAML 并自动编译交付。
- **完整接入 `rlit_cli` 机器调用契约 v1**：全局 `--json` 信封、统一退出码（`0-5`）、SHA-256 `artifacts` 回执、隐藏 `__schema` 导出与 `<output_dir>/archcast.log` 边车日志。

## 构建

```bash
cd archcast
cargo build --release
cargo test

# 启用原生 WebGPU WGSL Compute Shader 后端（加速动态 GIF 合成）
cargo build --release --features gpu
```

## 快速上手

```bash
# 1. 生成标准四栏 YAML 模板
archcast init starter.yaml --title "发布链路全景与改造落点"

# 2. 校验规范与拓扑连线
archcast validate starter.yaml --json

# 3. 编译 YAML → HTML（内置 WebGPU/Canvas2D 动效）+ 根目录独立 SVG + 标准 Markdown
archcast render starter.yaml -o out/ --strict --json

# 4. 管理外部结构化主题（默认存放在 ~/.config/archcast/themes 或 ARCHCAST_THEMES_DIR）
archcast theme list --json
archcast theme create corp-blue --from starlink-light \
  --set surface.page_bg=#f0f4ff --set chrome.h2_accent=#1e40af --json
archcast theme update corp-blue --set tones.emerald=#047857 --json
archcast render starter.yaml -o out/ --theme corp-blue --strict --json
archcast theme export corp-blue corp-blue.yaml --json
archcast theme delete corp-blue --json

# 5. 同步导出光栅 PNG 与分层流动动效 GIF
archcast render starter.yaml -o out/ --format media --strict --json

# 6. 将既有 HTML+SVG 总览页逆向提取为 YAML + Markdown
archcast extract legacy-overview.html -o extracted/ --json

# 7. 查看运行期 SIMD / Rayon / WebGPU 能力
archcast info --json
```

## 机器调用契约

- 传入 `--json` 时 stdout 只输出一个 `rlit_cli` JSON 信封；人类日志走 stderr 并落盘 `<output_dir>/archcast.log`。
- `archcast __schema` 输出机器可读的完整 CLI JSON Schema。
- 退出码：`0` 成功、`1` 运行期错误、`2` 参数/后端错误、`3` 输入/校验失败、`5` 批处理部分失败。
