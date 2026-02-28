# Dev Mode 使用指南

## 概述

`dev` 命令可以同时启动多个独立的应用实例，方便在单机环境下测试多用户协作功能。

## 基本用法

```bash
# 启动 2 个实例（默认）
./target/release/group_vibe_workbench dev

# 启动 3 个实例
./target/release/group_vibe_workbench dev -c 3

# 启动 4 个实例，自定义窗口大小
./target/release/group_vibe_workbench dev -c 4 --width 600 --height 500
```

## 命令参数

```
dev [OPTIONS]

Options:
  -c, --count <COUNT>          Number of instances to launch [default: 2]
      --base-port <BASE_PORT>  Base port for instances [default: 9000]
      --width <WIDTH>          Window width [default: 800]
      --height <HEIGHT>        Window height [default: 600]
  -h, --help                   Print help
```

## 工作原理

### 1. 独立数据目录

每个实例使用独立的数据目录：
- Instance 0: `./workbench_data_dev_0/`
- Instance 1: `./workbench_data_dev_1/`
- Instance 2: `./workbench_data_dev_2/`
- ...

这样可以避免数据库锁定和数据冲突。

### 2. 自动命名

实例会自动分配用户名：
- Instance 0: Alice (Dev0)
- Instance 1: Bob (Dev1)
- Instance 2: Charlie (Dev2)
- Instance 3: David (Dev3)
- ...

### 3. 窗口平铺

窗口会自动平铺排列，避免重叠：
```
┌─────────┬─────────┐
│ Alice   │ Bob     │
├─────────┼─────────┤
│ Charlie │ David   │
└─────────┴─────────┘
```

### 4. 共享文件

所有实例共享同一个协作文件：
- **路径**: `../chat.ctx`
- 任何实例的编辑都会同步到其他实例

## 测试协作流程

### 步骤 1: 启动实例

```bash
cargo build --release
./target/release/group_vibe_workbench dev -c 2
```

你会看到：
```
╔════════════════════════════════════════════════════════════╗
║  Development Mode: 2 Instances Running                     ║
╠════════════════════════════════════════════════════════════╣
║                                                            ║
║  Test Collaboration:                                       ║
║  1. Create a group in one instance                         ║
║  2. Join the group from other instances                    ║
║  3. Click '开始协作' in all instances                       ║
║  4. Edit ../chat.ctx to test file sync                     ║
║                                                            ║
║  Shared File: ../chat.ctx                                  ║
║  Data Dirs: ./workbench_data_dev_0 to _1                   ║
║                                                            ║
║  Press Ctrl+C to stop all instances                        ║
║                                                            ║
╚════════════════════════════════════════════════════════════╝
```

### 步骤 2: 创建群组（Alice）

在第一个窗口（Alice）：
1. 点击"创建新群组"
2. 记住群组ID（例如：`group_abc123`）
3. 进入群组大厅

### 步骤 3: 加入群组（Bob）

在第二个窗口（Bob）：
1. 等待几秒让 mDNS 发现生效
2. 应该能看到 Alice 的群组
3. 点击"加入"
4. 进入群组大厅

### 步骤 4: 开始协作

在两个窗口中：
1. 点击"🚀 开始协作"按钮
2. 看到 Toast 提示："开始协作！共享文件: chat.ctx"

### 步骤 5: 测试文件同步

打开终端编辑共享文件：
```bash
# 使用你喜欢的编辑器
vim ../chat.ctx
# 或
code ../chat.ctx
# 或
nano ../chat.ctx
```

编辑内容并保存，例如：
```
# 群组协作文件: group_abc123

欢迎来到协作空间！

使用你喜欢的编辑器编辑此文件，所有更改会自动同步到群组成员。

Alice: 大家好！
Bob: 你好 Alice！
```

### 步骤 6: 观察同步

查看日志输出：
```
[2026-02-28T06:30:15Z][INFO] File changed, syncing 234 bytes
[2026-02-28T06:30:15Z][INFO] Synced to swarm successfully
```

在另一个实例中也应该看到类似的日志，表示收到了同步的内容。

### 步骤 7: 停止测试

按 `Ctrl+C` 停止所有实例：
```
^C

Received Ctrl+C, shutting down all instances...
[2026-02-28T06:31:00Z][INFO] Stopping instance 0...
[2026-02-28T06:31:00Z][INFO] Stopping instance 1...
All instances stopped.

Cleanup development data directories? (y/N):
```

选择是否清理测试数据：
- 输入 `y`: 删除所有 `workbench_data_dev_*` 目录
- 输入 `n` 或直接回车: 保留数据供下次测试使用

## 高级用法

### 测试更多用户

```bash
# 启动 4 个实例测试多人协作
./target/release/group_vibe_workbench dev -c 4
```

### 自定义窗口布局

```bash
# 小窗口，适合笔记本屏幕
./target/release/group_vibe_workbench dev -c 2 --width 600 --height 400

# 大窗口，适合大屏幕
./target/release/group_vibe_workbench dev -c 2 --width 1000 --height 800
```

### 测试网络问题

```bash
# 使用不同的端口范围
./target/release/group_vibe_workbench dev --base-port 10000
```

## 调试技巧

### 1. 查看日志

每个实例的日志会输出到 stdout，你可以重定向到文件：
```bash
./target/release/group_vibe_workbench dev 2>&1 | tee dev.log
```

### 2. 检查数据目录

```bash
# 查看实例 0 的数据
ls -la ./workbench_data_dev_0/

# 查看用户数据库
sqlite3 ./workbench_data_dev_0/user.db "SELECT * FROM users;"

# 查看 Swarm 数据
ls -la ./workbench_data_dev_0/swarm/
```

### 3. 监控文件变化

在另一个终端监控共享文件：
```bash
watch -n 1 cat ../chat.ctx
```

或使用 `tail -f` 查看实时变化：
```bash
tail -f ../chat.ctx
```

### 4. 网络调试

检查 mDNS 广播：
```bash
# macOS
dns-sd -B _murmur._tcp

# Linux
avahi-browse -a
```

## 常见问题

### Q: 窗口重叠了怎么办？

A: 手动调整窗口位置，或使用更小的窗口尺寸：
```bash
./target/release/group_vibe_workbench dev --width 600 --height 400
```

### Q: 实例无法发现彼此？

A: 检查：
1. 防火墙是否阻止了 mDNS（端口 5353）
2. 是否在同一网络
3. 等待几秒让 mDNS 生效

### Q: 文件同步不工作？

A: 检查：
1. 是否所有实例都点击了"开始协作"
2. 查看日志是否有错误
3. 确认 `../chat.ctx` 文件存在且可写

### Q: 数据库锁定错误？

A: 确保：
1. 没有其他实例在运行
2. 使用 `dev` 命令而不是手动启动多个 `launch`
3. 清理旧的数据目录

### Q: 如何重置测试环境？

A: 删除所有开发数据：
```bash
rm -rf ./workbench_data_dev_*
rm -f ../chat.ctx
```

## 与生产环境的区别

| 特性 | Dev Mode | Production |
|------|----------|------------|
| 数据目录 | `workbench_data_dev_N` | `workbench_data` |
| 用户名 | 自动分配 | 用户输入 |
| 窗口位置 | 自动平铺 | 居中 |
| 多实例 | 支持 | 需手动管理 |
| 清理 | 自动提示 | 手动 |

## 最佳实践

1. **先测试 2 个实例**: 确保基本功能正常
2. **逐步增加**: 测试 3-4 个实例的性能
3. **保留日志**: 使用 `tee` 保存日志供分析
4. **定期清理**: 测试完成后清理开发数据
5. **使用版本控制**: 不要提交 `workbench_data_dev_*` 到 git

## 示例测试场景

### 场景 1: 基本协作

```bash
# 启动 2 个实例
./target/release/group_vibe_workbench dev

# Alice 创建群组
# Bob 加入群组
# 两人开始协作
# 编辑 chat.ctx 测试同步
```

### 场景 2: 多人协作

```bash
# 启动 4 个实例
./target/release/group_vibe_workbench dev -c 4

# Alice 创建群组
# Bob, Charlie, David 依次加入
# 所有人开始协作
# 轮流编辑文件，观察同步
```

### 场景 3: 压力测试

```bash
# 启动 6 个实例
./target/release/group_vibe_workbench dev -c 6

# 所有人加入同一群组
# 快速编辑文件，测试冲突解决
# 观察 CPU 和内存使用
```

## 相关文档

- [COLLABORATION_IMPLEMENTATION.md](COLLABORATION_IMPLEMENTATION.md) - 协作功能实现
- [CLAUDE.md](CLAUDE.md) - 项目开发指南
- [README.md](README.md) - 用户文档
