---
name: archcast
version: "0.1.0"
description: "面向 AI Agent 的确定性架构与流程汇报编译器（Rust + SIMD + Rayon + WebGPU）。将结构化 YAML 规范一键编译为固定样式 HTML 汇报页（内置浏览器 WebGPU/Canvas2D 流动动效与链路高亮）、根目录独立原版 SVG、可选光栅 PNG 与动态流光 GIF，以及标准 Markdown + Mermaid 工程文档；并支持将既有 HTML+SVG 总览页逆向提取为 YAML 与 Markdown。触发: archcast、画架构总览、出链路全景图、改造落点图、HTML流程总览、SVG架构图、现状改造对比图、提取HTML架构图、导出架构动图。"
---

# archcast Skill（v0.1.0 · 面向 AI Agent 与自动化流水线）

`archcast` 将 **AI 维护的单一事实源（YAML）** 与 **人类评审用的固定样式视觉产物（HTML + 独立 SVG + PNG + 动态 GIF）** 以及 **标准工程文档（Markdown + Mermaid）** 彻底解耦：

- **AI 只负责写语义清晰的 YAML**（省 80% Token、零坐标计算、100% 可做 Git Diff）；
- **`archcast` 负责确定性布局与多格式输出**（毫秒级出图、样式全团队 1:1 统一、绝不跑版）。

---

## 一、关键字触发与任务路由

| 场景 | 触发意图 | 推荐命令 |
| :--- | :--- | :--- |
| 从源码/需求出标准架构汇报 | 梳理链路全景、改造落点对比、跨团队评审 HTML | 先读代码生成 `<name>.yaml` → `archcast render <name>.yaml -o out/ --strict --json` |
| 同时导出飞书/PPT 可直接粘贴的静态图与动图 | 导出 PNG、导动图、流光 GIF | `archcast render <name>.yaml -o out/ --format media --strict --json`（启用 WebGPU 时加 `--backend gpu`） |
| 逆向分析既有 HTML+SVG 总览页 | 把别人给的 HTML+SVG 变成标准文档或可编辑 YAML | `archcast extract <legacy.html> -o out/ --json` |
| 校验 YAML 拓扑连线是否完整 | 检查有没有断链、重名卡片 | `archcast validate <name>.yaml --json` |
| 生成空白四栏模板 | 新建架构总览起步 | `archcast init <name>.yaml --title "..." --json` |
| 管理视觉主题（增删改查/导出） | 切换全局配色、创建团队定制主题、覆盖默认主题 | `archcast theme list\|show\|create\|update\|delete\|export --json`；渲染时传入 `--theme <id-or-path>` |
| 检查 SIMD / Rayon / WebGPU 能力 | 排查 GPU 后端是否就绪 | `archcast info --json` |

---

## 二、标准 Agent 工作流（读代码 → YAML → 确定性产物）

当用户要求把某个仓库、需求或接口改造整理成标准架构总览时，严格按以下 4 步执行：

1. **收集事实**：阅读相关 proto / router / client / 表结构 / 回调定义（或配合 `distill` 远端读仓），提炼出：
   - ① 触发源（人 / 定时任务 / N8N / 外部回调）
   - ② 核心编排与存储（入口 API、主表、子流程表、宽表）
   - ③ 外部系统（**按调用动作拆成 `subcards`**，同一平台多个接口 = 多张子卡）
   - ④ 读侧消费（后台列表、总览看板、GQL）
   - 现状缺口 vs 改造落点、核心创参结构、高风险警示条、全量入口→下游接口明细表
2. **编写单一事实源 `<name>.yaml`**：严格遵循第三节 Schema；**严禁手写 SVG 坐标**。
3. **严格校验与编译**：
   ```bash
   archcast validate <name>.yaml --json
   archcast render <name>.yaml -o <out_dir> --strict --json
   ```
   默认 `--format all` 会在 `<out_dir>` 根目录一次性产出：
   - `<name>.html`（固定样式汇报页，内置浏览器 WebGPU/Canvas2D 流动动效 + 点击卡片追踪上下游 + 一键下载 SVG）
   - `<name>-fig1.svg` / `<name>-fig2.svg`（独立原版 XML SVG，可直接拖入 Figma / Keynote / 文档）
   - `<name>.md`（标准 Markdown + Mermaid 工程文档，适合进 Wiki / Notion / 下游 AI 上下文）
   - `archcast.log`（运行期边车日志）
   
   如需同步产出 PNG 光栅图与动态流光 GIF，追加 `--png --gif`（或 `--format media`）：
   - `<name>-fig1.png` / `<name>-fig2.png`
   - `<name>-fig1.gif` / `<name>-fig2.gif`
4. **验收回执**：从 `--json` 信封里的 `artifacts[]` 读取所有产出文件的绝对路径与 SHA-256，向用户交付。

---

## 三、YAML Schema 速查与视觉配色约定

### 3.1 语义色调（`Tone`）规范

| `tone` / `accent` | 十六进制 | 推荐语义 |
| :--- | :--- | :--- |
| `blue` | `#4b83d4` | 人工触发源、主流程分组、v1 标签 |
| `orange` | `#e08b3c` | N8N / 自动化编排触发、班车流分组 |
| `green` | `#3e9e6e` | 核心编排/存储列、测试系统、通用回调 |
| `emerald` | `#0f8a5f` | **改造后新增 / 已补齐 / 透传绿标** |
| `amber` | `#b45309` | 项管系统、`caller` 透传链路、风险提示 |
| `purple` | `#7b61c9` | 方案平台 / 核心业务下游、v2 标签、创参卡 |
| `gold` | `#c9a227` | 读侧系统、代码审核 |
| `red` | `#d9534f` | **现状缺口 / 缺版本 / 缺失接口红标** |
| `muted` | `#8b95a5` | 不涉及版本、只读标签、表名与注释 |

### 3.2 最小完整 YAML 结构

```yaml
schema_version: 1
doc_title: 系统链路全景
title: 系统链路全景 / 改造落点
lead:
  - 一句话：**触发源 → 核心编排 → 外部系统 → 读侧消费。**
  - 本次仅新增 `api_version` 字段透传；已有 `caller` 保持不动。

diagrams:
  - id: fig1
    heading: 一、链路全景（现状）
    note: 红标 = 缺失版本标识；橙色 = 已有透传参数。
    canvas_title: 系统 · 链路全景（现状）
    subtitles:
      - text: 每个外部平台按「被调用动作」拆子卡。
        tone: muted
    columns:
      - id: triggers
        title: ① 触发源
        cards:
          - id: ui_create
            title: 人 · 发起创建
            accent: blue
            lines:
              - text: POST /api/v1/create
                mono: true
              - text: api_version：缺失
                mono: true
                bold: true
                tone: red
      - id: core
        title: ② 核心编排 / 存储
        cards:
          - id: core_api
            title: 入口 API
            accent: green
            lines:
              - text: POST /flow/create
                mono: true
      - id: external
        title: ③ 外部系统
        cards:
          - id: ext_svc
            title: 下游平台
            accent: purple
            badge: { text: 缺版本, tone: red }
            subcards:
              - title: 创建任务
                badge: { text: ✗ 只有 v1, tone: red }
                lines:
                  - text: v1 → POST /ext/v1/task
                    mono: true
            footer: 主流程调用
      - id: readers
        title: ④ 读侧
        cards:
          - id: hub
            title: 结果总览
            accent: gold
            align_with: ext_svc
            lines:
              - text: GET /ext/v1/list
                mono: true
    edges:
      - kind: bus          # bus | direct | loop_left | bottom_route
        from: [ui_create]
        to: [core_api]
        label: 触发
      - kind: bus
        from: [core_api]
        to: [ext_svc]
        label: 主流程调用
        bus_slot: 0.3
      - kind: direct
        from: [ext_svc]
        to: [hub]
    callouts:
      - title: 下游创参结构
        accent: purple
        style: code_box    # code_box | bullets
        lines:
          - text: origin_param { workflow_id, caller }
            tone: amber
          - text: api_version ——（待新增）
            tone: red
      - title: 当前缺口
        accent: red
        style: bullets
        lines:
          - text: ① 入口与下游均未区分 v1/v2
    warning:
      title: ⚠ 别忘了下游底板与异步回调
      body: 存量默认按 v1 兜底，所有回调必须校对版本归属。
    footer: 依据：service/api/*.proto · service/internal/data/*.go

flow_table:
  id: flowtable
  heading: 三、入口 → 核心编排 → 下游服务
  note: 每行 = 一个入口 + 核心接口 + 下游调用。
  groups:
    - name: 主流程
      tone: blue
      rows:
        - action: 创建任务
          trigger: 前端
          version: v1
          version_tone: blue
          entry: [POST /flow/create]
          downstream: [POST /ext/v1/task]
          note: 支持 **行内加粗** 与 `code`
  hint: 补齐 v2 时同步核对 `callback` 路由。
```

---

## 四、结构化主题配置与外部 CRUD

1. **随包默认主题（5 套）**：
   - `starlink-light`（**默认主题**，清爽冷灰浅色汇报风）
   - `obsidian-neon`（深邃暗黑霓虹，适合大屏/深色评审）
   - `paper-editorial`（暖纸本编辑风，适合打印与长文阅读）
   - `blueprint-ink`（高对比工程蓝图风）
   - `catppuccin-mocha`（Catppuccin Mocha 暗色工程风）
2. **解析优先级（`--theme <id-or-path>` 或 YAML 顶层 `theme:` 字段）**：
   - 直接存在的 `.yaml`/`.yml`/`.json` 文件路径 → 外部主题目录（`--themes-dir` / `ARCHCAST_THEMES_DIR`，默认 `~/.config/archcast/themes/`）→ 随包内置主题；
   - 支持 `extends: <parent-id>` 继承（仅需写差异字段即可局部覆写，也支持用同名外部文件覆盖随包主题，删除后自动恢复内置默认）。
3. **外部主题 CRUD 命令**：
   ```bash
   archcast theme list --json
   archcast theme show starlink-light --yaml
   archcast theme create corp-blue --from starlink-light \
     --set surface.page_bg=#f0f4ff --set chrome.h2_accent=#1e40af --json
   archcast theme update corp-blue --set tones.emerald=#047857 --json
   archcast theme export corp-blue ./corp-blue.yaml --json
   archcast theme delete corp-blue --json
   ```
   单份主题配置同时驱动 **HTML CSS、SVG 图形、光栅 PNG、动态 GIF 脉冲色与 Mermaid `%%{init:...}%%` 变量**。

---

## 五、WebGPU 与分层最小动态范围动效说明

1. **HTML 汇报页内置动效（默认开启，绝不全屏铺画布）**：
   - WebGPU / Canvas2D 叠加层 `<canvas class="fx-canvas">` 严格按 `dynamic_bounds`（仅包裹连线与状态呼吸灯的最小动态包围盒）百分比定位，不覆盖顶部标题、底部创参卡、警示条与静态留白；
   - 浏览器支持 WebGPU 时自动启用 WGSL Instanced 渲染管线 60fps 播放；不支持时自动平滑回退 Canvas2D；
   - 点击任意卡片可高亮上下游关联链路；传 `--static-html` 可生成零 JS 纯静态 HTML。
2. **CLI 侧 `wgpu` 光栅与分层动图导出（绝不全屏重复编码）**：
   - `--gif-mode overlay`（**默认**）：**不烘烤全屏底图**，只导出裁剪到最小动态包围盒的纯透明动效叠加层 GIF（几十 KB），并在 `--json` 回执中给出精确 `left_pct/top_pct/width_pct/height_pct`，专供叠加在静态 HTML / SVG / PNG 之上；
   - `--gif-mode delta`：仅第 0 帧输出静态底图，第 1..N 帧只编码相对上一帧真正发生像素变化的**最小脏区子矩形 + 透明未变像素**（`DisposalMethod::Keep`），静态文字零重绘、体积大幅压缩；
   - 默认构建通过 Rayon 多帧并行在无头 CI 稳定运行；`cargo build --release --features gpu` 下自动走原生 `wgpu` WGSL Compute Shader（仅在最小动态区内派发计算线程组）。

---

## 六、机器调用契约（`rlit_cli` v1）

- 全局 `--json`：stdout 仅输出单个 JSON 信封（含 `ok`、`data`、`artifacts[]`、`error`）；
- `archcast __schema`：输出机器可读的 CLI JSON Schema；
- 退出码：`0` 成功、`1` 运行期错误、`2` 参数/后端不可用、`3` 输入或校验失败、`5` 批量部分失败；
- 排障优先查看 `<output_dir>/archcast.log`。
