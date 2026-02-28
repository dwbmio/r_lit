use murmur::Swarm;
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <nickname> [discover|advertise]", args[0]);
        eprintln!("\nExamples:");
        eprintln!("  # 节点 A: 广播自己");
        eprintln!("  cargo run --example local_discovery Alice advertise");
        eprintln!();
        eprintln!("  # 节点 B: 发现并连接");
        eprintln!("  cargo run --example local_discovery Bob discover");
        std::process::exit(1);
    }

    let nickname = &args[1];
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("advertise");

    println!("🚀 Starting node: {}", nickname);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Create swarm
    let swarm = Swarm::builder()
        .storage_path(format!("./data/{}", nickname))
        .group_id("cc")  // 群组 "cc"
        .build()
        .await?;

    swarm.start().await?;

    println!("\n🆔 Node ID: {}", swarm.node_id().await);
    println!("🏠 Group: cc");

    match mode {
        "advertise" => {
            // 广播模式
            println!("\n📡 Broadcasting on local network...");

            let _discovery = swarm.advertise_local(nickname).await?;
            println!("✓ Now advertising as '{}'", nickname);

            println!("\n💡 Other nodes can now discover you!");
            println!("   Run: cargo run --example local_discovery <name> discover");

            // 保持运行
            println!("\n⏳ Running (press Ctrl+C to stop)...\n");
            tokio::signal::ctrl_c().await?;
        }

        "discover" => {
            // 发现模式
            println!("\n🔍 Discovering local network...");

            // 1. 发现所有群组
            println!("\n📋 Step 1: Discovering groups...");
            let groups = Swarm::discover_groups(5).await?;

            if groups.is_empty() {
                println!("❌ No groups found on local network");
                println!("   Make sure another node is running in 'advertise' mode");
                return Ok(());
            }

            println!("✓ Found {} group(s):", groups.len());
            for group in &groups {
                println!("   - {}", group);
            }

            // 2. 发现群组 "cc" 的成员
            println!("\n👥 Step 2: Discovering members in group 'cc'...");
            let members = Swarm::discover_group_members("cc", 5).await?;

            if members.is_empty() {
                println!("❌ No members found in group 'cc'");
                return Ok(());
            }

            println!("✓ Found {} member(s):", members.len());
            for member in &members {
                println!("   - {} ({})", member.nickname, &member.node_id[..16]);
            }

            // 3. 连接到所有成员
            println!("\n🔗 Step 3: Connecting to members...");
            for member in &members {
                println!("   Connecting to {}...", member.nickname);
                // TODO: 实现 connect_peer
                // swarm.connect_peer(&member.node_addr).await?;
            }

            println!("\n✓ Discovery complete!");
            println!("\n💡 Connected peers: {}", swarm.connected_peers().await.len());

            // 保持运行
            println!("\n⏳ Running (press Ctrl+C to stop)...\n");
            tokio::signal::ctrl_c().await?;
        }

        _ => {
            eprintln!("Unknown mode: {}", mode);
            eprintln!("Use 'advertise' or 'discover'");
            std::process::exit(1);
        }
    }

    println!("\n👋 Shutting down...");
    swarm.shutdown().await?;

    Ok(())
}
