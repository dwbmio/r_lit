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

## 主题系统：选择、自扩展、变更与新增指南

`archcast` 将整套视觉样式抽象为结构化 `ThemeConfig`（YAML / JSON）。**单份主题配置同时作用于全部导出产物**：HTML 页面样式、独立 SVG、光栅 PNG、动态流光 GIF（脉冲与呼吸灯配色）以及 Markdown 里的 Mermaid `%%{init:...}%%` 变量。

### 1. 随包内置的 5 套默认主题

| 主题 ID | 模式 | 视觉风格与推荐场景 |
| :--- | :--- | :--- |
| `starlink-light`（**默认**） | `light` | 清爽冷灰浅色汇报风，1:1 复刻标准评审文档样式 |
| `obsidian-neon` | `dark` | 深邃暗黑霓虹，适合大屏汇报与暗色模式评审 |
| `paper-editorial` | `light` | 暖色纸本编辑风，适合打印、PDF 归档与长文阅读 |
| `blueprint-ink` | `dark` | 高对比工程蓝图风，适合技术架构评审与系统拓扑展示 |
| `catppuccin-mocha` | `dark` | Catppuccin Mocha 暗色工程风，柔和低刺眼 |

### 2. 导出时如何选择主题（及解析优先级）

1. **命令行临时指定（优先级最高）**：支持传主题 ID 或直接传外部 `.yaml`/`.yml`/`.json` 文件路径：
   ```bash
   # 使用内置或已注册的外部主题 ID
   archcast render flow.yaml -o render/ --theme obsidian-neon --strict --json

   # 直接使用项目目录内的独立主题文件（无需提前注册）
   archcast render flow.yaml -o render/ --theme ./themes/my-brand.yaml --strict --json

   # 指定自定义外部主题注册目录（或通过环境变量 ARCHCAST_THEMES_DIR）
   archcast render flow.yaml -o render/ --theme corp-blue --themes-dir ./team-themes --json
   ```
2. **写入架构 YAML 规范固定主题**：在规范文件顶层加 `theme: blueprint-ink`（或相对/绝对路径），导出时未传 `--theme` 会自动生效。
3. **默认兜底**：两者都未指定时使用 `starlink-light`。

**主题查找顺序**：直接存在的文件路径 → 外部主题目录（`--themes-dir` > `ARCHCAST_THEMES_DIR` > `$XDG_CONFIG_HOME/archcast/themes` > `~/.config/archcast/themes`）→ 随包内置主题。

### 3. 新增自定义主题（3 种方式）

主题 ID 约束：长度 `1..=64`，以 ASCII 字母或数字开头，仅允许 `[a-zA-Z0-9_-]`。

- **方式 A：基于已有主题快速派生 + 字段覆盖**
  ```bash
  archcast theme create corp-blue \
    --from starlink-light \
    --display-name "企业蓝主题" \
    --set surface.page_bg=#f0f4ff \
    --set chrome.h2_accent=#1e40af \
    --set tones.emerald=#047857 \
    --set motion.pulse_hex=#1d4ed8,#047857,#d97706,#7c3aed
  ```
- **方式 B：导出内置主题完整 YAML，编辑后注册或直接使用**
  ```bash
  # 导出完整 7 组 Token 模板（或运行 `archcast theme show starlink-light` 查看）
  archcast theme export starlink-light ./corp-blue.yaml
  # 编辑完成后注册到外部主题目录（如已存在追加 --force 覆盖）
  archcast theme create corp-blue --file ./corp-blue.yaml
  ```
- **方式 C：编写极简增量扩展文件（利用 `extends` 自扩展）**
  只需写与父主题不同的字段，其余全部自动继承（支持多级 `extends`，最多 4 层）：
  ```yaml
  id: team-dark
  display_name: Team Dark
  extends: obsidian-neon
  surface:
    page_bg: "#0b1020"
    svg_bg: "#0f172a"
  chrome:
    h2_accent: "#38bdf8"
    edge_stroke: "#64748b"
  tones:
    emerald: "#10b981"
  motion:
    pulse_hex: ["#38bdf8", "#10b981", "#f59e0b", "#a78bfa"]
  ```
  可直接 `archcast render flow.yaml -o render/ --theme ./team-dark.yaml` 渲染，或 `archcast theme create team-dark --file ./team-dark.yaml` 纳入全局目录。

### 4. 查看、变更、覆盖默认主题与删除恢复

```bash
# 查看全部内置 + 外部主题（外部覆盖内置时会标出 overrides_builtin）
archcast theme list
archcast theme list --json

# 查看某主题的完整解析结果（默认输出 YAML，加 --json 输出结构化信封）
archcast theme show corp-blue
archcast theme show corp-blue --json

# 变更已有外部主题（支持 --set 多次叠加、--display-name 或 --file 批量覆盖）
archcast theme update corp-blue --set surface.card_bg=#ffffff --set chrome.edge_stroke=#334155

# 覆盖随包默认主题（写入外部覆盖文件后，连未传 --theme 的默认渲染也会自动采用新配色）
archcast theme update starlink-light --set chrome.h2_accent=#0f766e

# 删除外部主题（若删除的是内置主题覆盖文件，自动恢复随包默认值；纯内置主题受保护不可删）
archcast theme delete corp-blue
archcast theme delete starlink-light   # 仅移除外部覆盖并恢复随包 starlink-light
```

### 5. 结构化主题可配置字段速查（`--set <路径>=<值>` 与 YAML 键一致）

| 分组 | 可配置键名 | 说明 |
| :--- | :--- | :--- |
| 顶层元数据 | `display_name`, `color_scheme` (`light`\|`dark`), `is_dark` (`true`\|`false`), `extends`（YAML 内） | 主题名称、明暗标记与继承父主题 |
| `surface.*` | `page_bg`, `card_bg`, `card_border`, `card_shadow`, `svg_bg`, `node_bg`, `node_border`, `subcard_bg`, `subcard_border`, `callout_bg`, `callout_border`, `code_bg`, `codebox_bg`, `codebox_border` | 页面/卡片/SVG 画布/节点/子卡/创参卡/代码块背景与边框（`svg_bg` 需合法 `#RRGGBB`） |
| `ink.*` | `page_fg`, `lead_fg`, `note_fg`, `code_fg`, `svg_title`, `svg_subtitle`, `svg_meta`, `col_header`, `node_title`, `node_text`, `node_mono`, `node_footer`, `subcard_title` | 页面正文、说明、图标题、列头、节点正文/等宽字/页脚前景色 |
| `chrome.*` | `toc_link`, `h2_accent`, `btn_bg`, `btn_fg`, `btn_border`, `btn_hover_bg`, `pill_bg`, `pill_fg`, `edge_spine`, `edge_stroke`, `edge_label_bg`, `edge_label_fg`, `badge_opacity`, `ver_bg_alpha` | 导航链接、章节强调条、按钮、胶囊标签、总线/连线与标签底色 |
| `table.*` | `head_bg`, `head_fg`, `border`, `row_border`, `hover_bg`, `grp_bg`, `grp_fg`, `cnt_fg`, `small_fg`, `down_fg` | 接口明细表的表头、边框、悬停高亮、分组栏与下游列文字色 |
| `warning.*` | `hint_bg`, `hint_fg`, `hint_accent`, `banner_bg`, `banner_border`, `banner_accent`, `banner_title`, `banner_body` | 底部提示条与全宽风险警示横幅配色 |
| `tones.*` | `default`, `muted`, `blue`, `orange`, `green`, `emerald`, `amber`, `purple`, `gold`, `red` | 10 个语义色调映射（同时作用于卡片顶条、徽章、行文字与表格分组） |
| `motion.*` | `pulse_hex`（4 个逗号分隔 `#RRGGBB`）, `beacon_red`, `beacon_green` | HTML WebGPU/Canvas2D 与动态 GIF 的 4 通道脉冲光粒色与状态呼吸灯色 |

## 机器调用契约

- 传入 `--json` 时 stdout 只输出一个 `rlit_cli` JSON 信封；人类日志走 stderr 并落盘 `<output_dir>/archcast.log`。
- `archcast __schema` 输出机器可读的完整 CLI JSON Schema。
- 退出码：`0` 成功、`1` 运行期错误、`2` 参数/后端错误、`3` 输入/校验失败、`5` 批处理部分失败。
