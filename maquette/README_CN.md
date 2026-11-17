# Maquette

> 积木式低模资产生成器 —— 堆方块、上材质，导出 3D 模型与 2D 等距渲染图。

**Maquette**（v0.1.1）是一个面向独立游戏开发者的轻量级 Rust 建模工具，由**无窗口核心库**（`maquette`）、**无头命令行工具**（`maquette-cli`）和 **Bevy + egui 桌面客户端**（`maquette`）组成，无需启动 Blender 即可快速拼装 low-poly 道具与资产。

## 核心特性

- **GUI / 无头 CLI 双模架构**：`maquette-cli` 可通过 `--no-default-features --bin maquette-cli` 编译（完全不带 `bevy_egui` / `wgpu` 窗口依赖），适合 CI 缩略图生成与批处理导出。
- **积木建模与组合器**：网格编辑器与多块组合器（`block_composer`），内置 12 种基础块并支持从 `hfrog` 同步块元数据（`block_meta`），支持撤销/重做（`history`）与 `.maq` 工程存档/自动保存。
- **glTF 2.0 / GLB 导出**：支持导出 `.glb` / `.gltf`，内置反面法线描边烘焙（`--outline-width`、`--outline-color`、`--no-outline`），GUI 与 CLI 共用同一套导出引擎。
- **纯 CPU 等距光栅渲染**：`maquette-cli render` 无需 GPU 或桌面会话即可直接把 `.maq` 工程渲染成 PNG 预览图。
- **调色板与 AI 贴图管线**：支持 `colors.json` 调色板导入/导出（`palette_io`）、槽位贴图元数据（`texture_meta`）、离线确定性 `mock` 生成器与基于 Rustyme（sonargrid/Redis）的 AI 贴图生成及 SHA-256 本地缓存。
- **桌面端增强**：轨道相机、卡通着色预览、多视角画中画（`multiview`）、浮动窗口与云端任务队列状态展示。

## 技术栈

| 组成 | Crate | 版本 |
|---|---|---|
| 核心 / ECS / 3D | `bevy` | 0.18 |
| UI 面板（`gui` feature） | `bevy_egui` | 0.39 |
| 轨道相机（`gui` feature） | `bevy_panorbit_camera` | 0.34 |
| 无限地面网格（`gui` feature） | `bevy_infinite_grid` | 0.18 |
| 轮廓描边（`gui` feature） | `bevy_mod_outline` | 0.12 |
| CLI | `clap` | 4 |
| 队列 / HTTP / 缓存 | `redis`、`ureq`、`sha2`、`png` | - |

## 安装与构建

预编译二进制安装（Linux / macOS）：

```bash
curl -fsSL https://r2.gamesci-lite.com/r_lit/maquette/install.sh | bash
```

本地源码构建（在 `maquette/` 目录下）：

```bash
# 桌面 GUI + CLI（默认开启 `gui` feature）
cargo build --release
cargo run --bin maquette

# 仅编译无头 CLI（跳过 GUI 依赖，CI 极速构建）
cargo build --no-default-features --bin maquette-cli --release
cargo test --no-default-features
```

也可在仓库根目录执行：

```bash
just build maquette release
```

## 命令行用法（`maquette-cli`）

```bash
# 导出 .maq 工程为 GLB / glTF（支持反面法线描边）
maquette-cli export scene.maq -o scene.glb
maquette-cli export scene.maq -o scene.gltf --no-outline

# 查看工程摘要与结构校验（遵循 rlit_cli 契约：支持全局 `--json` 与 `__schema`）
maquette-cli info scene.maq --json
maquette-cli validate scene.maq
maquette-cli __schema

# 纯 CPU 渲染等距视角 PNG 预览图
maquette-cli render scene.maq -o preview.png --width 512 --height 512 --background '#181a1e'

# 调色板导入 / 导出
maquette-cli palette export scene.maq -o colors.json
maquette-cli palette import scene.maq --from colors.json -o scene_recolored.maq

# 贴图生成（mock 离线确定性 / rustyme 集群）
maquette-cli texture gen --prompt "mossy stone tile" -o stone.png --provider mock --seed 42
maquette-cli texture revoke <TASK_UUID>
maquette-cli texture purge texgen

# 块库元数据查询与同步（本地内置 + hfrog 制品库）
maquette-cli block list --source all --json
maquette-cli block get oak_planks
maquette-cli block sync
```

## 目录结构

```text
src/
├── lib.rs              # 无窗口核心库（Headless Invariant）
├── bin/
│   └── maquette_cli.rs # 无头命令行入口（`maquette-cli`）
├── main.rs             # Bevy + egui 桌面 GUI 入口（`maquette`）
├── grid.rs             # 2D/体素网格模型与调色板
├── mesher.rs           # 体素网格转三角网格
├── project.rs          # `.maq` 工程文件读写与序列化
├── export.rs           # glTF 2.0 / GLB 导出与反面描边烘焙
├── render.rs           # 纯 CPU 等距视角 PNG 光栅器
├── palette_io.rs       # `colors.json` 调色板导入/导出
├── texture_meta.rs     # 槽位贴图元数据与 TexturePrefs
├── texgen.rs           # Mock / Rustyme AI 贴图生成器与 SHA-256 缓存
├── block_meta.rs       # BlockMeta trait、LocalProvider 与 HfrogProvider
└── （GUI 专属模块）     # ui, camera, scene, block_library, block_composer,
                        # preview_mesh, multiview, export_dialog, slot_texgen,
                        # texture_registry, toon, history, autosave, session,
                        # float_window, notify, cloud_status, fonts
```

## 关于 r_lit

本 crate 隶属 [r_lit](https://github.com/dwbmio/r_lit) 仓库；桌面端 `maquette` 与无头 `maquette-cli` 共享同一套核心库。
