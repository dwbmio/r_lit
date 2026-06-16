# deskpet

一个用 [Bevy](https://bevyengine.org) 编写的**高效、无边框、透明、置顶的 3D 桌面萌宠**。
它常驻在**系统托盘 / macOS 菜单栏**，点击托盘图标显示或隐藏萌宠。萌宠是一只绑好骨架、
带 Idle 动画的 3D 模型（由一张图经 Meshy / fal.ai 生成），会在桌面漫步、可拖拽、点击会蹦，
没绘制的地方鼠标点击穿透。还带一个 egui 小 HUD 做快捷控制。

## 特性

- **托盘 / 菜单栏常驻**：启动即隐藏，点托盘图标切换显隐；右键弹 Show / Hide / Quit 菜单。
  macOS 上以 *accessory* 模式运行（仅菜单栏、**无 Dock 图标**）。
- **从图片生成的 3D 萌宠**：加载绑骨 + Idle 动画的 `.glb`（`assets/block.glb`，由 fal.ai
  托管的 Meshy image-to-3d 生成）。无 `.glb` 时回退内置程序化史莱姆。HUD 可在两个生成模型
  （`block` / `blast`）间切换。
- **透明无边框置顶窗口**（`ClearColor(Color::NONE)` + `CompositeAlphaMode::PostMultiplied`）。
- **按像素感穿透**：透明区点击穿透到后面的程序；身体 / HUD 上的点击被接收。
- **可折叠 egui HUD**：一个小齿轮，点开是半透明面板（行走速度、漫步开关、Hop、Switch、Quit）。
- 随机漫步、点击蹦跳、拖拽移动、右键 / Esc 退出。
- **懒渲染**：自适应帧率——交互时约 60fps，静止时约 8fps 心跳，真正不动时 CPU ~0%。
- **精简内存**：裁掉 Bevy/egui 无用 feature（audio/picking/bevy_ui/手柄/sysinfo），萌宠贴图
  128²，MSAA 关闭。约 120MB 私有 / ~290MB 含 GPU。

## 操作

| 操作 | 效果 |
|------|------|
| 点托盘 / 菜单栏图标 | 切换萌宠显隐 |
| 右键托盘图标 | Show / Hide / Quit 菜单 |
| 左键点身体 | 打招呼蹦一下 |
| 左键拖身体 | 移动萌宠窗口 |
| 右键点身体 / Esc | 退出 |
| 齿轮（萌宠右上角） | 打开 HUD 面板 |
| HUD “Switch” | 切换 block ↔ blast |
| 点透明区域 | 穿透到后面的窗口 |

## 构建与运行

本仓库无根 `Cargo.toml` workspace，请在本目录内构建。

```bash
cd deskpet
cargo run            # 开发
cargo run --release  # 优化
```

`assets/` 按当前工作目录解析，所以从本目录直接跑二进制（`./target/release/deskpet`）也能找到模型。

## 生成 / 替换萌宠

萌宠是 `assets/` 下的 `.glb`。自带的这两个是用参考图经 **fal.ai 托管的 Meshy 6 image-to-3d**
（绑骨 + Idle 动画）生成，再把贴图降采样。流程脚本在 `tools/`：

```bash
export FAL_KEY=<uuid>:<secret>          # fal.ai API key

# 图片 -> 绑骨 + Idle 动画 GLB（action 0 = Idle）
python3 tools/fal_meshy.py gen --image tools/block.png --out assets/block.glb --action 0

# 把内嵌 2048² 贴图（16MB 显存）降到 128²（~64KB）
python3 tools/glb_shrink_texture.py assets/block.glb --size 128
```

想用自己的模型：把一个人形绑骨 `.glb` 放到 `assets/block.glb`（场景 0、动画 0 = idle）。
没有 `.glb` 就回退程序化史莱姆。

> 直连 Meshy（`tools/meshy_image_to_3d.py` / `meshy-animator` skill）需要**付费版** Meshy
> ——免费版创建任务返回 HTTP 402。fal.ai 托管了同款 Meshy 模型、按量计费（约 $1.5/个），
> 所以流程走 fal。

## 穿透原理

`bevy::window::CursorOptions::hit_test` 是整窗级开关，且为 `false` 时窗口收不到 Bevy 光标
事件。deskpet 每帧轮询**操作系统全局光标位置**（免权限：macOS 用 CGEvent、Windows 用
GetCursorPos），据此判断光标是否在身体 / HUD 上来切 `hit_test`。鼠标按键走 Bevy（因为需要
点击时 `hit_test` 一定开着）。靠近时还会聚焦窗口，否则 macOS 不给无边框 overlay 发事件。

## 内存 / 性能

| 手段 | 效果 |
|------|------|
| 萌宠贴图 2048² → 128² | 显存 16MB → 64KB |
| 裁 Bevy feature（去 audio/picking/bevy_ui/gilrs/sysinfo） | 体积更小、线程更少 |
| 裁 bevy_egui（只留 `render` + `default_fonts`） | 去掉剪贴板/URL + bevy_ui_render/picking |
| `Msaa::Off` | 释放多重采样目标 |
| 自适应帧率 | 静止 CPU ~0% |

可调常量在 `src/main.rs` 顶部（`PET_W`、`WIN_H`、`HUD_W`、`UI_SCALE`、行走/跳跃/帧率值）。

## 平台支持

| 平台 | 托盘 | 透明 | 穿透 | 状态 |
|------|:----:|:----:|:----:|:----:|
| Windows | 支持（任务栏托盘） | 支持 | 支持 | 支持 |
| macOS | 支持（菜单栏，无 Dock） | 支持 | 支持 | 支持 |
| Linux | `tray-icon` 不支持点击事件；Wayland 不能自定位 | best-effort | X11 不稳 | best-effort |

## 许可

MIT
