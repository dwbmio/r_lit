# Maquette · 用户验证 TODO（到 v1.0）

**给你自己用的勾选清单**。每项标注了所属版本、预计耗时，以及如何复现 / 判定通过。
做完一项就把 `- [ ]` 改成 `- [x]`，遇到不过的直接在条目下追加 `> 失败现象：...`，我下次看到会处理。

Agent 侧的进度与决策见 `NEXT.md` 与 `vX.Y-complete.md`。
Agent 一直在跑的 76 项自动化测试 + clippy + headless CLI 构建在每版交付时都会跑通；本清单只列**必须你亲眼看 / 真机跑 / 真引擎开**的事。

---

## 快速启动（装机后做一次）

- [ ] **S-1**（5 min）`cargo install --path maquette`；确认 `maquette` 和 `maquette-cli` 同时在 `$PATH` 上。
- [ ] **S-2**（2 min）`cargo test`（在你机器上再跑一次，≥ 76 passed）。
- [ ] **S-3**（1 min）`maquette` 冷启动 → 看到默认 16×16 画布 + 右上角浮动工具栏（Fit / Reset / Multi / Float）+ 画布中央的空态提示。

---

## v0.4 · 基础流水线（GUI 肉眼检查）

- [ ] **#1**（3 min）Brush Height 测试
  - 打开 GUI → 左侧 Brush 滑块依次设 1 / 3 / 8 → 分别画一格 → 右侧预览应看到对应高度的柱体。
  - 判定：高度跟滑块数值一致，上表面用 toon shader 正确着色。

- [ ] **#2**（30 min）三引擎导出验证
  - File → Export → `.glb` 格式 → outline 勾上 → 存到某临时路径。
  - **Godot 4**：`docs/export/godot.md` 第一步开始走；预期看到模型 + 黑色描边 mesh。
  - **Unity 6**：装 glTFast → 拖入 `.glb` → 预期几何 + vertex color 正确。
  - **Blender 4**：File → Import → glTF 2.0 → 预期 mesh + materials。
  - 三家都过了：勾掉；没过：在这行下面写失败现象。

- [ ] **#3**（2 min）`.gltf`（文本）格式导出
  - Export 时选 `.gltf`，确认目标目录同时出现 `foo.gltf` + `foo.bin` 两个文件。

---

## v0.5 · CLI

- [ ] **#4**（已包含在 S-1）`cargo install` 后 CLI 在 `$PATH`。

- [ ] **#5**（5 min）CLI 导出在引擎开
  ```sh
  maquette-cli export some_project.maq --out test.glb
  ```
  然后把 `test.glb` 丢进 Godot/Unity/Blender 任一个打开。

- [ ] **#6**（1 min）`info --json` 解析
  ```sh
  maquette-cli info some_project.maq --json | jq
  ```
  输出应是合法 JSON（`jq` 不报错），含 `grid`, `palette`, `cells` 等字段。

---

## v0.6 · Palette / 笔画 / Greedy

- [ ] **#8**（3 min）色板编辑持久化
  - 右键任一色板色块 → 调色盘改色 → 点别处 → 颜色立即生效。
  - Save → 重开 → 颜色原封不动。

- [ ] **#9**（5 min）删除色 modal
  - 新建画布，用色 A 画几格，用色 B 画几格。
  - 右键色 A → Delete… → 选 "Erase" → 确认 → 色 A 的格子变空。
  - 再画一批色 A → 右键 → Delete… → 选 "Remap to 色 B" → 确认 → 色 A 的格子变色 B。

- [ ] **#10**（2 min）"+" 按钮与 `1-9` 快捷键
  - 点 "+" → 产生色相偏移的新色并自动选中。
  - 按 `1`、`2` ... `9`：依次选中第 n 个**活**色（跳过已删除的 slot）。

- [ ] **#11**（1 min）拖动笔画的 Undo
  - 一次拖拽横过 5 格 → `Cmd+Z` → 5 格一次性清空（不是一格一格）。

- [ ] **#12**（3 min）Greedy meshing 体积收益
  - 画一个 16×16 满铺同色画布 → Export `.glb` → 记录文件大小。
  - 参考：v0.5 同画布 export 会有 ~1500+ 三角形；v0.6 起应 ≤ ~12 三角形。文件应明显变小（几十 KB 量级 → 几 KB）。

---

## v0.7 · 渲染 / Palette 可移植

- [ ] **#13**（3 min）CLI `render` 和 GUI 预览视觉一致
  ```sh
  maquette-cli render some_project.maq --out preview.png --width 800 --height 600
  ```
  打开 `preview.png` 和 GUI 左下角等距预览对照；顶面应最亮、+Z/-X 面次之、另一侧最暗。角度 yaw=−45°、pitch≈35°，形状一致。

- [ ] **#14**（1 min）Headless 构建
  ```sh
  cargo build --no-default-features --bin maquette-cli
  ```
  预期：成功，且 `cargo tree --no-default-features --bin maquette-cli | grep -E "bevy_egui|bevy_panorbit|bevy_infinite_grid|bevy_mod_outline|rfd"` 返回空。

- [ ] **#15**（60 min）**跨引擎截图归档**（v1.0 前必做）
  - 选一个"代表作"（比如小房子或人物）→ CLI 导 `.glb`（outline 开）+ CLI render `.png`。
  - Godot 4 / Unity 6（glTFast）/ Blender 4 各打开一次，各截一张图。
  - 4 张图（CLI PNG + 三引擎截图）存到 `maquette/docs/export/screenshots/` 下，文件名形如 `house_cli.png`, `house_godot.png`, `house_unity.png`, `house_blender.png`。
  - 这是 v1.0 docs 里要引用的素材。

- [ ] **#16**（5 min）`palette` 往返
  ```sh
  maquette-cli palette export proj.maq --out colors.json
  # 手动编辑 colors.json，改一个 hex（比如把 "#7fbfff" 改成 "#ff8800"）
  maquette-cli palette import proj.maq --from colors.json --out proj2.maq
  ```
  打开 `proj2.maq` in GUI → 那个 slot 的所有格子改了颜色。

---

## v0.8 · 预览 UX

- [ ] **#17**（5 min）Multi-view PIPs
  - 画一个明显不对称的形状（比如 3×3 L 形、只在左半列高度 3）。
  - 确认：Top PIP 看到 L 形、Front PIP 看到不对称高度、Side PIP 看到另一侧轮廓。
  - 按 `F2` → PIPs 隐藏 → 再按 `F2` → 重现；位置恢复到右下角。

- [ ] **#18**（3 min）Float / Dock 姿态记忆
  - 点右上 `Float` → 弹出 "Maquette Preview" 第二个 OS 窗口，带着当前相机姿态。
  - 在浮窗里 orbit 到一个明显新的角度 → 点浮窗 OS 关闭按钮 → 主窗口相机**不变**（`Float` 按钮自己弹起）。
  - 再次点 `Float` → 浮窗开在**上次浮窗**的姿态，不是原始的 docked 姿态。

- [ ] **#19**（2 min）Fit to Model
  - 新建 32×32 画布 → 仅一角画一格 → 按 `F` → 预览自动框住那一格，约占 70% 视口。
  - 按 `Cmd+R` → 预览回默认角度 + 距离。

- [ ] **#20**（1 min）Empty-state 提示
  - File → New → 画布中央有"Start painting"提示面板。
  - 画任一格 → 提示消失。
  - Edit → Clear Canvas → 提示重现。

---

## v0.9 · 稳定性（Agent 尚在开发；到手后验收）

Agent 交付 `v0.9-complete.md` 后再走这些。

- [ ] **#21**（3 min）Autosave 恢复（到手时）
  - 画几笔 → 别保存 → 从终端 `kill -9 <maquette_pid>`。
  - 重开 maquette → 弹出恢复 modal → 点 Recover → 内容回来。

- [ ] **#22**（2 min）Prefs 持久化（到手时）
  - 开 `Multi`，开 `Float`，把 Brush Height 调到 5 → 退出。
  - 重开 → 三项状态全保留。

- [ ] **#23**（5 min）Release build 体积（到手时）
  ```sh
  cargo build --release
  ls -lh target/release/maquette
  ```
  目标：< 25 MB。

- [ ] **#24**（10 min）Perf 目测（到手时）
  - 用 release build 开 32×32 画布 → 拉高度到 8 → 涂满 → 开 Multi-view。
  - 主观 60 fps（拖动预览不卡顿）。在 M1 base 上应 OK。

---

## v1.0 · 发布候选（最后冲刺）

- [ ] **#25**（10 min）README / docs 审阅（Agent 写完后）
  - 读一遍 `README.md` → 描述是否准确？有没有夸大？
  - 可以加你自己的一段"为什么做 Maquette" 进 `docs/user-guide.md` 开头。

- [ ] **#26**（30 min）App Icon 拍板
  - 看 Agent 这次生成的 4 版图标提案 → 告诉 Agent 哪版或改哪里。
  - 最终 `.icns`（macOS）、`.ico`（Windows）、`.png` (1024 / 512 / 256 / 128 / 64 / 32 / 16) 都由 Agent 生成；你只做最终审美拍板。

- [ ] **#27**（30-60 min）Smoke Matrix
  - 每台目标系统各走一遍：`cargo install --path maquette` → 启动 → 新建 → 画 3 种形状 → Export `.glb` → 在 Blender 打开确认无误 → 关闭。
  - **必做**：macOS（你当前主力）
  - **强烈建议**：Linux（随便一个 Ubuntu 虚拟机即可）
  - **可选**：Windows（若无 Windows 机器可跳，在 v1.0 release notes 里注明"Linux/macOS 已验证，Windows 社区测试"）

- [ ] **#28**（1 min）打 Tag 发 v1.0
  - Agent 写好 CHANGELOG → 你本地：
    ```sh
    git tag v1.0.0
    git push --tags
    ```
  - Agent 不自动执行远端写，这步一定你来。

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
