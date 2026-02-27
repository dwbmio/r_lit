# 共享 KV 存储示例

## 🎯 场景说明

这个示例演示了多人协作维护一个共享的 KV 配置存储：

- **Alice**：设置项目基础配置（名称、版本、数据库）
- **Bob**：更新版本、添加 API 配置
- **Charlie**：修改数据库端口、添加缓存配置

所有修改会自动同步到其他节点，每个节点都有完整的 SQLite 副本。

## 🚀 运行步骤

### 1. 启动 Alice（第一个节点）

```bash
cd murmur
cargo run --example shared_kv --release alice
```

**输出：**
```
╔════════════════════════════════════════╗
║  共享 KV 存储 - 多人协作示例           ║
╚════════════════════════════════════════╝

👤 用户: alice
🆔 节点 ID: 7a3f2e1b...
📍 节点地址:
   NodeAddr { node_id: 7a3f2e1b..., relay_url: ... }

💡 提示: 这是第一个节点，等待其他人连接...

👑 角色: LEADER (协调者)
🌐 已连接节点: 0

──────────────────────────────────────────────────
📝 开始协作维护配置...

Alice: 设置项目名称
Alice: 设置项目版本
Alice: 设置数据库配置
```

**复制 Alice 的节点地址**（用于其他节点连接）

### 2. 启动 Bob（连接到 Alice）

```bash
# 新终端
cargo run --example shared_kv --release bob "<alice-node-addr>"
```

**输出：**
```
👤 用户: bob
🔗 正在连接到 peer...
✓ 连接成功!

👥 角色: FOLLOWER (跟随者)
   Leader: 7a3f2e1b...
🌐 已连接节点: 1
   - 7a3f2e1b...

Bob: 更新项目版本
Bob: 添加 API 配置
```

### 3. 启动 Charlie（连接到 Alice）

```bash
# 新终端
cargo run --example shared_kv --release charlie "<alice-node-addr>"
```

**输出：**
```
👤 用户: charlie
🔗 正在连接到 peer...
✓ 连接成功!

👥 角色: FOLLOWER (跟随者)
🌐 已连接节点: 1

Charlie: 更新数据库端口
Charlie: 添加缓存配置
```

## 📊 查看同步结果

等待几秒后，所有节点都会显示：

```
══════════════════════════════════════════════════
📊 当前共享配置 (本地 SQLite 副本):

  project:name = MyAwesomeProject
  project:version = 1.1.0
  db:host = localhost
  db:port = 3306
  api:endpoint = https://api.example.com
  api:timeout = 30
  cache:enabled = true
  cache:ttl = 3600
  user:alice = alice
  user:bob = bob
  user:charlie = charlie

══════════════════════════════════════════════════

🔍 验证数据一致性:
  ✓ 项目版本: 1.1.0
    (Bob 的更新覆盖了 Alice 的设置 - CRDT 自动解决冲突)
  ✓ 数据库端口: 3306
    (Charlie 的更新覆盖了 Alice 的设置)

📈 统计信息:
  - 连接节点数: 2
  - 本地存储: ./data/shared_kv/alice/murmur.db
  - 数据完整性: ✓ CRDT 保证最终一致性
  - 因果顺序: ✓ 向量时钟追踪
```

## 🔄 并发修改演示

示例还演示了并发修改的处理：

```
🔄 演示并发修改处理:

Alice: 同时修改 counter (设置为 100)
Bob: 同时修改 counter (设置为 200)

最终 counter 值: 200
(CRDT 使用 Last-Write-Wins 策略自动解决冲突)
```

## 🔍 验证本地存储

每个节点都有独立的 SQLite 数据库：

```bash
# 查看 Alice 的数据库
sqlite3 ./data/shared_kv/alice/murmur.db

sqlite> SELECT * FROM kv_store;
project:name|MyAwesomeProject|1|1234567890
project:version|1.1.0|1|1234567891
db:host|localhost|1|1234567892
db:port|3306|1|1234567893
...

sqlite> .quit
```

**所有节点的数据库内容完全一致！**

## 💡 关键特性演示

### 1. 自动同步

```
Alice 写入 → 自动广播 → Bob 和 Charlie 收到 → 写入本地 SQLite
```

### 2. 冲突解决

```
Alice: project:version = "1.0.0"  (时间 T1)
Bob:   project:version = "1.1.0"  (时间 T2)

最终结果: "1.1.0" (Last-Write-Wins)
```

### 3. 因果追踪

```
每条消息携带向量时钟：
Alice: VC{alice:1, bob:0, charlie:0}
Bob:   VC{alice:1, bob:1, charlie:0}
Charlie: VC{alice:1, bob:1, charlie:1}

保证因果顺序正确
```

### 4. 离线可用

```bash
# 断开 Bob 的网络
# Bob 仍然可以读写本地数据

# 重新连接后
# 自动同步所有变更
```

## 🧪 测试场景

### 场景 1：顺序修改

```bash
# Alice 先修改
alice> put("key", "v1")

# 等待同步
sleep 2s

# Bob 再修改
bob> put("key", "v2")

# 结果：所有节点都是 "v2"
```

### 场景 2：并发修改

```bash
# Alice 和 Bob 同时修改
alice> put("key", "v1")  # 同一时刻
bob>   put("key", "v2")  # 同一时刻

# CRDT 自动解决冲突
# 结果：所有节点收敛到同一个值（v1 或 v2）
```

### 场景 3：网络分区

```bash
# 断开 Charlie 的网络
# Alice 和 Bob 继续协作

alice> put("x", "1")
bob>   put("y", "2")

# Charlie 离线修改
charlie> put("z", "3")

# 重新连接后
# 所有节点都有 x=1, y=2, z=3
```

## 📁 数据存储位置

```
./data/shared_kv/
├── alice/
│   └── murmur.db       # Alice 的本地副本
├── bob/
│   └── murmur.db       # Bob 的本地副本
└── charlie/
    └── murmur.db       # Charlie 的本地副本
```

**每个数据库都是完整的副本，内容完全一致！**

## 🎓 学习要点

1. **去中心化**：无需中心服务器，每个节点平等
2. **最终一致性**：所有节点最终达到相同状态
3. **CRDT 魔法**：自动解决冲突，无需手动处理
4. **向量时钟**：追踪因果关系，保证顺序
5. **本地优先**：每个节点都有完整副本，离线可用

## 🔧 自定义使用

```rust
use murmur::Swarm;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 创建 Swarm
    let swarm = Swarm::builder()
        .storage_path("./my_data")
        .group_id("my-group")
        .build()
        .await?;

    swarm.start().await?;

    // 写入数据
    swarm.put("config:theme", b"dark").await?;
    swarm.put("config:lang", b"zh-CN").await?;

    // 读取数据
    if let Some(theme) = swarm.get("config:theme").await? {
        println!("Theme: {}", String::from_utf8_lossy(&theme));
    }

    // 连接其他节点
    swarm.connect_peer("<peer-addr>").await?;

    // 数据自动同步！

    Ok(())
}
```

## ⚠️ 注意事项

1. **节点地址格式**：目前使用 Debug 格式，不易解析（待改进）
2. **连接限制**：需要手动交换地址（未来可添加自动发现）
3. **规模限制**：适合小规模协作（<50 节点）
4. **冲突策略**：当前使用 Last-Write-Wins（可自定义）

## 🚀 下一步

- 添加 mDNS 自动发现（局域网）
- 实现持久化连接列表
- 添加数据加密
- 支持更复杂的数据类型（JSON、结构化数据）
