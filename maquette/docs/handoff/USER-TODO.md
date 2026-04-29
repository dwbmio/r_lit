# Maquette · 用户验证 TODO（到 v1.0）

**给你自己用的勾选清单**。每项标注了所属版本、预计耗时，以及如何复现 / 判定通过。
做完一项就把 `- [ ]` 改成 `- [x]`，遇到不过的直接在条目下追加 `> 失败现象：...`，我下次看到会处理。

Agent 侧的进度与决策见 `NEXT.md` 与 `vX.Y-complete.md`。
Agent 每版交付时跑的 143 项自动化测试 + clippy + headless CLI 构建都会绿；本清单只列**必须你亲眼看 / 真机跑 / 真引擎开**的事。

---

## 📋 待你验收 dashboard（截至 2026-04-27 晚）

下面是「**坐下就能挨个验**」的清单，按耗时升序排，每条对应下面 § 章节里的详细步骤（**Cmd+F 搜该行的编号**就能直接跳到详细步骤）。

- 完成 → 把 dashboard 这行的 `- [ ]` 改成 `- [x]`；遇到不过 → 在下面 § 详细步骤里追加 `> 失败：...`，我下次 session 会读。
- 整个清单 ① ≈ 70 min 能清完。② 是大块头需要单独腾时间。③ 是 agent 还没交付的项，先别碰。

### ① 现在就可以挨个验（GUI / 终端）

| 状态 | 时长 | 编号 | 一句话主题 |
|---|--:|---|---|
| - [ ] | 1m | `#19b` | 预览缩放 −/+ 按钮 + `+`/`-` 快捷键 |
| - [ ] | 1m | `#20`  | Empty-state 提示出现/消失 |
| - [ ] | 1m | `#11`  | 拖动笔画 1 次 Undo 全清 |
| - [ ] | 1m | `#3`   | `.gltf` 导出同时产出 `.gltf` + `.bin` |
| - [ ] | 1m | `#6`   | `maquette-cli info --json` 是合法 JSON |
| - [ ] | 2m | `#TEX-C-cli` | **(新)** schema v4 向后兼容 — CLI 打开旧 v3 .maq |
| - [ ] | 3m | `#BLOCK-cli` | **(新)** `maquette-cli block list/get/sync` — local 12 块 + hfrog 联调通路 |
| - [ ] | 3m | `#BLOCK-gui` | **(新)** GUI 右侧 Block Library 面板 + slot 绑定 + 蓝徽章 + Sync 按钮 |
| - [ ] | 3m | `#COMPOSER-mock` | **(新)** Window → New Block Composer 第二窗口 + Cube/Sphere + mock generate + Save Local Draft → 主 Library 看到 |
| - [ ] | 3m | `#COMPOSER-publish` | **(新)** 同上换 rustyme + Publish to Hfrog →`Sync hfrog` 在主 Library 看到 |
| - [ ] | 4m | `#D1-material` | **(新)** Material 抽屉 model_description 文本框 + Cmd+S 重开保留 + Cmd+Z 撤销 |
| - [ ] | 4m | `#D1-slotgen` | **(新)** Palette 右键 → Generate texture → 三 lane（Mock / CPU / Fal）→ `~/.cache/maquette/textures/<sha>.png` 落盘 |
| - [ ] | 3m | `#shortcuts` | **(新)** 全套键盘快捷键（含 Shift+A 显隐 axes / `[` `]` 改 brush / Cmd+B 开 Composer / G / Shift+G 生纹理）|
| - [ ] | 2m | `#10`  | "+" 加色按钮 / `1-9` 快捷键选活色 |
| - [ ] | 2m | `#17`  | Multi-view PIPs + `F2` 切换 |
| - [ ] | 2m | `#17b` | PIP 点击 → 主预览动画对齐角度 |
| - [ ] | 2m | `#17c` | PIP 边框颜色 / 坐标系小图 / World axes 半透明 |
| - [ ] | 2m | `#18`  | Float 关闭联动 + 浮窗姿态记忆 |
| - [ ] | 2m | `#19`  | `F` Fit / `Cmd+R` Reset |
| - [ ] | 3m | `#1`   | Brush Height 滑块（1 / 3 / 8） |
| - [ ] | 3m | `#1b`  | Paint Mode：Overwrite / Additive 切换（`A` 键） |
| - [ ] | 3m | `#8`   | 色板编辑持久化（右键改色 + Save 重开） |
| - [ ] | 3m | `#12`  | Greedy meshing 体积收益（满铺 16×16 → ≤ 12 三角形） |
| - [ ] | 3m | `#13`  | `maquette-cli render` 与 GUI 等距预览视觉一致 |
| - [ ] | 3m | `#20b` | 事件驱动渲染（idle ≈ 0% CPU） |
| - [ ] | 3m | `#1c-async` | 异步导出 + 进度弹窗 + 失败 toast（macOS 26 卡死回归） |
| - [ ] | 4m | `#1c`  | 右键 cycle 形状 / Backspace 抹除 / 高度数字 / Sphere 圈环 / 居中 modal |
| - [ ] | 5m | `#5`   | CLI `export` 后丢进任一引擎打开 |
| - [ ] | 5m | `#9`   | 删除色 modal（Erase / Remap） |
| - [ ] | 5m | `#16`  | `maquette-cli palette export/import` 往返 |
| - [ ] | 5m | `#21`  | Autosave 恢复（`kill -9` 实测） |
| - [ ] | 5m | `#TEX-A` | CLI 离线 mock provider 决定性 + 磁盘缓存 |

**①  小计 ≈ 93 min**（`#BLOCK-cli` / `#BLOCK-gui` / `#COMPOSER-mock` / `#COMPOSER-publish` / `#shortcuts` 各 3 min；`#D1-material` / `#D1-slotgen` 各 4 min）。建议顺序就按上表从短到长跑，连续打钩很有成就感。

### ② 大块头（单独安排时间）

| 状态 | 时长 | 编号 | 主题 |
|---|--:|---|---|
| - [ ] | 30m | `#2`  | 三引擎（Godot 4 / Unity 6 / Blender 4）导出对比 |
| - [ ] | 60m | `#15` | 跨引擎截图归档（4 张图存进 `docs/export/screenshots/`） |
| - [ ] | 30-60m | `#27` | Smoke matrix（macOS 必做 / Linux 强烈建议 / Windows 可选） |

### ③ 等开发交付后再跑

| 编号 | 主题 | 阻塞 |
|---|---|---|
| `#22` | Prefs 持久化（Multi/Float/Brush 重开保留） | 等 **v0.9 C** prefs 文件 |
| `#23` | Release build 体积 < 25 MB | 等 **v0.9 B** Bevy feature trim |
| `#24` | Perf 目测（32×32 + 高度 8 + Multi 全开 60 fps） | 等 release build |
| `#TEX-D1` | GUI "一句话出全套贴图" | 等 **v0.10 D-1** |
| `#TEX-D2` | 单 slot 再生 + 手写 override | 等 **v0.10 D-2** |
| `#25` | README / user-guide 审阅 | 等 v1.0 docs |
| `#26` | App icon 拍板（4 版提案） | 等 agent 出图 |
| `#28` | 打 tag `v1.0.0` 发布 | 等所有上面项绿 |

### ✅ 已结（无需手验）

- [x] `S-1` / `S-2` / `S-3` — 装机 + 单测 + 冷启动 **(done 2026-04-23)**
- [x] `#4` — `cargo install` 把 CLI 推上 `$PATH`（包含在 S-1）
- [x] `#14` — Headless 构建无 GUI 依赖（CI 每版必跑，绿）
- [x] `#TEX-B` — Rustyme 端到端联调 **(done 2026-04-27 晚 / `v0.10b-bis-complete.md` § 4)**  
      cpu solid + cpu smart LLM + fal routing + revoke + 缓存全绿，**且顺手修了一个 RPUSH 死循环 bug**。
- [x] `#TEX-B-fal` — **2026-04-28 下午联调通过**：sonargrid 那边配 `FAL_KEY` 后 + Maquette 端把 `MAQUETTE_RUSTYME_MODEL=fal-ai/flux/schnell` 显式覆盖（默认值会被 lua hook 当 endpoint 拼出 `https://fal.run/rustyme:texture.gen` → 404）。一行批量出 12 块 Minecraft 风格纹理：`scripts/gen-mc-blocks.sh --provider fal --width 256`。84 秒、$0.036、PNG 256×256 全到 `/tmp/mc-blocks/`。`docs/texture/rustyme.md` 已注明这个坑。
- [x] `#TEX-C`（lib 部分）— Schema v4 25 条单测全过 **(done 2026-04-27 晚 / `v0.10c1-complete.md`)**  
      GUI 部分（"open v3 → 改一笔 → save → 看到 version=4 + palette_meta"）等 D-1 加 GUI Generate 按钮时一起跑。

**接下来 agent 要做的下一件事是 `v0.10 D-1` GUI material panel + Canvas group fan-out** —— 见 `NEXT.md` § "Outstanding work / Now"。

---

## 快速启动（装机后做一次）

- **S-1**（5 min）`cargo install --path maquette`；确认 `maquette` 和 `maquette-cli` 同时在 `$PATH` 上。
- **S-2**（2 min）`cargo test`（在你机器上再跑一次，≥ 76 passed）。
- **S-3**（1 min）`maquette` 冷启动 → 看到默认 16×16 画布 + 右上角浮动工具栏（Fit / Reset / Multi / Float）+ 画布中央的空态提示。
  - 已知历史 bug（2026-04-23 已修）：
    1. bevy_egui 0.39 会把 primary context 挂到第一个 spawn 的 Camera 上；若 `spawn_ortho_cameras` 抢先于 `spawn_camera`，整 UI 会被挤进右下角 ~180×180 方块。修复：`main.rs` 里关掉 `auto_create_primary_context` 并在 `MainPreviewCamera` 上显式插 `PrimaryEguiContext`。如再遇到「UI 缩在右下角」现象，先检查 `camera.rs` 是否仍带 `PrimaryEguiContext` 标记。
    2. Bevy 0.18 把 Material bind group 从 `@group(2)` 挪到 `@group(3)`；旧硬编码会在开启 GPU preprocessing 的设备（Apple Silicon 等）点第一笔就 wgpu validation panic。修复：`assets/shaders/toon.wgsl` 改用 `@group(#{MATERIAL_BIND_GROUP})` 预处理宏。
    3. Top PIP 跟 2D 画布方向对不上（上下+左右全反）：原来 `up = +Z`，右手系叉乘把 +X 甩到左侧、+Z 甩到上侧。改成 `up = -Z` 后 Top PIP 跟 2D 画布逐像素对齐。
    4. 主 3D 预览视觉偏左：相机渲染全窗口 → `SidePanel::left` 盖住左半 → 焦点看起来偏了。修复：每帧把 egui 的 `available_rect()` 同步给主相机 `viewport`，让预览在可视区居中。
    5. 修 4 时撞出反馈环（整屏抖动）：若 `PrimaryEguiContext` 还留在主相机上，bevy_egui 会把被缩小的 viewport 当成新的 egui screen_rect，panels 再从中扣一层，viewport 进一步缩……无限震荡。修复：新建一个 `Camera2d` 专门当 egui host（永远覆盖全窗口），把 `PrimaryEguiContext` 挪到它上面；主 3D 相机只负责 3D viewport，和 egui 彻底解耦。
    6. 修 5 时把 egui host 设成 `is_active: false` 导致整窗全黑 —— bevy_egui 的 render graph node 只对 active 相机执行，inactive = 不画 egui。修复：host 相机保留 active，但用 `ClearColorConfig::None`（不清屏）+ `order: 1000`（最后渲染），让它只叠 egui 不盖 3D。
    7. **主预览是透视不是正交** —— 这是有意为之：三个 PIP（Top/Front/Side）负责正交精确判断，主预览负责"看起来像什么"，和 Blender / Unity / Asset Forge 的默认一致。如果你还是想要正交，可以在 v1 之后讨论加个"Orthographic Main View"选项。
    8. View → **Show World Axes**（默认开）：在世界原点叠加红/绿/蓝 (X/Y/Z) 轴线并每格打点，主预览和三个 PIP 里都会出现，用来眼判对齐。关掉之后如果你还觉得对不齐，截个图 + 说一下左右 PIP 哪格应该对应 2D 画布哪格，我再精调。
- S-1【done】
- S-2【done】
- S-3【done】

---

## v0.4 · 基础流水线（GUI 肉眼检查）

- **#1**（3 min）Brush Height 测试
  - 打开 GUI → 左侧 Brush 滑块依次设 1 / 3 / 8 → 分别画一格 → 右侧预览应看到对应高度的柱体。
  - 判定：高度跟滑块数值一致，上表面用 toon shader 正确着色。
- **#1b**（3 min）Paint Mode: Overwrite vs Additive（2026-04-23 加）
  - **Brush 面板现在浮在画布左上角**（Blender 风格浮动 HUD，不再占侧栏底部）；里面应该有 `Mode: [Overwrite] [Additive]` 切换；状态栏"Left-click: paint (overwrite/stack +)"文字会跟随变。
  - **Overwrite 模式**：Brush Height=3 画一格 → 柱高 3。再点同一格 Brush=5 → 柱高变 5（替换）。
  - 切 Additive（面板切换或按 `A`）。Brush Height=2 → 点那格 → 高度变 5+2=7。再按 A 切 → 状态栏跟着切。
  - **拖拽保护**：Additive 模式下，Brush=1，从空白区域拖过一条 5 格长的线 → 那 5 格各应该是高度 1（不是 60+），因为每个 cell 在一次 stroke 内只叠一次。
  - 用不同颜色在已有柱子上 Additive 刷 → 柱子颜色**保留原色**，只长高（想换色就切回 Overwrite）。
  - Edit → Paint Mode 下拉也能切，且显示的单选跟面板同步。
- **#1c**（4 min）右键切形状 + Backspace/Delete 抹除 + 高度数字（2026-04-23 加）
  - 画任意一格（任何颜色、任何高度、Cube 形状，默认就是 Cube）。
  - **右键**那一格 → 预览里该格从立方体变成一颗**单位球**；如果高度=3，预览里是**三颗同样大小的球竖向堆叠**（不是拉长的椭球/药丸），跟 Cube 柱"每层一个方块"的语义完全一致。**同时 2D 画布上那格中央会叠一圈黑底白芯的小圆环**——那就是 Sphere 形状的标记，Cube 状态下没有这个环。再右键 → 预览和 2D 圈环同时消失，回到 Cube。循环一次对应 `Cube → Sphere → Cube`。
  - 在空格子上右键 → 应该**什么也不发生**（现在右键只作用于已有方块，避免"空点还创建个球出来"）。
  - 右键按住从左拖到右 5 格 → 每格只被 cycle 一次（不是每帧 cycle 一次震荡）。松开后 Ctrl+Z 一次回到全 Cube，2D 圈环也一起消失。
  - **Backspace** 或 **Delete** 抹除：鼠标悬停任意已画格子 → 按 `Backspace` 或 `Delete` → 该格变空。每按一次就一个 undo 步骤。
  - 鼠标悬停空格时按 Del/Back → 应该**无副作用**（不崩，不触发菜单）。
  - **高度数字徽章**：Brush Height=3 画一格（或 Additive 叠到 3）→ 2D 画布该格的**左上角**会显示 `3`（黑底白字小药丸），Cube/Sphere 都会显示。高度降到 1 或擦掉之后数字消失。画布格子太小（<12 px，比如 128×128 画布）时数字自动隐藏避免糊成一坨。
  - 导出（`File → Export`）时 **Sphere 形状的格子暂时不会出现在导出模型里**（只有 Cube 会进 mesh），这是 v0.9 占位阶段预期行为——未来会补完 Sphere 的 glTF 几何，先不用担心。
  - 边界验证（2026-04-23 加）：如果**只画 Sphere、一个 Cube 都没有**就点 Export → 应当**立刻**弹红色 toast：`Export failed — no exportable geometry … (v0.9: Sphere cells are a placeholder ...)`，而**不是窗口卡死**。如果看到"卡死"超过 1 秒还没反应，在 stderr 里找 `export: writing` 日志——有这行说明正在写，没有这行是事件循环没唤醒，属于 bug，请截图发我。
  - 已修（2026-04-24）：**菜单唤起的所有 modal（New Project / Export / About / Delete palette color / Recover）** 之前默认定位在左上角 ~(32,32)，会和菜单栏 + 左 SidePanel + 浮动 Brush 面板重叠；用户感觉"点了没反应"其实是交互区域被盖住一半。现统一用 `egui::Align2::CENTER_CENTER` 钉在屏幕正中、不可拖动。验证：File → New / File → Export / Help → About / 右键色板 → Delete… 四个入口弹出来的都应该在**画布正中**，不再在左上。
  - **#1c-async**（2026-04-24 加）**异步导出 + 进度弹窗 + 日志**（v0.9 D）
    - 画一些 Cube（随便几格）→ `File → Export…` → 选 `.glb` → 保存到临时目录。
    - 点 Save 之后应立刻看到一个居中的 **"Exporting…" 小弹窗**，里面有转圈 spinner + 目标路径 + `elapsed Xs` 秒表（每帧递增）。
    - **窗口在导出期间应保持响应**：鼠标划过 canvas、拖画布、拖色板、按快捷键 `A` 切 Paint Mode 都不应该卡顿（写导出任务跑在 `AsyncComputeTaskPool` 后台线程，不占主事件循环）。
    - 导出完成 → 进度弹窗自动消失 → 右下角 toast 提示 `Exported to ...`。
    - stderr/日志里应按顺序看到：
      1. `ui: File → Export clicked — opening Export modal`
      2. `ui: opening save dialog for export (.glb, outline=true/false)`
      3. `ui: dispatching ExportRequest → /tmp/xxx.glb (Glb)`
      4. `export: starting /tmp/xxx.glb (Glb, outline=...)`
      5. `export: wrote /tmp/xxx.glb in 12.3ms`
      - 如果在 Save 弹框里按 Cancel：只会看到前 2 条 + `ui: save dialog cancelled — no ExportRequest dispatched`，**不会**触发后台任务。
    - 并发保护：导出进行中 `File` 菜单里 `Export…` 那项应变灰显示 `Export… (running)`，点不动。若你能诱发看到 `export: ignoring request for ... — another export is already running`，也算验证成功。
    - 失败场景：清空画布（全 Sphere 或全空）→ Export → 应立即弹红色 toast：`Export failed — no exportable geometry ...`，**不**应该看到进度弹窗卡在那。
- **#2**（30 min）三引擎导出验证
  - File → Export → `.glb` 格式 → outline 勾上 → 存到某临时路径。
  - **Godot 4**：`docs/export/godot.md` 第一步开始走；预期看到模型 + 黑色描边 mesh。
  - **Unity 6**：装 glTFast → 拖入 `.glb` → 预期几何 + vertex color 正确。
  - **Blender 4**：File → Import → glTF 2.0 → 预期 mesh + materials。
  - 三家都过了：勾掉；没过：在这行下面写失败现象。
- **#3**（2 min）`.gltf`（文本）格式导出
  - Export 时选 `.gltf`，确认目标目录同时出现 `foo.gltf` + `foo.bin` 两个文件。

--

- #1 Done
- #1b 我已经改了模式 你是不是也更新下

  > agent (2026-04-27): 已对接。Brush 浮动 HUD + Paint Mode（Overwrite / Additive 切换 + `A`
  > 快捷键 + 拖拽 stroke 防叠加保护）已经在 `src/ui.rs` 落盘，相应 v0.10 A/B 验收清单也已经在
  > 本文件下方列好了。如果你点 `A` 切的时候 HUD 没跟着切、或者状态栏文字没变，截图发我。

  > agent (2026-04-27 晚): v0.9 polish + v0.10 A/B/B-bis/C-1 之后对 brush HUD / 渲染管线 /
  > schema 都有动过，**dashboard ① 里的 `#1` / `#1b` 是建议复跑一次的待验项**——历史 done
  > 标记保留作为过往证据，但本次渲染管线版本不一样，最稳还是花 6 分钟把这俩重过一遍。

---

## v0.5 · CLI

- **#4**（已包含在 S-1）`cargo install` 后 CLI 在 `$PATH`。
- **#5**（5 min）CLI 导出在引擎开
  ```sh
  maquette-cli export some_project.maq --out test.glb
  ```
  然后把 `test.glb` 丢进 Godot/Unity/Blender 任一个打开。
- **#6**（1 min）`info --json` 解析
  ```sh
  maquette-cli info some_project.maq --json | jq
  ```
  输出应是合法 JSON（`jq` 不报错），含 `grid`, `palette`, `cells` 等字段。

---

## v0.6 · Palette / 笔画 / Greedy

- **#8**（3 min）色板编辑持久化
  - 右键任一色板色块 → 调色盘改色 → 点别处 → 颜色立即生效。
  - Save → 重开 → 颜色原封不动。
- **#9**（5 min）删除色 modal
  - 新建画布，用色 A 画几格，用色 B 画几格。
  - 右键色 A → Delete… → 选 "Erase" → 确认 → 色 A 的格子变空。
  - 再画一批色 A → 右键 → Delete… → 选 "Remap to 色 B" → 确认 → 色 A 的格子变色 B。
- **#10**（2 min）"+" 按钮与 `1-9` 快捷键
  - 点 "+" → 产生色相偏移的新色并自动选中。
  - 按 `1`、`2` ... `9`：依次选中第 n 个**活**色（跳过已删除的 slot）。
- **#11**（1 min）拖动笔画的 Undo
  - 一次拖拽横过 5 格 → `Cmd+Z` → 5 格一次性清空（不是一格一格）。
- **#12**（3 min）Greedy meshing 体积收益
  - 画一个 16×16 满铺同色画布 → Export `.glb` → 记录文件大小。
  - 参考：v0.5 同画布 export 会有 ~1500+ 三角形；v0.6 起应 ≤ ~12 三角形。文件应明显变小（几十 KB 量级 → 几 KB）。

---

## v0.7 · 渲染 / Palette 可移植

- **#13**（3 min）CLI `render` 和 GUI 预览视觉一致
  ```sh
  maquette-cli render some_project.maq --out preview.png --width 800 --height 600
  ```
  打开 `preview.png` 和 GUI 左下角等距预览对照；顶面应最亮、+Z/-X 面次之、另一侧最暗。角度 yaw=−45°、pitch≈35°，形状一致。
- **#14**（1 min）Headless 构建
  ```sh
  cargo build --no-default-features --bin maquette-cli
  ```
  预期：成功，且 `cargo tree --no-default-features --bin maquette-cli | grep -E "bevy_egui|bevy_panorbit|bevy_infinite_grid|bevy_mod_outline|rfd"` 返回空。
- **#15**（60 min）**跨引擎截图归档**（v1.0 前必做）
  - 选一个"代表作"（比如小房子或人物）→ CLI 导 `.glb`（outline 开）+ CLI render `.png`。
  - Godot 4 / Unity 6（glTFast）/ Blender 4 各打开一次，各截一张图。
  - 4 张图（CLI PNG + 三引擎截图）存到 `maquette/docs/export/screenshots/` 下，文件名形如 `house_cli.png`, `house_godot.png`, `house_unity.png`, `house_blender.png`。
  - 这是 v1.0 docs 里要引用的素材。
- **#16**（5 min）`palette` 往返
  ```sh
  maquette-cli palette export proj.maq --out colors.json
  # 手动编辑 colors.json，改一个 hex（比如把 "#7fbfff" 改成 "#ff8800"）
  maquette-cli palette import proj.maq --from colors.json --out proj2.maq
  ```
  打开 `proj2.maq` in GUI → 那个 slot 的所有格子改了颜色。

---

## v0.8 · 预览 UX

- **#17**（5 min）Multi-view PIPs
  - 画一个明显不对称的形状（比如 3×3 L 形、只在左半列高度 3）。
  - 确认：Top PIP 看到 L 形、Front PIP 看到不对称高度、Side PIP 看到另一侧轮廓。
  - 按 `F2` → PIPs 隐藏 → 再按 `F2` → 重现；位置恢复到右下角。
- **#17c**（2 min）PIP 色彩/坐标系区分（2026-04-23 加）
  - 三个 PIP 的边框颜色应当**不同**：Top = 绿、Front = 蓝、Side = 红（分别对应各视图"看不到"的那根轴），底部有一条细色条强化。
  - 每个 PIP 右上角有一个带半透明圆盘底的小坐标系小图：
    - Top → 红 `X` 指右、蓝 `Z` 指下（和 2D 画布一致）
    - Front → 红 `X` 指右、绿 `Y` 指上
    - Side → 绿 `Y` 指上、蓝 `Z` 指左（相机在 +X 观察）
  - 世界坐标轴（View → Show World Axes 开启时）应当**半透明**（约 55% 不透明度），不再完全盖住模型表面。
- **#17b**（2 min）PIP 点击 → 主预览对齐角度（2026-04-23 加）
  - 鼠标悬停 Top / Front / Side PIP → 边框变浅蓝，光标变指针。
  - 点任一个 PIP → 主预览平滑旋转到对应正面角度（不切正交，保持透视，但 yaw/pitch 对齐）。
  - 点 Top → 主预览从正上方俯视；点 Front → 看到 +Z 方向；点 Side → 看到 +X 方向。
  - 点完之后仍然能鼠标拖拽旋转 / 滚轮缩放，不锁死。
- **#18**（3 min）Float / Dock 姿态记忆 + 关闭联动（2026-04-23 加强）
  - 点右上 `Float` → 弹出 "Maquette Preview" 第二个 OS 窗口，带着当前相机姿态。
  - 在浮窗里 orbit 到一个明显新的角度 → 点浮窗 OS 关闭按钮 → `Float` 按钮**自动弹起**（回到 docked 状态），并且主窗口相机**同步到浮窗刚才的角度**（方便你无缝接着操作）。
  - 再次点 `Float` → 浮窗开在**上次浮窗**的姿态，不是原始的 docked 姿态。
  - 边缘验证：浮窗关了之后 `Float` 按钮必须立刻变灰/弹起，不能继续亮着。
- **#19**（2 min）Fit to Model
  - 新建 32×32 画布 → 仅一角画一格 → 按 `F` → 预览自动框住那一格，约占 70% 视口。
  - 按 `Cmd+R` → 预览回默认角度 + 距离。
- **#19b**（1 min）预览缩放按钮（2026-04-23 加）
  - 右上浮动工具栏最左侧现在有 `−` / `+` 两个小按钮（在 Fit / Reset 之前，用竖线隔开）。
  - 点 `+` 几下 → 主预览**拉近**（柱子变大），点 `−` 几下 → **拉远**。连点会被裁剪在 `[MIN_RADIUS=3, MAX_RADIUS=120]` 之间，不会无限穿模或缩到看不见。
  - 键盘快捷键 `+` / `=` 拉近、`-` 拉远（没有 Shift 需求）。滚轮原来的缩放继续能用。
- **#20**（1 min）Empty-state 提示
  - File → New → 画布中央有"Start painting"提示面板。
  - 画任一格 → 提示消失。
  - Edit → Clear Canvas → 提示重现。
- **#20b**（3 min）事件驱动渲染（2026-04-23 加，Godot 编辑器同款）
  - 打开 Maquette，**别动鼠标** → 开个 `top` 或活动监视器看 maquette 进程 CPU → 应该接近 0%（之前是 ~15–40%，持续渲染）。
  - 鼠标移到 canvas 上 → CPU 瞬间拉起，正常响应。
  - Cmd+Tab 切到别的 App → `top` 再看 maquette → 应该几乎完全 0%（unfocused 用 `ReactiveLowPower(60s)`）。
  - 切回 Maquette → 响应即时恢复。
  - 关键回归验证：点 PIP / 按 `F` Fit / 按 `Cmd+R` Reset → 主预览动画必须依然**平滑**（不是一跳到底）。动画靠 `request_redraw_while_animating` 在 panorbit 插值未完时每帧补 `RequestRedraw`，所以要边动边检查。

---

## v0.9 · 稳定性（Agent 尚在开发；到手后验收）

Agent 交付 `v0.9-complete.md` 后再走这些。

- **#21**（5 min）Autosave 恢复 ✅ **已到手（v0.9 A, 2026-04-23）**
  - **前置**：先随便保存一个 `.maq` 文件（autosave 需要文件路径；untitled 项目要等 v0.9 C）
  - 画几笔（几次 stroke 都行）→ 别手动 Save → 从终端 `kill -9 <maquette_pid>`
  - 确认 `ls <那个 maq 的目录>` 能看到 `foo.maq.swap` 旁生文件
  - 重开 maquette → File → Open → 选同一个 `.maq` → 弹出恢复 modal
  - 点 **Recover unsaved edits** → 内容回来，标题栏有 `•` 未保存标记 → Cmd+S 确认保存
  - 重新测一遍，这次点 **Discard swap and open saved file** → 打开的是 kill 之前保存的版本，swap 文件被删掉（`ls` 没有了）
  - 正常 Save 一次完整项目 → 确认目录里**没有**遗留 `.maq.swap`（干净保存后 swap 必须清理）
- **#22**（2 min）Prefs 持久化（到手时）
  - 开 `Multi`，开 `Float`，把 Brush Height 调到 5 → 退出。
  - 重开 → 三项状态全保留。
- **#23**（5 min）Release build 体积（到手时）
  ```sh
  cargo build --release
  ls -lh target/release/maquette
  ```
  目标：< 25 MB。
- **#24**（10 min）Perf 目测（到手时）
  - 用 release build 开 32×32 画布 → 拉高度到 8 → 涂满 → 开 Multi-view。
  - 主观 60 fps（拖动预览不卡顿）。在 M1 base 上应 OK。

---

## v1.0 · 发布候选（最后冲刺）

- **#25**（10 min）README / docs 审阅（Agent 写完后）
  - 读一遍 `README.md` → 描述是否准确？有没有夸大？
  - 可以加你自己的一段"为什么做 Maquette" 进 `docs/user-guide.md` 开头。
- **#26**（30 min）App Icon 拍板
  - 看 Agent 这次生成的 4 版图标提案 → 告诉 Agent 哪版或改哪里。
  - 最终 `.icns`（macOS）、`.ico`（Windows）、`.png` (1024 / 512 / 256 / 128 / 64 / 32 / 16) 都由 Agent 生成；你只做最终审美拍板。
- **#27**（30-60 min）Smoke Matrix
  - 每台目标系统各走一遍：`cargo install --path maquette` → 启动 → 新建 → 画 3 种形状 → Export `.glb` → 在 Blender 打开确认无误 → 关闭。
  - **必做**：macOS（你当前主力）
  - **强烈建议**：Linux（随便一个 Ubuntu 虚拟机即可）
  - **可选**：Windows（若无 Windows 机器可跳，在 v1.0 release notes 里注明"Linux/macOS 已验证，Windows 社区测试"）
- **#28**（1 min）打 Tag 发 v1.0
  - Agent 写好 CHANGELOG → 你本地：
    ```sh
    git tag v1.0.0
    git push --tags
    ```
  - Agent 不自动执行远端写，这步一定你来。

---

## v0.10 · AI 纹理 MVP（Phase 推进时逐项打开）

- **#TEX-A**（5 min · v0.10 A 已 ship）**离线 Mock provider 跑通**
  - 终端：`cargo run --bin maquette-cli -- texture gen --prompt "mossy stone" --seed 7 --out /tmp/a.png`
  - 同条命令再跑一次输出到 `/tmp/b.png` → `md5 /tmp/a.png /tmp/b.png` 应**完全一致**（确定性）。
  - 第二次终端会显示 `texgen: cache hit` 日志（用 `RUST_LOG=info` 跑可以看到）。
  - 不同 prompt（"grass tile"）→ 不同 md5 + 不同 cache_key。
  - 缓存目录：`~/.cache/maquette/textures/<64-char-hex>.png`，可以直接用 Preview.app 打开看（噪声色块，每个 prompt 一种基础色调，纯粹是占位用，不是产品级出图）。
  - 验证目的：**管道结构对、缓存有效、CLI 参数没接错**。真正的"AI 出图"要等 Phase B。
- **#TEX-B**（v0.10 B-bis 已 ship 2026-04-27 · 已对接 sonargrid 现网 `10.100.85.15:12121/ui`）**通过 Rustyme 扇出** ✅ 已联调通过
  - sonargrid 实际部署的是 **两条** 队列：
    - `rustyme:texgen-cpu:queue` — 程序化 CPU 合成（含 LLM 智能解析），免费，决定性
    - `rustyme:texgen-fal:queue` — Fal.ai FLUX schnell 真 AI 出图（worker 端配 `FAL_KEY` 后启用）
  - 默认走 `cpu`，省钱。要切 fal：`MAQUETTE_RUSTYME_PROFILE=fal`。
  - 现网 6 项验证全过（详见 `v0.10b-bis-complete.md` § 4）：
    ```sh
    export MAQUETTE_RUSTYME_REDIS_URL=redis://10.100.85.15:6379/0
    export MAQUETTE_RUSTYME_ADMIN_URL=http://10.100.85.15:12121
    cargo run --bin maquette-cli -- texture gen --provider rustyme \
        --prompt "ui-tag-success" --seed 1 \
        --width 64 --height 64 --no-cache -o /tmp/r.png
    file /tmp/r.png   # → PNG image data, 64 x 64
    ```
    - 同 prompt+seed → 字节级相同（md5 比对）
    - 第二次同请求 < 500ms（磁盘缓存命中，不再投 Rustyme）
    - `MAQUETTE_RUSTYME_STYLE_MODE=smart` + 中文 prompt → LLM (GLM-4-Flash 免费) 解析 → 噪声渲染 PNG
    - `MAQUETTE_RUSTYME_PROFILE=fal` → envelope 路由到 fal queue（worker 没 FAL_KEY 时 Maquette 优雅超时 + 自动 revoke，Admin API 可见 `status: REVOKED`）
  - 撤销 / 清队仍然可用：
    ```sh
    # 故意短超时让任务 timeout，看 revoke 日志
    MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS=1 cargo run --bin maquette-cli -- \
        texture gen --provider rustyme --prompt "x" --no-cache -o /tmp/z.png
    # 清空 cpu 队列（sonargrid Admin UI 里能看到 pending=0）
    cargo run --bin maquette-cli -- texture purge texgen-cpu
    ```
  - **顺手修了一个潜伏的 bug**：result 队列里有别人留下的回写时，原版代码会无限循环（`RPUSH` 到队尾、`BRPOP` 从队尾取，永远捞同一条）。修后秒过。`rustyme-py` 也有同款 bug，已知会上抛给段文彬。
- **#TEX-B-fal**（外部工作，对 sonargrid 那边的 ops）真接 Fal.ai
  - sonargrid Worker 已经实现了 `texgen-fal` Lua hook（见 `sonargrid/rustyme-lua/examples/scripts/texgen_fal.lua`），唯一缺的是部署时给容器 `-e FAL_KEY=sk-xxx`。
  - 配上之后重跑上面 `MAQUETTE_RUSTYME_PROFILE=fal` 那条命令，应能看到真 AI 草地 PNG 写入磁盘。计费 ~$0.003 / 张。
- **#TEX-C**（v0.10 C-1 lib 部分已 ship 2026-04-27 晚 · GUI 部分留 D-1）schema v4 向后兼容验证
  - **lib 端已通过 25 条单测**（`cargo test -p maquette --lib`）：v3 文件加载、v4 round-trip、palette_meta 长度修复、`legacy write_project` 产出 v4 等。详见 `v0.10c1-complete.md` § 2。
  - **GUI 部分等 D-1**：打开 v3 → 改一笔 → save → 新 `.maq` 里 `version=4` 且新字段默认值、文件能再次打开还原原样；`override_hint` 编辑进 undo 链 → Ctrl+Z → 空。
  - 目的：锁死"加字段不破旧档"的契约，未来再加字段时照这个跑一遍。
- **#TEX-C-cli**（2 min · v0.10 C-1 后立即可手验）schema v4 命令行回归
  - 拿一个**现成的 v0.9 / v0.10 A 版本存的 `.maq`** 文件（任何之前用得顺的项目都行），或者从 `maquette/tests/fixtures/` 找一个：
    ```sh
    maquette-cli info path/to/old.maq
    # 应正常打印 grid wxh / 调色板色数 / selected slot，等
    maquette-cli info path/to/old.maq --json | jq .
    # 应是合法 JSON，不含字段错位
    maquette-cli palette export path/to/old.maq --out /tmp/p.json
    cat /tmp/p.json | jq .
    # 应是合法 JSON，slot 数与上面一致
    ```
  - 然后 v3→v4 自动升级路径手验（**重点**）：
    ```sh
    maquette-cli export path/to/old.maq --out /tmp/old.glb
    # 仍能正常 export glb，无 schema 报错
    ```
  - **不通过现象**：`info` 报 `unsupported schema version` / `cell count mismatch` / serde error；任何这类错误就是 lib 兼容路径出问题，截图发我。
  - **预期内但不算 fail**：CLI 输出里**不会**显示 `model_description` 或 `texture_prefs` 字段——CLI 表层还没接入这些（D-1 才接），lib 内部已读到、也写出去，但 CLI 显示层暂不展示。
- **#TEX-D1**（待 ship · **关键体验里程碑**）一句话出全套贴图
  - 前提：sonargrid Worker Stage 1 跑通（#TEX-B 已绿）。
  - 打开一个现有小模型（建议先用自己搓的 4×4×2 泥土块），右侧有 "Material (AI)" 面板 →
    输入 `Minecraft 风格的草地泥土方块` → 点 Generate。
  - 期望：~几秒后画布上所有格子按各自 palette slot 贴上 AI 生成贴图；View 菜单 "Flat / Textured" 切换正常；切回 Flat 看到的纯色和生成前 100% 一样（没被覆盖）。
  - 日志：单次点击应看到 `N` 个 LPUSH（N = 非空 slot 数），同一个 `group_id`，最后一个 chord callback 回写。
  - 反例验证：把 `TexturePrefs::ignore_color_hint` 打开重新 Generate → 贴图色调会偏离调色板，恢复关闭后重新 Generate → 色调回到协调状态。
- **#TEX-D2**（D-1 之后）单 slot 再生 + 手写 override
  - palette list 里每个 slot 右边有 `[regenerate]` / `[edit hint]`；改 hint 后单独点 `[regenerate]` 只发一个任务；其它 slot 贴图不动。
  - 改 hint 应进 undo 链。

---

## v0.10 C-2 · 块 meta + 制品维护（2026-04-28 早 ship）

详细背景见 `v0.10c2-blockmeta-complete.md`。两条 smoke：

- **#BLOCK-cli**（3 min）`maquette-cli block` 三件套
  ```sh
  # 1) 内置 12 块（不联网）
  maquette-cli block list --source local
  # → 应是 5 列表格：ID / FROM / SHAPE / COLOR / TEXTURE HINT
  # → 共 12 行（amethyst/bone/brick/dirt/grass/ice/lava/moss/sand/stone/water/wood）
  # → 末尾打印 "12 blocks total"

  # 2) 单查
  maquette-cli block get grass --source local
  # → 应有 7 行：id / name / source / shape_hint / default_color / tags / texture_hint / description
  maquette-cli block get grass --json | jq .  # 合法 JSON

  # 3) 联网 sync（默认指向 https://hfrog.gamesci-lite.com）
  maquette-cli block sync
  # → 现网 hfrog 暂无 maquette-block/v1 制品，应输出
  #   "synced 0 blocks from https://hfrog.gamesci-lite.com (runtime=maquette-block/v1) → ~/.cache/maquette/blocks/hfrog/maquette-block/v1"
  # → 0 是预期的（数据没人传）；网络 200 + 协议解析 OK 就算 pass

  # 4) all 来源合并（不联网，读缓存）
  maquette-cli block list --source all
  # → 当前应该跟 --source local 一样 12 行；等 hfrog 上有数据后这里会增长
  ```
  - 不通过现象：sync 报 `Remote: GET https://...: connection failed`（网络 / 证书问题）；list/get 报 schema 错；--json 不是合法 JSON（jq 报错）。截图 + 命令发我。

- **#BLOCK-gui**（3 min）右侧 Block Library 面板
  1. 启动 maquette → 主窗口右侧应多一个 **`Block Library`** SidePanel（默 280 px 宽，显示卡片列表）
  2. 顶部一行：`[Sync hfrog]` 按钮 + 右侧 `12 blocks` 字样
  3. 中部分割线下：`Selected slot: #3` 字样（默认 palette 选中第 3 槽 = green）
  4. 滚动列表里能看到 12 个**块卡片**：左侧 28 px 色块 + 右侧 `名字 / id · local / tags / texture_hint`
  5. 拖滚动条，找到 `grass` 卡片 → 点 `[Bind to selected slot]`
     - 卡片边框变成蓝色 + 标签 `[✔ Bound]` + 出现 `[Unbind]` 按钮
     - 看 palette 第 4 槽 swatch 右下角应**多了一个小蓝圆点徽章**
     - toast 提示 `slot #3 bound to grass`
  6. **palette 那格右键**：菜单原本只有 `Delete…`，现在应该多了一行 **`Block: grass ▶`**（子菜单）；展开后看到所有块、`grass` 前面带 `✔`、底部有 `Unbind`
  7. 再点 Library 卡片的 `[Unbind]`（或右键菜单里的 `Unbind`）→ 蓝徽章消失，toast `slot #3 unbound (was grass)`
  8. 点 `[Sync hfrog]` → 按钮变 `[Syncing…]` 不可点 → < 1 秒后变回，toast `Synced 0 hfrog block(s)`
  9. 把 maquette 项目 Save → 用 `maquette-cli info <file>.maq --json | jq '.. | objects | select(.block_id?)'` 应**没**有 block_id（因为最后 unbind 了）
  - 不通过：右侧没出现面板（窗口太窄？拉宽试试）/ 蓝徽章不出现 / 子菜单 `Block: ▶` 没有 / Sync 按钮永远 `Syncing…` 不回 / 卡片没显示中文。截图 + stderr 日志（启动时 `RUST_LOG=info maquette` 看 `block_library:` 那几行）。
  - 想真看到 hfrog 来的块？让 ops 按 `v0.10c2-blockmeta-complete.md` § 8 的 curl 上传一个 `maquette-block/v1` 制品，然后再点 Sync 即可。卡片里那个 `local` 字样的会变成 `hfrog`。

---

## v0.10 C-3 · Block Composer 第二窗口 + 多轮调试 + Save / Publish（2026-04-28 下午 ship）

详细背景见 `v0.10c3-block-composer.md`。两条 smoke：

- **#COMPOSER-mock**（3 min）第二窗口骨架 + 离线 mock + 本地 draft
  1. 主窗口 menu bar 应该多了一个 `Window` 菜单 → 点 `New Block Composer…`
  2. 应该弹出**第二个 OS 窗口**：标题 `Maquette · Block Composer`，1100×720
  3. 第二窗口左上角有一个**浮动 `Shape` 选择器**：`Cube` / `Sphere` 两个按钮
  4. 第二窗口右侧有一个 SidePanel，从上到下：`Prompt`（多行）/ Provider 下拉 / Style mode 下拉 / Seed / Size / **Generate** 按钮
  5. 选 Provider = `mock (offline)`、prompt 填 `red brick wall`、点 Generate → 1 秒内右侧 History 出现 `#1 mock · seed 1 · 128×128` 一张卡片，带 `[✔ Selected]`
  6. 中央 3D 预览的 cube 表面应**贴上**生成的 PNG（一片低饱和的紫红色噪声，因为 mock 是 hash-derived noise）
  7. 浮窗里点 `Sphere` → 中央 cube 变成球
  8. 在右侧 `Save / Publish` 区域：id 填 `red_brick_test`、name `Red brick test`、点 **Save Local Draft** → toast `Saved local draft 'red_brick_test'`
  9. 切回主窗口（不关闭 composer），右侧 Block Library 应**立刻多一个新卡片** id=`red_brick_test`，来源 badge 是 **橙色 `draft`**
  10. 关闭第二窗口 OS 标题栏 X → 主窗口正常运行；Library 里那张 draft 卡片仍在
  - **不通过**：第二窗口不开 / Generate 报错（mock 不应该报错）/ 贴图没贴上 cube / draft 没出现在 Library / 关窗主窗口跟着退出。日志 `RUST_LOG=info maquette` 看 `block_library:` 和 `composer:` 行。

- **#COMPOSER-publish**（3 min）真生图 + 推 hfrog
  - **前置**：环境变量 `MAQUETTE_RUSTYME_REDIS_URL=redis://10.100.85.15:6379/0`、`MAQUETTE_HFROG_S3_INC_ID=<对应 bucket id>`（问 ops 拿）。hfrog URL 不用配，默认就是 `https://hfrog.gamesci-lite.com`；如果 ops 把 hfrog 部到了别的节点再用 `MAQUETTE_HFROG_BASE_URL` 覆盖。
  1. 同 `#COMPOSER-mock` 1-3 步开 Composer
  2. Provider 改 **`rustyme · cpu`**、prompt `iron block, weathered surface`、Style mode `auto`、点 Generate → 几秒后 History 多一张真生成的 PNG（颜色由 LLM 智能解析）
  3. 选这张为 Selected，id `iron_block`、name `铁块测试`、description `测试用铁块`、tags `metal,test`
  4. 点 **Publish to Hfrog** → 转圈后 toast `Published 'iron_block' to hfrog (pid=N)`
  5. 主窗口 Block Library 顶部点 **`Sync hfrog`** → 回来后应**多一张新卡片** id=`iron_block`，来源 badge 是 **蓝色 `hfrog`**
  6. 那张本地 draft 应该已经被删了（publish 成功后自动 dispose）
  - **不通过现象**：Publish 报 `hfrog refused publish (code=...)` 或 `S3 upload failed` → 说明 S3_INC_ID 没配对、或者 hfrog 那边某项校验。完整 stderr + toast 文字发我。
  - 注意 `0.0.<timestamp>` 形式的 ver 是 maquette 自动生成的，目的是保证 `(name, ver, runtime)` 三元组不撞；如果你后续想自定义 ver，给我说一下我们加个表单字段。

- **#D1-material**（4 min）Material 抽屉 + 持久化 + undo
  1. 主窗口左侧栏：依次能看到 `Canvas` → 22px Palette → **▾ Material** → ▾ Block Library 四块。展开 **Material** 抽屉。
  2. 多行文本框输入 `a Minecraft-style grass dirt block` → 点其它地方让 textarea 失焦
  3. 文件 → Save As 任意路径，关 maquette
  4. 重启 + 同一个文件 Open → Material 抽屉里那段文字应该**完整保留**
  5. 在文本框里追加 `, vibrant low-poly` → 失焦 → **Cmd+Z** → 那段追加应该消失（撤销回 step 2 的文本）。再 Cmd+Shift+Z → 回到追加后的版本
  6. 切换 `Ignore color hint in prompts` 复选框 → Cmd+Z 应该把它切回去（同一根 undo 链）
  7. View 下拉 Flat / Textured 切来切去（注意：D-1 阶段只持久化，3D 预览还**不会**真变贴图 — 那是 D-1.D 的事）
  - **不通过现象**：Save 后 Open 文字丢了 → autosave / write_project_with_meta 没接入；或者输入时每个字符都进 undo 栈（应该是失焦才一次）。日志 `RUST_LOG=info maquette` 看 `apply_open` / `flush_swap` 行有没有报丢字段。

- **#shortcuts**（3 min）全套键盘快捷键
  - **思路**：menu 上一切高频动作都应该有快捷键。下表是 v0.10 D-1 之后的当前现状（基础 7 项 + v0.10 新增 5 项）。挨个按一次确认无误。
  - **File / Edit**

    | 组合 | 动作 |
    |---|---|
    | `Cmd+N` | New 项目（弹尺寸对话框）|
    | `Cmd+O` | Open .maq 项目 |
    | `Cmd+S` | Save 当前项目（无路径时弹另存为）|
    | `Cmd+Shift+S` | Save As… 另存 |
    | `Cmd+Z` | Undo（笔画 + meta 编辑统一时间轴）|
    | `Cmd+Shift+Z` / `Cmd+Y` | Redo |
    | `Cmd+E` | Export… 弹导出对话框 |

  - **View**

    | 组合 | 动作 |
    |---|---|
    | `Cmd+R` | Reset Preview 视角 |
    | `F` | Fit to Model（聚焦到画好的几何体）|
    | `+` / `=` | 预览放大 |
    | `-` | 预览缩小 |
    | `F2` | Multi-view PIPs 显隐 |
    | `Shift+A` | **(新)** 世界坐标轴 (X/Y/Z) 显隐 |

  - **Paint**

    | 组合 | 动作 |
    |---|---|
    | `1` … `9` | 选第 N 个活色 palette slot |
    | `A` | Paint Mode 切 Overwrite ↔ Additive |
    | `[` | **(新)** Brush 高度 -1 |
    | `]` | **(新)** Brush 高度 +1 |

  - **Window / D-1 texgen**

    | 组合 | 动作 |
    |---|---|
    | `Cmd+B` | **(新)** 打开 New Block Composer 第二窗口 |
    | `G` | **(新)** 给当前选中 palette slot 生纹理（Mock lane，离线总成功）|
    | `Shift+G` | **(新)** 同上但走 Rustyme Fal lane（高质量；要 env）|

  1. 跑一遍 File / Edit 7 项：`Cmd+N` 弹新建对话框 → 取消 → `Cmd+S` 弹另存 → 取消 → `Cmd+Z` / `Cmd+Shift+Z` 撤销重做 → `Cmd+E` 弹导出 → 取消
  2. 跑一遍 View 6 项：`Cmd+R` 转视角 → `F` 重新 fit → `+/-` 缩放 → `F2` 切 PIP → `Shift+A` 切 axes
  3. 跑一遍 Paint 4 项：`1` `5` 切色 → `A` 切 Overwrite/Additive → `]` `]` `]` 把 brush 高度推到 4 → `[` 退回 → 看左下 Brush 浮窗的 Slider 跟着动
  4. `Cmd+B` 应该开第二窗口（Block Composer），关掉 → `G` 给当前 slot 出 Mock 纹理（toast 提示成功）→ 配好 env 后 `Shift+G` 走 Fal
  - **不通过现象**：键按了没反应（focus 在 textarea 时基本快捷键被吃掉是正常 — Cmd+S 仍应工作）/ Shift+A 切 paint mode 而没切 axes（说明事件被 A 抢先消费了 — 立即报）/ `[` `]` 不是 ascii 79/93 在某些键盘布局触发 OpenBracket/CloseBracket egui Key — 跨布局可能要补 layout-specific binding。完整 stderr `RUST_LOG=info maquette` 发我。

- **#D1-slotgen**（4 min）Palette 右键 → Generate texture → 三 lane
  - **前置**：先做完 `#D1-material` 给项目一个 `model_description`，效果更可见。
  1. 在 palette 选一个色（比如绿色），右键
  2. 应该能看到菜单：(色块编辑) / `Bind block ▶` / **`Generate texture ▶`** / `Delete…`
  3. 鼠标移到 **Generate texture** → 子菜单三项 `Mock (offline)` / `Rustyme CPU` / `Rustyme Fal`，每项 hover 都有提示文字
  4. 点 **Mock (offline)** → 顶部 toast `Generating slot #N: <derived prompt>` 立刻紧跟一条 `Mock (offline) done · slot #N · ...`
  5. 检查 `~/.cache/maquette/textures/` 应该多一个 `<sha256>.png`
  6. 同一个 slot 立刻再点 Mock → 提示 `slot #N already generating — waiting...`（虽然 Mock 几乎瞬间完成，连点要小心；正经看效果换 CPU/Fal lane）
  7. 配好 `MAQUETTE_RUSTYME_REDIS_URL` 后 → 点 **Rustyme CPU** → 几秒后 toast 成功 + cache 落新 PNG
  8. 配好 `MAQUETTE_RUSTYME_REDIS_URL` + `MAQUETTE_RUSTYME_MODEL=fal-ai/flux/schnell` 后 → 点 **Rustyme Fal** → 几十秒（Fal 本身延迟）后 toast 成功
  - **不通过现象**：右键菜单看不到 Generate texture（slot_texgen plugin 没注册）/ Mock 失败（cache_dir 失败 → log warn）/ Rustyme 报 `rustyme provider needs MAQUETTE_RUSTYME_REDIS_URL` → 检查 env。完整 stderr 发我。

---

## 如何把发现的问题回报给 Agent

- **小问题**（某个按钮字错、快捷键冲突）：直接在本文件对应 `- [ ]` 下面追加 `> 失败：...` 即可，我下次会读。
- **阻塞级**（app 启动崩 / 导出坏文件）：开新对话，发文件路径 + stderr + 重现步骤。
- **走哪版回归**：本文件的 `#X` 编号和 `NEXT.md` 的 verification debt 对齐，方便引用。

---

## 小工具：列出 dashboard ① 里未完成项

dashboard 表格在文件顶部，状态格 `- [ ]` / `- [x]` 直接修改即可。

```sh
# 看 ① 里还剩什么没勾
sed -n '/^### ① 现在就可以挨个验/,/^### ② 大块头/p' \
    maquette/docs/handoff/USER-TODO.md | grep -E '^\| - \[ \]'

# 全文找还没打勾的项
grep -nE '^- \[ \]|^\| - \[ \]' maquette/docs/handoff/USER-TODO.md
```

**建议刷的顺序**：

1. 一杯咖啡时间（≤ 30 min）：dashboard ① 里 1-3 min 那 18 项一口气过完。
2. 工作间隙（≤ 30 min）：① 里 4-5 min 那 7 项（含 `#1c-async` 异步导出 + `#TEX-A` 离线生图 + `#21` autosave 实测）。
3. 单独腾时间：dashboard ② —— `#2` 三引擎 / `#15` 截图归档 / `#27` smoke matrix。
4. dashboard ③ 等 agent 开发到对应版本（NEXT.md "Now" 里写的下一步是 `v0.10 D-1`，所以 `#TEX-D1` / `#TEX-D2` 估计是接下来 2-3 个 session 之内能到手）。