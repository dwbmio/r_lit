# pgpour

Postgres CDC → Kafka 实时数据管道。基于 [supabase/etl](https://github.com/supabase/etl) 构建，独立于业务服务运行，纯运维侧部署。

## 架构

```
┌────────────┐  logical replication  ┌──────────┐  produce  ┌───────────────┐
│ PostgreSQL │ ────────────────────→ │ pgpour   │ ────────→ │ Topic-A (tbl1)│
│ (WAL)      │                       │          │ ────────→ │ Topic-B (tbl2)│
└────────────┘                       └──────────┘           └───────────────┘
```

- PG 端零侵入（仅需 `wal_level=logical` + `CREATE PUBLICATION`）
- 每张表对应一个 Kafka topic: `{prefix}.{schema}.{table}`
- 消息格式: JSON envelope (`op` / `before` / `after`)
- 多 topic 扇出：一个 replication slot，通过 `kafka_destinations` 按表路由事件到不同 topic

## 前置条件

- Rust >= 1.90.0, cmake
- PostgreSQL 14+ 且 `wal_level = logical`
- 连接用户需 `REPLICATION` 权限

## 快速开始

### 1. PG 端配置

```sql
-- 1. 确认 wal_level（需 superuser，修改后重启 PG）
SHOW wal_level;  -- 期望输出: logical
-- ALTER SYSTEM SET wal_level = logical;  -- 然后重启 PG

-- 2. 创建 publication
CREATE PUBLICATION cdc_publication FOR TABLE feature_config;
-- 后续追加表: ALTER PUBLICATION cdc_publication ADD TABLE solution;

-- 3. (可选) UPDATE/DELETE 事件包含完整旧行
ALTER TABLE feature_config REPLICA IDENTITY FULL;

-- 4. 确认连接用户有 REPLICATION 权限
ALTER ROLE repl_user REPLICATION;
```

### 2. 应用配置

配置优先级：**CLI 参数 > 环境变量 > YAML 配置文件 > 默认值**

```bash
cp examples/config.example.yml config.yml
# 编辑填入实际值，敏感字段建议走环境变量
```

### 3. 构建与运行

```bash
# 开发
cargo run -- --config config.yml

# Docker
docker compose up -d
```

## 消息格式

```json
{
  "op": "insert | update | delete | sync",
  "table": "public.solution",
  "before": null,
  "after": { "id": 1, "solution_id": "abc" }
}
```

- `op=sync`: 初始全量同步阶段
- Message Key: 主键列值

## 配置参考

完整参数见 `examples/config.example.yml` 和英文 README。支持 Postgres SSH 隧道 (密码/私钥)、Kafka 多种安全协议 (plaintext/ssl/sasl_plaintext/sasl_ssl)。

### 多 topic 扇出（按表过滤路由）

在 YAML 配置中添加 `kafka_destinations` 数组，将 CDC 事件按表路由到同一 Kafka 集群的不同 topic 前缀。只用一个 replication slot，WAL 只解码一次，匹配的目标并发写入。

每个目标支持可选的 `tables` 白名单（格式 `schema.table`，如 `public.orders`）：设置后仅转发匹配的表；不设或为空则转发所有表。若某表不在任何目标的 `tables` 列表中，对应事件会被静默丢弃，管道继续正常运行。额外目标自动继承主 `kafka` 的 `brokers` 和认证信息——通常只需填 `name`、`topic_prefix` 和 `tables`。

```yaml
kafka:
  brokers: 10.9.169.2:9092,10.9.131.68:9092
  topic_prefix: cdc
  tables:                        # 主目标只收 table1
    - public.table1

kafka_destinations:
  - name: backup
    topic_prefix: cdc-backup     # 不设 tables → 收全部表
  - name: analytics
    topic_prefix: analytics
    tables:                      # analytics 只收 table2
      - public.table2
```

主 `kafka` 节（也可通过 CLI / 环境变量配置）始终作为 `"default"` 目标；`kafka_destinations` 和 `tables` 仅支持 YAML 配置。如需连接不同 Kafka 集群，在对应条目中覆盖 `brokers` / 认证字段即可。

## 可选功能

- `--features metric`: 启用 OpenTelemetry OTLP 指标导出

## License

MIT
