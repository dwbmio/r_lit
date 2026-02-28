use murmur::Swarm;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;

/// 多人聊天室 - 消息持久化到 SQLite + 导出到 Markdown
///
/// 功能：
/// 1. 多人实时聊天
/// 2. 消息存储到 SQLite
/// 3. 持续导出到 Markdown 文件
///
/// 运行方式：
/// Terminal 1: cargo run --example chat_room --release alice
/// Terminal 2: cargo run --example chat_room --release bob <alice-addr>
/// Terminal 3: cargo run --example chat_room --release charlie <alice-addr>

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    id: String,
    sender: String,
    group_id: String,
    content: String,
    timestamp: u64,
    message_type: String, // text, system, join, leave
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <username> [peer-addr]", args[0]);
        eprintln!("\nExample:");
        eprintln!("  Terminal 1: cargo run --example chat_room alice");
        eprintln!("  Terminal 2: cargo run --example chat_room bob <alice-addr>");
        std::process::exit(1);
    }

    let username = &args[1];
    let peer_addr = args.get(2);
    let group_id = "main-chat";

    println!("\n╔════════════════════════════════════════╗");
    println!("║  多人聊天室 - {} ", username);
    println!("╚════════════════════════════════════════╝\n");

    // 创建 Swarm
    let swarm = Swarm::builder()
        .storage_path(format!("./chat_data/{}", username))
        .group_id(group_id)
        .build()
        .await?;

    swarm.start().await?;

    println!("✓ 聊天室已启动");
    println!("  用户名: {}", username);
    println!("  群组: {}", group_id);
    println!("  节点地址:\n  {}\n", swarm.node_addr().await);

    // 连接到其他节点
    if let Some(addr) = peer_addr {
        println!("🔗 正在连接到 peer...");
        match swarm.connect_peer(addr).await {
            Ok(_) => println!("✓ 连接成功!\n"),
            Err(e) => println!("⚠ 连接失败: {} (继续运行)\n", e),
        }
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // 初始化 SQLite
    let db_path = format!("./chat_data/{}/messages.db", username);
    let conn = Connection::open(&db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            sender TEXT NOT NULL,
            group_id TEXT NOT NULL,
            content TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            message_type TEXT NOT NULL
        )",
        [],
    )?;

    println!("✓ SQLite 数据库已初始化: {}", db_path);

    // 发送加入消息
    let join_msg = ChatMessage {
        id: format!("{}:{}", username, timestamp_now()),
        sender: username.clone(),
        group_id: group_id.to_string(),
        content: format!("{} 加入了聊天室", username),
        timestamp: timestamp_now(),
        message_type: "join".to_string(),
    };

    send_message(&swarm, &join_msg).await?;
    save_to_db(&conn, &join_msg)?;

    println!("✓ 已加入聊天室\n");

    // 启动后台任务
    let swarm_clone = swarm.clone();
    let username_clone = username.clone();
    let group_id_clone = group_id.to_string();

    tokio::spawn(async move {
        if let Err(e) = message_receiver_task(
            swarm_clone,
            username_clone,
            group_id_clone,
            db_path,
        ).await {
            eprintln!("消息接收任务错误: {}", e);
        }
    });

    // 启动 Markdown 导出任务
    let username_clone = username.clone();
    tokio::spawn(async move {
        if let Err(e) = markdown_export_task(username_clone).await {
            eprintln!("Markdown 导出任务错误: {}", e);
        }
    });

    // 模拟发送一些消息
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💬 开始聊天...\n");

    let messages = match username.as_str() {
        "alice" => vec![
            "大家好！",
            "今天天气不错",
            "有人在吗？",
        ],
        "bob" => vec![
            "嗨 Alice！",
            "我在这里",
            "今天要讨论什么？",
        ],
        "charlie" => vec![
            "大家好！",
            "我也来了",
            "让我们开始吧",
        ],
        _ => vec!["Hello everyone!"],
    };

    for (i, content) in messages.iter().enumerate() {
        tokio::time::sleep(Duration::from_secs(2 + i as u64)).await;

        let msg = ChatMessage {
            id: format!("{}:{}:{}", username, timestamp_now(), i),
            sender: username.clone(),
            group_id: group_id.to_string(),
            content: content.to_string(),
            timestamp: timestamp_now(),
            message_type: "text".to_string(),
        };

        send_message(&swarm, &msg).await?;
        save_to_db(&conn, &msg)?;

        println!("[{}] {}: {}", format_timestamp(msg.timestamp), username, content);
    }

    // 保持运行
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✨ 聊天室运行中...");
    println!("💡 消息会自动同步到其他节点");
    println!("📝 消息持续导出到 chat_export_{}.md", username);
    println!("🗄️  消息存储在 SQLite: {}", db_path);
    println!("⏸️  按 Ctrl+C 退出\n");

    tokio::signal::ctrl_c().await?;

    // 发送离开消息
    let leave_msg = ChatMessage {
        id: format!("{}:leave:{}", username, timestamp_now()),
        sender: username.clone(),
        group_id: group_id.to_string(),
        content: format!("{} 离开了聊天室", username),
        timestamp: timestamp_now(),
        message_type: "leave".to_string(),
    };

    send_message(&swarm, &leave_msg).await?;
    save_to_db(&conn, &leave_msg)?;

    println!("\n👋 正在退出...");
    swarm.shutdown().await?;
    println!("✓ 已安全退出");

    Ok(())
}

async fn send_message(swarm: &Swarm, msg: &ChatMessage) -> anyhow::Result<()> {
    let key = format!("msg:{}", msg.id);
    let value = serde_json::to_vec(msg)?;
    swarm.put(&key, &value).await?;
    Ok(())
}

fn save_to_db(conn: &Connection, msg: &ChatMessage) -> anyhow::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO messages (id, sender, group_id, content, timestamp, message_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![msg.id, msg.sender, msg.group_id, msg.content, msg.timestamp, msg.message_type],
    )?;
    Ok(())
}

async fn message_receiver_task(
    swarm: Swarm,
    username: String,
    group_id: String,
    db_path: String,
) -> anyhow::Result<()> {
    let conn = Connection::open(&db_path)?;
    let mut last_check = timestamp_now();

    let mut interval = interval(Duration::from_secs(2));

    loop {
        interval.tick().await;

        // 简单轮询：检查新消息
        // 注意：这是简化实现，生产环境应该用事件驱动
        let current_time = timestamp_now();

        // 从 KV 读取可能的新消息
        for i in 0..100 {
            let key = format!("msg:{}:{}:{}", username, current_time - 60, i);
            if let Some(value) = swarm.get(&key).await? {
                if let Ok(msg) = serde_json::from_slice::<ChatMessage>(&value) {
                    if msg.timestamp > last_check && msg.sender != username {
                        save_to_db(&conn, &msg)?;
                        println!("[{}] {}: {}",
                            format_timestamp(msg.timestamp),
                            msg.sender,
                            msg.content
                        );
                    }
                }
            }
        }

        last_check = current_time;
    }
}

async fn markdown_export_task(username: String) -> anyhow::Result<()> {
    let db_path = format!("./chat_data/{}/messages.db", username);
    let md_path = format!("./chat_export_{}.md", username);

    let mut interval = interval(Duration::from_secs(5));

    loop {
        interval.tick().await;

        let conn = Connection::open(&db_path)?;

        let mut stmt = conn.prepare(
            "SELECT sender, content, timestamp, message_type
             FROM messages
             ORDER BY timestamp ASC"
        )?;

        let messages: Vec<_> = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?.collect::<Result<Vec<_>, _>>()?;

        // 写入 Markdown
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&md_path)?;

        writeln!(file, "# 聊天记录 - {}\n", username)?;
        writeln!(file, "导出时间: {}\n", format_timestamp(timestamp_now()))?;
        writeln!(file, "总消息数: {}\n", messages.len())?;
        writeln!(file, "---\n")?;

        for (sender, content, timestamp, msg_type) in messages {
            let time_str = format_timestamp(timestamp as u64);

            match msg_type.as_str() {
                "join" | "leave" => {
                    writeln!(file, "**[{}]** _{}_\n", time_str, content)?;
                }
                "text" => {
                    writeln!(file, "**[{}] {}:** {}\n", time_str, sender, content)?;
                }
                _ => {
                    writeln!(file, "[{}] {}: {}\n", time_str, sender, content)?;
                }
            }
        }

        writeln!(file, "\n---\n")?;
        writeln!(file, "_自动生成于 {}_", format_timestamp(timestamp_now()))?;
    }
}

fn timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn format_timestamp(ts: u64) -> String {
    use chrono::{DateTime, Local, TimeZone};
    let dt = Local.timestamp_opt(ts as i64, 0).unwrap();
    dt.format("%H:%M:%S").to_string()
}
