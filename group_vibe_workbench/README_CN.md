# Group Vibe Workbench

基于 GPUI + Murmur P2P 的桌面协作工作台。多用户可在局域网内零配置发现彼此，实时协同编辑共享 Prompt 上下文文件。

## 功能

- 原生桌面应用 (GPUI GPU 加速渲染)
- 零配置 P2P — mDNS 自动发现局域网内的群组和成员
- 创建或加入群组，动态发现已有群组
- CRDT 无冲突同步 (Automerge)
- 自动 Leader 选举 (Bully 算法)
- 本地持久化 + 分布式同步
- NAT 穿透 (iroh-net)

## 安装

```bash
cd group_vibe_workbench
cargo build --release
```

### Linux 额外依赖

```bash
# Ubuntu/Debian
sudo apt-get install libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev
```

## 使用

```bash
# 启动
group_vibe_workbench launch --nickname "Alice"

# 自定义窗口尺寸
group_vibe_workbench launch -n "Alice" --width 1920 --height 1080

# 多实例开发模式
group_vibe_workbench dev --count 2
```

## 工作流程

1. 启动应用，输入昵称登录
2. 点击 "搜索群组" 扫描局域网内的群组
3. 加入已有群组或创建新群组
4. 自动连接同组节点，开始协同编辑

## 项目结构

```
src/
├── main.rs              # CLI 入口
├── error.rs             # 错误类型
├── model.rs             # 数据模型
├── controller.rs        # 异步逻辑
├── swarm_manager.rs     # Murmur Swarm 单例
├── shared_file.rs       # 文件同步
├── instance_lock.rs     # 单实例锁
├── user_db.rs           # 本地用户数据库
├── subcmd/              # 子命令 (launch, dev)
└── gui/                 # UI 组件、页面、主题
```

## License

See LICENSE file.
