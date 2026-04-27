# Maquette · 用户验证 TODO（到 v1.0）

**给你自己用的勾选清单**。每项标注了所属版本、预计耗时，以及如何复现 / 判定通过。
做完一项就把 `- [ ]` 改成 `- [x]`，遇到不过的直接在条目下追加 `> 失败现象：...`，我下次看到会处理。

Agent 侧的进度与决策见 `NEXT.md` 与 `vX.Y-complete.md`。
Agent 一直在跑的 112 项自动化测试 + clippy + headless CLI 构建在每版交付时都会跑通；本清单只列**必须你亲眼看 / 真机跑 / 真引擎开**的事。

---

## 2026-04-27 — 本次刚到手的可验证清单

刚刚一波代码（v0.9 polish + v0.10 A/B）落盘。下面这些**全是新到手**、还没你亲眼验证过的：

| 编号 | 主题 | 预计耗时 |
|------|------|--------|
| `#1c` | 右键 cycle 形状 / Backspace 抹除 / 高度数字徽章 / Sphere 圈环标记 / 居中 modal | 4 min |
| `#1c-async` | 异步导出 + 进度弹窗 + 修 macOS 26 卡死 + 并发保护 + 失败 toast | 3 min |
| `#17b` | PIP 点击 → 主预览动画对齐角度 | 2 min |
| `#17c` | PIP 边框颜色 / 坐标系小图 / World axes 半透明 | 2 min |
| `#18` | Float 关闭联动 + 浮窗姿态记忆 | 2 min |
| `#19b` | 预览缩放 −/+ 按钮 + `+`/`-` 快捷键 | 1 min |
| `#20b` | 事件驱动渲染（Godot 编辑器同款，idle ≈ 0% CPU） | 3 min |
| `#21` | Autosave 恢复（v0.9 A 已 ship，等你 `kill -9` 实测一遍） | 5 min |
| `#TEX-A` | CLI 离线 mock provider + 磁盘缓存 + cache key 确定性 | 5 min |
| `#TEX-B` | Rustyme 端到端（**等 sonargrid 那边出 Stage 1 Echo worker 才能真验**，前置先看 #TEX-A 跑通 mock） | 5–10 min |

总预计 ~30–40 min 能把上面非阻塞项过一遍。`#TEX-B` 等 sonargrid worker。

接下来 agent 的下一步是 **v0.10 C 项目 schema v4**（不依赖 worker，能立即开工）—— 详见 `NEXT.md`。

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
- **#TEX-B**（v0.10 B 已 ship · 需 **本地起 sonargrid + 一个 texture.gen worker** 才能真验）**通过 Rustyme 扇出**
  - 前提：
    - 起 Rustyme（`cd /Users/admin/data0/public_work/sonargrid && just run` 或参考其 README），`QUEUE_0_NAME=texgen`、`QUEUE_0_KEY=rustyme:texgen:queue`、`QUEUE_0_RESULT_KEY=rustyme:texgen:result`。
    - 写/借一个 worker 订阅 `texture.gen` 任务，契约见 `maquette/docs/texture/rustyme.md`，**分阶段实现路线图见 `maquette/docs/texture/rustyme-worker-roadmap.md`**（Stage 1 Echo worker 半天能出货，够 `#TEX-B` 用；真 AI 出图是 Stage 2）。
  - 端到端跑通：
    ```sh
    export MAQUETTE_RUSTYME_REDIS_URL=redis://localhost:6379/0
    export MAQUETTE_RUSTYME_ADMIN_URL=http://localhost:12121
    cargo run --bin maquette-cli -- texture gen --provider rustyme \
        --prompt "grass tile" --seed 1 --no-cache -o /tmp/r.png
    ```
    期望：命令以 0 返回；`/tmp/r.png` 是合法 PNG；`RUST_LOG=info` 能看到 `rustyme LPUSH id=…` → `rustyme got result id=… bytes=…`。
  - 再跑一次**去掉** `--no-cache`：应秒返回 + `texgen: cache hit`，不再投递到 Rustyme。
  - 撤销 / 清队：
    ```sh
    # 故意用一个超短超时让任务 timeout，观察 revoke 日志
    MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS=1 cargo run --bin maquette-cli -- \
        texture gen --provider rustyme --prompt "x" --no-cache -o /tmp/z.png
    # 主动清空某个 queue（sonargrid Admin UI 里也能看到消息计数归零）
    cargo run --bin maquette-cli -- texture purge texgen
    ```
  - 验证目的：**Maquette 不跑任何 worker 代码、不管实际走哪个供应商**，只要 Rustyme 契约（`docs/texture/rustyme.md`）对得上就能出图。这是把 "选用哪家生图 API" 从代码里彻底解耦的前提。
- **#TEX-B-fal**（可选，Worker 侧工作 · 推荐 `sonargrid` 项目里实现）实际接入 Fal.ai FLUX schnell worker
  - 该 worker 只需：收 `texture.gen` 任务 → 调 Fal → 把返回的 PNG 用 base64 塞进 `{"png_b64": "..."}` → `LPUSH rustyme:texgen:result`。
  - 计费心理预期：schnell ~$0.003 / 张，2-3 s 延迟。
  - 上线后重跑 `#TEX-B`：应该看到真 AI 贴图写入磁盘缓存，`ls ~/.cache/maquette/textures/` 文件数++。
- **#TEX-C**（无 worker 依赖 · 先行完成）schema v4 向后兼容验证
  - 用 v0.9 版本存的 `.maq` 文件，在 v0.10 C 后的版本里打开 → 应能正常显示画布、原有调色板、涂色一切不变。
  - 打开后立刻存一次：新 `.maq` 文件里应出现 `model_description: ""`（或被忽略）、`texture: null`、`override_hint: null` 等默认字段，但打开仍能回到原样。
  - `override_hint` 编辑应进入 undo 链（改 → Ctrl+Z → 空）。
  - 目的：锁死"加字段不破旧档"的契约，未来再加字段时照这个跑一遍。
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

## 如何把发现的问题回报给 Agent

- **小问题**（某个按钮字错、快捷键冲突）：直接在本文件对应 `- [ ]` 下面追加 `> 失败：...` 即可，我下次会读。
- **阻塞级**（app 启动崩 / 导出坏文件）：开新对话，发文件路径 + stderr + 重现步骤。
- **走哪版回归**：本文件的 `#X` 编号和 `NEXT.md` 的 verification debt 对齐，方便引用。

---

## 小工具：一次性打印所有未完成项

```sh
grep -nE '^- \[ \]' maquette/docs/handoff/USER-TODO.md
```

打算在某个周末集中刷：`#1, #3, #8, #9, #10, #11, #17-#20` 这一批都是 1-5 分钟的 GUI 点按，一杯咖啡时间能清完。
`#2, #15, #27` 是真引擎 + 截图归档的大块头，预算一个下午。