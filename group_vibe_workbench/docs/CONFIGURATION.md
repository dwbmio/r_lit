# 配置系统文档

## 概述

Group Vibe Workbench 使用环境变量和 `.env` 文件进行配置，所有硬编码的路径和设置都已抽象到配置系统中。

## 配置文件

### .env 文件

在项目根目录创建 `.env` 文件（参考 `.env.example`）：

```bash
# 复制示例配置
cp .env.example .env

# 编辑配置
vim .env
```

### 配置项

| 配置项 | 说明 | 默认值 | 示例 |
|--------|------|--------|------|
| `DATA_DIR` | 数据目录 | `./workbench_data` | `/var/lib/workbench` |
| `NETWORK_PORT` | P2P 通信端口 | `9000` | `10000` |
| `MDNS_SERVICE_NAME` | mDNS 服务名 | `_murmur._tcp` | `_myapp._tcp` |
| `SHARED_FILE_PATH` | 共享文件路径 | `../chat.ctx` | `./shared/chat.txt` |
| `WINDOW_WIDTH` | 窗口宽度 | `1280` | `1920` |
| `WINDOW_HEIGHT` | 窗口高度 | `720` | `1080` |
| `LOG_LEVEL` | 日志级别 | `info` | `debug` |
| `JSON_OUTPUT` | JSON 格式输出 | `false` | `true` |

### 向后兼容

为了向后兼容，以下环境变量仍然有效：
- `WORKBENCH_DATA_DIR` → `DATA_DIR`
- `WORKBENCH_PORT` → `NETWORK_PORT`

## 使用方式

### 1. 使用 .env 文件

```bash
# 创建 .env 文件
cat > .env <<EOF
DATA_DIR=./my_data
NETWORK_PORT=10000
SHARED_FILE_PATH=./shared.txt
LOG_LEVEL=debug
EOF

# 启动应用（自动加载 .env）
./target/release/group_vibe_workbench launch -n "Alice"
```

### 2. 使用环境变量

```bash
# 临时设置
DATA_DIR=./test_data ./target/release/group_vibe_workbench launch -n "Bob"

# 导出环境变量
export DATA_DIR=./prod_data
export LOG_LEVEL=info
./target/release/group_vibe_workbench launch -n "Charlie"
```

### 3. Dev Mode 自动配置

Dev Mode 会自动为每个实例设置独立的配置：

```bash
# 启动 2 个实例
./target/release/group_vibe_workbench dev -c 2

# 实例 0: DATA_DIR=./workbench_data_dev_0, NETWORK_PORT=9000
# 实例 1: DATA_DIR=./workbench_data_dev_1, NETWORK_PORT=9001
```

## 配置优先级

配置加载顺序（后者覆盖前者）：

1. **默认值** - 代码中的 `Config::default()`
2. **.env 文件** - 项目根目录的 `.env` 文件
3. **环境变量** - 系统环境变量
4. **命令行参数** - 某些参数可以通过命令行覆盖

示例：
```bash
# .env 文件
DATA_DIR=./data_from_file

# 环境变量覆盖 .env
DATA_DIR=./data_from_env ./target/release/group_vibe_workbench launch

# 最终使用: ./data_from_env
```

## 配置结构

### Config 结构体

```rust
pub struct Config {
    pub data_dir: PathBuf,
    pub network_port: u16,
    pub mdns_service_name: String,
    pub shared_file_path: PathBuf,
    pub window_width: u32,
    pub window_height: u32,
    pub log_level: String,
    pub json_output: bool,
}
```

### 辅助方法

```rust
// 获取用户数据库路径
config.user_db_path()  // {data_dir}/user.db

// 获取 Swarm 存储路径
config.swarm_path("user-id")  // {data_dir}/swarm/user-id

// 确保目录存在
config.ensure_dirs()
```

## 使用场景

### 场景 1: 开发环境

```bash
# .env.development
DATA_DIR=./dev_data
LOG_LEVEL=debug
SHARED_FILE_PATH=./dev_chat.txt

# 使用
ln -s .env.development .env
cargo run -- launch -n "Dev User"
```

### 场景 2: 生产环境

```bash
# .env.production
DATA_DIR=/var/lib/group_vibe_workbench
LOG_LEVEL=info
SHARED_FILE_PATH=/shared/chat.ctx
NETWORK_PORT=9000

# 使用
ln -s .env.production .env
./target/release/group_vibe_workbench launch -n "Prod User"
```

### 场景 3: 测试环境

```bash
# 每个测试使用独立配置
DATA_DIR=/tmp/test_$$ \
SHARED_FILE_PATH=/tmp/test_$$/chat.txt \
./target/release/group_vibe_workbench launch -n "Test User"
```

### 场景 4: Docker 部署

```dockerfile
# Dockerfile
ENV DATA_DIR=/app/data
ENV NETWORK_PORT=9000
ENV LOG_LEVEL=info

# 或使用 docker-compose.yml
services:
  workbench:
    environment:
      - DATA_DIR=/app/data
      - NETWORK_PORT=9000
```

## 配置验证

### 检查当前配置

```bash
# 启动时会输出配置信息
./target/release/group_vibe_workbench launch -n "Alice" 2>&1 | grep "Using data directory"
# [INFO] Using data directory: "./workbench_data"
```

### 测试配置

```rust
// 在代码中
let config = Config::load()?;
println!("Data dir: {:?}", config.data_dir);
println!("Port: {}", config.network_port);
```

## 迁移指南

### 从硬编码迁移

**之前**:
```rust
let db_path = PathBuf::from("./workbench_data/user.db");
let swarm_path = format!("./workbench_data/swarm/{}", user.id);
```

**之后**:
```rust
let config = Config::load()?;
let db_path = config.user_db_path();
let swarm_path = config.swarm_path(&user.id);
```

### 从环境变量迁移

**之前**:
```rust
let data_dir = std::env::var("WORKBENCH_DATA_DIR")
    .unwrap_or_else(|_| "./workbench_data".to_string());
```

**之后**:
```rust
let config = Config::load()?;
let data_dir = config.data_dir;
```

## 最佳实践

### 1. 不要提交 .env 到版本控制

```bash
# .gitignore
.env
.env.local
```

### 2. 提供 .env.example

```bash
# .env.example (提交到 git)
DATA_DIR=./workbench_data
NETWORK_PORT=9000
# ...

# 用户复制并修改
cp .env.example .env
```

### 3. 使用环境特定的配置

```bash
# 开发
.env.development

# 测试
.env.test

# 生产
.env.production

# 使用符号链接切换
ln -sf .env.development .env
```

### 4. 文档化所有配置项

在 `.env.example` 中添加注释说明每个配置项的用途。

### 5. 验证配置

```rust
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.network_port == 0 {
            return Err("Invalid port".into());
        }
        // ...
        Ok(())
    }
}
```

## 故障排除

### 问题 1: 配置未生效

**检查**:
- `.env` 文件是否在正确的位置（项目根目录）
- 环境变量是否正确设置
- 配置优先级是否正确

**解决**:
```bash
# 检查 .env 文件
cat .env

# 检查环境变量
env | grep DATA_DIR

# 使用绝对路径
DATA_DIR=/absolute/path/to/data ./app launch
```

### 问题 2: 数据目录权限错误

**检查**:
```bash
ls -la ./workbench_data
```

**解决**:
```bash
chmod 755 ./workbench_data
chown $USER:$USER ./workbench_data
```

### 问题 3: 端口冲突

**检查**:
```bash
lsof -i :9000
```

**解决**:
```bash
# 使用不同端口
NETWORK_PORT=10000 ./app launch
```

## 相关文件

- [src/config.rs](../src/config.rs) - 配置模块实现
- [.env.example](../.env.example) - 配置示例
- [src/subcmd/launch.rs](../src/subcmd/launch.rs) - 配置使用示例

## 总结

配置系统的优势：
1. ✅ 消除硬编码
2. ✅ 灵活的配置方式
3. ✅ 环境隔离
4. ✅ 易于测试
5. ✅ 向后兼容
6. ✅ 类型安全

所有环境相关的设置现在都通过配置系统管理，使应用更加灵活和可维护。
