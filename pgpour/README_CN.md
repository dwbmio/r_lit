# pgpour

Postgres CDC → Kafka 实时数据管道。基于 [supabase/etl](https://github.com/supabase/etl) 构建，独立于业务服务运行，纯运维侧部署。

## 架构

```
┌────────────┐  logical replication  ┌──────────┐  produce  ┌───────┐
│ PostgreSQL │ ────────────────────→ │ pgpour   │ ────────→ │ Kafka │
│ (WAL)      │                       │          │           │       │
└────────────┘                       └──────────┘           └───────┘
```

- PG 端零侵入（仅需 `wal_level=logical` + `CREATE PUBLICATION`）
- 每张表对应一个 Kafka topic: `{prefix}.{schema}.{table}`
- 消息格式: JSON envelope (`op` / `before` / `after`)

## 前置条件

- Rust >= 1.88.0, cmake
- PostgreSQL 14+ 且 `wal_level = logical`
- 连接用户需 `REPLICATION` 权限

## 快速开始

### 1. PG 端配置

```sql
SHOW wal_level;  -- 需要输出 logical
CREATE PUBLICATION cdc_publication FOR TABLE feature_config;
ALTER TABLE feature_config REPLICA IDENTITY FULL;
ALTER ROLE repl_user REPLICATION;
```

详见 `sql/setup_publication.sql`。

### 2. 应用配置

配置优先级：**CLI 参数 > 环境变量 > YAML 配置文件 > 默认值**

```bash
cp config.example.yml config.yml
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

完整参数见 `config.example.yml` 和英文 README。支持 Postgres SSH 隧道 (密码/私钥)、Kafka 多种安全协议 (plaintext/ssl/sasl_plaintext/sasl_ssl)。

## 可选功能

- `--features metric`: 启用 OpenTelemetry OTLP 指标导出

## License

MIT
