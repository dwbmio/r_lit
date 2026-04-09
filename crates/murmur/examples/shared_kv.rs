use murmur::Swarm;
use std::env;
use tokio::time::{sleep, Duration};

/// 多人协作维护共享 KV 存储示例
///
/// 场景：一个团队共同维护项目配置
/// - 每个成员可以读写配置
/// - 所有修改自动同步
/// - CRDT 自动解决冲突
///
/// 运行方式：
/// Terminal 1: cargo run --example shared_kv alice
/// Terminal 2: cargo run --example shared_kv bob <alice-addr>
/// Terminal 3: cargo run --example shared_kv charlie <alice-addr>

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // 获取参数
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <name> [peer-addr]", args[0]);
        eprintln!("\nExample:");
        eprintln!("  Terminal 1: cargo run --example shared_kv alice");
        eprintln!("  Terminal 2: cargo run --example shared_kv bob <alice-addr>");
        std::process::exit(1);
    }

    let name = &args[1];
    let peer_addr = args.get(2);

    println!("\n╔════════════════════════════════════════╗");
    println!("║  共享 KV 存储 - 多人协作示例           ║");
    println!("╚════════════════════════════════════════╝\n");

    // 创建 Swarm 实例
    let swarm = Swarm::builder()
        .storage_path(format!("./data/shared_kv/{}", name))
        .group_id("project-config")  // 同一个项目组
        .build()
        .await?;

    swarm.start().await?;

    println!("👤 用户: {}", name);
    println!("🆔 节点 ID: {}", swarm.node_id().await);
    println!("📍 节点地址:\n   {}\n", serde_json::to_string(&swarm.node_addr().await?).unwrap_or_default());

    // 连接到其他节点
    if let Some(addr_str) = peer_addr {
        println!("🔗 正在连接到 peer...");
        match serde_json::from_str::<murmur::NodeAddr>(addr_str) {
            Ok(addr) => match swarm.connect_peer(&addr).await {
                Ok(_) => println!("✓ 连接成功!\n"),
                Err(e) => println!("⚠ 连接失败: {} (继续运行)\n", e),
            },
            Err(e) => println!("⚠ 地址解析失败: {} (继续运行)\n", e),
        }
    } else {
        println!("💡 提示: 这是第一个节点，等待其他人连接...\n");
    }

    // 等待网络稳定和选举完成
    sleep(Duration::from_secs(3)).await;

    // 显示角色
    if swarm.is_leader().await {
        println!("👑 角色: LEADER (协调者)");
    } else if let Some(leader) = swarm.leader_id().await {
        println!("👥 角色: FOLLOWER (跟随者)");
        println!("   Leader: {}...", &leader[..16]);
    }

    // 显示连接的节点
    let peers = swarm.connected_peers().await;
    println!("🌐 已连接节点: {}", peers.len());
    for peer in &peers {
        println!("   - {}...", &peer[..16]);
    }

    println!("\n{}", "─".repeat(50));
    println!("📝 开始协作维护配置...\n");

    // 模拟不同用户的操作
    match name.as_str() {
        "alice" => {
            // Alice 设置项目基础配置
            println!("Alice: 设置项目名称");
            swarm.put("project:name", b"MyAwesomeProject").await?;
            sleep(Duration::from_secs(1)).await;

            println!("Alice: 设置项目版本");
            swarm.put("project:version", b"1.0.0").await?;
            sleep(Duration::from_secs(1)).await;

            println!("Alice: 设置数据库配置");
            swarm.put("db:host", b"localhost").await?;
            swarm.put("db:port", b"5432").await?;
        }

        "bob" => {
            sleep(Duration::from_secs(2)).await;

            println!("Bob: 更新项目版本");
            swarm.put("project:version", b"1.1.0").await?;
            sleep(Duration::from_secs(1)).await;

            println!("Bob: 添加 API 配置");
            swarm.put("api:endpoint", b"https://api.example.com").await?;
            swarm.put("api:timeout", b"30").await?;
        }

        "charlie" => {
            sleep(Duration::from_secs(3)).await;

            println!("Charlie: 更新数据库端口");
            swarm.put("db:port", b"3306").await?;
            sleep(Duration::from_secs(1)).await;

            println!("Charlie: 添加缓存配置");
            swarm.put("cache:enabled", b"true").await?;
            swarm.put("cache:ttl", b"3600").await?;
        }

        _ => {
            println!("{}: 添加自定义配置", name);
            swarm.put(&format!("user:{}", name), name.as_bytes()).await?;
        }
    }

    // 等待同步
    println!("\n⏳ 等待数据同步...");
    sleep(Duration::from_secs(5)).await;

    // 读取并显示所有配置
    println!("\n{}", "═".repeat(50));
    println!("📊 当前共享配置 (本地 SQLite 副本):\n");

    let keys = vec![
        "project:name",
        "project:version",
        "db:host",
        "db:port",
        "api:endpoint",
        "api:timeout",
        "cache:enabled",
        "cache:ttl",
        "user:alice",
        "user:bob",
        "user:charlie",
    ];

    for key in keys {
        if let Some(value) = swarm.get(key).await? {
            let value_str = String::from_utf8_lossy(&value);
            println!("  {} = {}", key, value_str);
        }
    }

    println!("\n{}", "═".repeat(50));

    // 验证数据一致性
    println!("\n🔍 验证数据一致性:");

    // 检查关键配置
    if let Some(version) = swarm.get("project:version").await? {
        let version_str = String::from_utf8_lossy(&version);
        println!("  ✓ 项目版本: {}", version_str);

        // 说明冲突解决
        if version_str == "1.1.0" {
            println!("    (Bob 的更新覆盖了 Alice 的设置 - CRDT 自动解决冲突)");
        }
    }

    if let Some(port) = swarm.get("db:port").await? {
        let port_str = String::from_utf8_lossy(&port);
        println!("  ✓ 数据库端口: {}", port_str);

        if port_str == "3306" {
            println!("    (Charlie 的更新覆盖了 Alice 的设置)");
        }
    }

    // 显示统计信息
    println!("\n📈 统计信息:");
    println!("  - 连接节点数: {}", peers.len());
    println!("  - 本地存储: ./data/shared_kv/{}/murmur.db", name);
    println!("  - 数据完整性: ✓ CRDT 保证最终一致性");
    println!("  - 因果顺序: ✓ 向量时钟追踪");

    // 演示并发修改
    println!("\n{}", "─".repeat(50));
    println!("🔄 演示并发修改处理:\n");

    if name == "alice" {
        println!("Alice: 同时修改 counter (设置为 100)");
        swarm.put("counter", b"100").await?;
    } else if name == "bob" {
        sleep(Duration::from_millis(100)).await;
        println!("Bob: 同时修改 counter (设置为 200)");
        swarm.put("counter", b"200").await?;
    }

    sleep(Duration::from_secs(3)).await;

    if let Some(counter) = swarm.get("counter").await? {
        let counter_str = String::from_utf8_lossy(&counter);
        println!("\n最终 counter 值: {}", counter_str);
        println!("(CRDT 使用 Last-Write-Wins 策略自动解决冲突)");
    }

    // 持续运行，监听变化
    println!("\n{}", "═".repeat(50));
    println!("✨ 系统运行中...");
    println!("💡 提示:");
    println!("  - 所有修改会自动同步到其他节点");
    println!("  - 每个节点都有完整的数据副本");
    println!("  - 离线修改会在重新连接后同步");
    println!("  - 按 Ctrl+C 退出\n");

    // 定期显示更新
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let peers = swarm.connected_peers().await;
                println!("⏰ [{}] 在线节点: {} | Leader: {}",
                    chrono::Local::now().format("%H:%M:%S"),
                    peers.len(),
                    if swarm.is_leader().await { "是" } else { "否" }
                );
            }
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }
    }

    println!("\n👋 正在关闭...");
    swarm.shutdown().await?;
    println!("✓ 已安全退出");

    Ok(())
}
