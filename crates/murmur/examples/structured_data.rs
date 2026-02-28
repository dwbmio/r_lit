use murmur::Swarm;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// 结构化数据测试 - KV 存储 + SQL 查询
///
/// 场景：
/// 1. 在 KV 中存储结构化数据（JSON）
/// 2. 将数据同步到 SQLite
/// 3. 使用 SQL 查询和分析
///
/// 运行方式：
/// cargo run --example structured_data --release

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Message {
    id: u64,
    sender: String,
    group_id: String,
    content: String,
    timestamp: u64,
    message_type: String, // text, image, file
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct User {
    id: u64,
    name: String,
    email: String,
    age: u32,
    groups: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("\n╔════════════════════════════════════════╗");
    println!("║  结构化数据测试 - KV + SQL             ║");
    println!("╚════════════════════════════════════════╝\n");

    // 创建 Swarm
    let swarm = Swarm::builder()
        .storage_path("./structured_data/node1")
        .group_id("structured-test")
        .build()
        .await?;

    swarm.start().await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("✓ Swarm 已启动");
    println!("  节点 ID: {}", &swarm.node_id().await[..16]);
    println!();

    // ========================================
    // 第 1 步：在 KV 中存储结构化数据
    // ========================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📝 步骤 1: 在 KV 中存储结构化数据");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // 创建用户
    let users = vec![
        User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 25,
            groups: vec!["team-a".to_string(), "all".to_string()],
        },
        User {
            id: 2,
            name: "Bob".to_string(),
            email: "bob@example.com".to_string(),
            age: 30,
            groups: vec!["team-b".to_string(), "all".to_string()],
        },
        User {
            id: 3,
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            age: 28,
            groups: vec!["team-a".to_string(), "team-b".to_string(), "all".to_string()],
        },
    ];

    for user in &users {
        let key = format!("user:{}", user.id);
        let value = serde_json::to_vec(&user)?;
        swarm.put(&key, &value).await?;
        println!("✓ 存储用户: {} ({})", user.name, user.email);
    }

    println!();

    // 创建消息
    let messages = vec![
        Message {
            id: 1,
            sender: "Alice".to_string(),
            group_id: "team-a".to_string(),
            content: "Hello team A!".to_string(),
            timestamp: 1700000000,
            message_type: "text".to_string(),
        },
        Message {
            id: 2,
            sender: "Bob".to_string(),
            group_id: "team-b".to_string(),
            content: "Hi team B!".to_string(),
            timestamp: 1700000100,
            message_type: "text".to_string(),
        },
        Message {
            id: 3,
            sender: "Charlie".to_string(),
            group_id: "all".to_string(),
            content: "Hello everyone!".to_string(),
            timestamp: 1700000200,
            message_type: "text".to_string(),
        },
        Message {
            id: 4,
            sender: "Alice".to_string(),
            group_id: "team-a".to_string(),
            content: "Check this image".to_string(),
            timestamp: 1700000300,
            message_type: "image".to_string(),
        },
        Message {
            id: 5,
            sender: "Bob".to_string(),
            group_id: "all".to_string(),
            content: "Important file attached".to_string(),
            timestamp: 1700000400,
            message_type: "file".to_string(),
        },
    ];

    for msg in &messages {
        let key = format!("message:{}", msg.id);
        let value = serde_json::to_vec(&msg)?;
        swarm.put(&key, &value).await?;
        println!("✓ 存储消息: {} -> {} ({})",
            msg.sender, msg.group_id, msg.content);
    }

    println!();

    // ========================================
    // 第 2 步：从 KV 读取并同步到 SQLite
    // ========================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔄 步骤 2: 同步到 SQLite");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    use rusqlite::{Connection, params};

    let conn = Connection::open("./structured_data/analytics.db")?;

    // 创建表
    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT NOT NULL,
            age INTEGER NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY,
            sender TEXT NOT NULL,
            group_id TEXT NOT NULL,
            content TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            message_type TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS user_groups (
            user_id INTEGER NOT NULL,
            group_id TEXT NOT NULL,
            PRIMARY KEY (user_id, group_id)
        )",
        [],
    )?;

    println!("✓ 创建 SQLite 表");

    // 同步用户数据
    for user in &users {
        conn.execute(
            "INSERT OR REPLACE INTO users (id, name, email, age) VALUES (?1, ?2, ?3, ?4)",
            params![user.id, user.name, user.email, user.age],
        )?;

        for group in &user.groups {
            conn.execute(
                "INSERT OR REPLACE INTO user_groups (user_id, group_id) VALUES (?1, ?2)",
                params![user.id, group],
            )?;
        }
    }

    println!("✓ 同步 {} 个用户", users.len());

    // 同步消息数据
    for msg in &messages {
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, sender, group_id, content, timestamp, message_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![msg.id, msg.sender, msg.group_id, msg.content, msg.timestamp, msg.message_type],
        )?;
    }

    println!("✓ 同步 {} 条消息", messages.len());
    println!();

    // ========================================
    // 第 3 步：SQL 查询和分析
    // ========================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔍 步骤 3: SQL 查询和分析");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // 查询 1: 每个用户的消息数量
    println!("📊 查询 1: 每个用户的消息数量");
    let mut stmt = conn.prepare(
        "SELECT sender, COUNT(*) as msg_count
         FROM messages
         GROUP BY sender
         ORDER BY msg_count DESC"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
        ))
    })?;

    for row in rows {
        let (sender, count) = row?;
        println!("  {} 发送了 {} 条消息", sender, count);
    }
    println!();

    // 查询 2: 每个群组的消息数量
    println!("📊 查询 2: 每个群组的消息数量");
    let mut stmt = conn.prepare(
        "SELECT group_id, COUNT(*) as msg_count
         FROM messages
         GROUP BY group_id
         ORDER BY msg_count DESC"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
        ))
    })?;

    for row in rows {
        let (group_id, count) = row?;
        println!("  {} 群组有 {} 条消息", group_id, count);
    }
    println!();

    // 查询 3: 按消息类型统计
    println!("📊 查询 3: 按消息类型统计");
    let mut stmt = conn.prepare(
        "SELECT message_type, COUNT(*) as count
         FROM messages
         GROUP BY message_type"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
        ))
    })?;

    for row in rows {
        let (msg_type, count) = row?;
        println!("  {} 类型: {} 条", msg_type, count);
    }
    println!();

    // 查询 4: 用户参与的群组
    println!("📊 查询 4: 用户参与的群组");
    let mut stmt = conn.prepare(
        "SELECT u.name, GROUP_CONCAT(ug.group_id, ', ') as groups
         FROM users u
         JOIN user_groups ug ON u.id = ug.user_id
         GROUP BY u.id, u.name"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
        ))
    })?;

    for row in rows {
        let (name, groups) = row?;
        println!("  {} 参与群组: {}", name, groups);
    }
    println!();

    // 查询 5: 复杂 JOIN 查询 - 每个群组的活跃用户
    println!("📊 查询 5: 每个群组的活跃用户");
    let mut stmt = conn.prepare(
        "SELECT m.group_id, m.sender, COUNT(*) as msg_count
         FROM messages m
         GROUP BY m.group_id, m.sender
         ORDER BY m.group_id, msg_count DESC"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut current_group = String::new();
    for row in rows {
        let (group_id, sender, count) = row?;
        if group_id != current_group {
            if !current_group.is_empty() {
                println!();
            }
            println!("  群组 {}:", group_id);
            current_group = group_id;
        }
        println!("    - {}: {} 条消息", sender, count);
    }
    println!();

    // ========================================
    // 第 4 步：验证 KV 和 SQL 数据一致性
    // ========================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ 步骤 4: 验证数据一致性");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // 从 KV 读取
    let mut kv_users = Vec::new();
    for i in 1..=3 {
        let key = format!("user:{}", i);
        if let Some(value) = swarm.get(&key).await? {
            let user: User = serde_json::from_slice(&value)?;
            kv_users.push(user);
        }
    }

    // 从 SQL 读取
    let mut stmt = conn.prepare("SELECT id, name, email, age FROM users ORDER BY id")?;
    let sql_users: Vec<_> = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?.collect::<Result<Vec<_>, _>>()?;

    println!("✓ KV 中有 {} 个用户", kv_users.len());
    println!("✓ SQL 中有 {} 个用户", sql_users.len());

    if kv_users.len() == sql_users.len() {
        println!("✓ 数据一致性验证通过！");
    } else {
        println!("⚠ 数据不一致！");
    }

    println!();

    // ========================================
    // 总结
    // ========================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🎉 测试完成");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("💡 关键点:");
    println!("  1. KV 存储结构化数据（JSON 序列化）");
    println!("  2. 同步到 SQLite 用于复杂查询");
    println!("  3. SQL 支持 JOIN、GROUP BY、聚合函数");
    println!("  4. KV 负责分布式同步，SQL 负责本地分析");
    println!();

    println!("📁 数据位置:");
    println!("  - KV 数据: ./structured_data/node1/murmur.redb");
    println!("  - SQL 数据: ./structured_data/analytics.db");
    println!();

    swarm.shutdown().await?;

    Ok(())
}
