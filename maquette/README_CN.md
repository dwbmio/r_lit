# Maquette

> 积木式低模资产生成器 —— 拼方块，导 3D 模型和 2D 精灵。

**Maquette** 是一个面向独立游戏开发者的轻量级桌面工具。不用开 Blender，就能用基础几何体拼出
低多边形道具，顺手把各个角度的 2D sprite 批量烘焙出来。从方块库里选形状、丢进视口、
摆位置上色，最后导出 glTF / OBJ 或者一整套方向 sprite。

名字取自法语 / 电影业术语 "maquette"——建筑/场景的**模块化缩比模型**，和这个工具干的事是同一回事。

## 当前状态

**MVP · v0.1** —— 基础骨架：

- 带 3D 视口的主窗口，轨道相机操作
- 无限地面网格 + 世界坐标轴
- 左侧方块库面板，内置 6 种基础体（立方、球、圆柱、圆锥、平面、圆环）
- 右侧属性面板，显示最后生成方块的 Transform
- 顶部菜单栏骨架（File / Edit / View / Help），按钮已连但逻辑待填
- 底部状态栏，显示方块数量和操作提示

尚未实现（v0.2+ 规划）：

- 视口内点选方块（`bevy_picking`）
- 变换 gizmo（`transform-gizmo-bevy`）
- 自定义方块导入（用户的 glTF / OBJ）
- 工程存档（自研 `.maq` JSON 格式）
- 整场景 glTF 导出
- 多方向 sprite 烘焙（8 / 16 / 24 向）
- 程序化脚本钩子（rhai 或 mlua）

## 技术栈

| 组成 | Crate | 版本 |
|------|-------|------|
| 引擎 / 3D | `bevy` | 0.18 |
| UI 面板 | `bevy_egui` | 0.39 |
| 相机 | `bevy_panorbit_camera` | 0.34 |
| 地面网格 | `bevy_infinite_grid` | 0.18 |

## 构建

在本目录下：

```bash
cargo run                # dev 版本（依赖已优化，迭代快）
cargo build --release    # 发布版，产物在 target/release/maquette
```

从仓库根目录：

```bash
just build maquette release
```

首次构建会比较久，Bevy 和它的依赖体量不小。之后的增量编译会很快。

## 操作

| 输入 | 动作 |
|------|------|
| 视口内左键拖拽 | 轨道旋转相机 |
| 右键拖拽 / 中键拖拽 | 平移相机 |
| 滚轮 | 缩放 |
| 点击库里的方块 | 生成到场景 |
| Edit → Clear Scene | 清空全部方块 |

## 目录结构

```
src/
├── main.rs      # App 入口，插件装配
├── camera.rs    # 轨道相机初始化
├── scene.rs     # 光照、背景、地面网格
├── block.rs     # BlockKind 枚举、生成/清理 system、Message 定义
└── ui.rs        # egui 面板（菜单、库、属性、状态栏）
```

UI 部分是 **egui 即时模式**，**不是 ECS**。ECS 只用来存场景数据（方块、变换、相机）。
加一个新功能通常就是：一个 `Message` 类型 + 一个消费它的 Bevy system。

## 设计说明

- 每个放置的方块是一个 ECS entity，带 `BlockKind` + `Transform` + `Mesh3d` +
  `MeshMaterial3d` + `Name` 组件
- UI 到场景的通信是单向的——靠 `Message`（`SpawnBlockEvent`、`ClearSceneEvent`），
  不存在共享可变状态
- Bevy 的 `EguiPrimaryContextPass` 调度每帧跑一次 UI（多 pass 模式）

## 许可证

MIT

## 在 r_lit 仓库中的位置

本 crate 属于 [r_lit](../README.md) 的 Rust 工具集。
和仓库里其他**短时运行 CLI 工具**不同，Maquette 是一个长期运行的桌面程序——它放在这里
是一个刻意的例外，因为它共享同一套构建基础设施。
