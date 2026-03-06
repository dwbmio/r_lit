-- ============================================================
-- Postgres CDC 前置配置脚本
-- 目标: lgh_blockblast_client @ starlink-dev-uc-internal.hs99.vip
-- ============================================================

-- 1. 确认 wal_level（需要 superuser，修改后需重启 PG）
--    SHOW wal_level;  -- 期望输出: logical
--    如果不是 logical，运维需执行:
--    ALTER SYSTEM SET wal_level = logical;
--    然后重启 Postgres。

-- 2. 创建 publication（监听 feature_config 表）
CREATE PUBLICATION cdc_publication FOR TABLE feature_config;

-- 后续如需追加更多表:
-- ALTER PUBLICATION cdc_publication ADD TABLE solution, feature_config_history;

-- 3. 验证 publication 创建成功
SELECT * FROM pg_publication;
SELECT * FROM pg_publication_tables WHERE pubname = 'cdc_publication';

-- 4. (可选) 设置 REPLICA IDENTITY 为 FULL，让 UPDATE/DELETE 事件包含完整旧行数据
-- 默认 DEFAULT 只在旧行中包含主键列
ALTER TABLE feature_config REPLICA IDENTITY FULL;

-- 5. 确认 replication slot 权限
-- 连接用户需要 REPLICATION 权限或 superuser
-- ALTER ROLE solutionhub_dev REPLICATION;
