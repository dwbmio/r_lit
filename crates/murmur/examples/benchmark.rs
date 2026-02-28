use murmur::Swarm;
use std::time::{Duration, Instant};

/// 基准测试 - 测试 Murmur 的性能
///
/// 测试场景：
/// 1. 单节点写入性能
/// 2. 单节点读取性能
/// 3. 多节点同步延迟
/// 4. 结构化数据存储
///
/// 运行方式：
/// cargo run --example benchmark --release

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .init();

    println!("\n╔════════════════════════════════════════╗");
    println!("║  Murmur 性能基准测试                   ║");
    println!("╚════════════════════════════════════════╝\n");

    // 创建测试节点
    let swarm = Swarm::builder()
        .storage_path("./bench_data/node1")
        .group_id("benchmark")
        .build()
        .await?;

    swarm.start().await?;
    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("🔧 测试配置:");
    println!("  - 存储后端: redb (默认)");
    println!("  - 节点 ID: {}", &swarm.node_id().await[..16]);
    println!();

    // 测试 1: 写入性能
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📝 测试 1: 写入性能");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let write_counts = vec![100, 1000, 10000];
    for count in write_counts {
        let start = Instant::now();

        for i in 0..count {
            let key = format!("bench:write:{}", i);
            let value = format!("value_{}", i);
            swarm.put(&key, value.as_bytes()).await?;
        }

        let elapsed = start.elapsed();
        let ops_per_sec = count as f64 / elapsed.as_secs_f64();

        println!("  {} 次写入:", count);
        println!("    耗时: {:?}", elapsed);
        println!("    吞吐: {:.0} ops/s", ops_per_sec);
        println!();
    }

    // 测试 2: 读取性能
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📖 测试 2: 读取性能");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let read_counts = vec![100, 1000, 10000];
    for count in read_counts {
        let start = Instant::now();

        for i in 0..count {
            let key = format!("bench:write:{}", i % 1000);
            let _ = swarm.get(&key).await?;
        }

        let elapsed = start.elapsed();
        let ops_per_sec = count as f64 / elapsed.as_secs_f64();

        println!("  {} 次读取:", count);
        println!("    耗时: {:?}", elapsed);
        println!("    吞吐: {:.0} ops/s", ops_per_sec);
        println!();
    }

    // 测试 3: 结构化数据存储
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📦 测试 3: 结构化数据存储");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    #[derive(serde::Serialize, serde::Deserialize, Debug)]
    struct User {
        id: u64,
        name: String,
        email: String,
        age: u32,
        created_at: u64,
    }

    let user_count = 1000;
    let start = Instant::now();

    for i in 0..user_count {
        let user = User {
            id: i,
            name: format!("user_{}", i),
            email: format!("user_{}@example.com", i),
            age: 20 + (i % 50) as u32,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let key = format!("user:{}", i);
        let value = serde_json::to_vec(&user)?;
        swarm.put(&key, &value).await?;
    }

    let write_elapsed = start.elapsed();

    // 读取并反序列化
    let start = Instant::now();
    let mut users = Vec::new();

    for i in 0..user_count {
        let key = format!("user:{}", i);
        if let Some(value) = swarm.get(&key).await? {
            let user: User = serde_json::from_slice(&value)?;
            users.push(user);
        }
    }

    let read_elapsed = start.elapsed();

    println!("  {} 个用户对象:", user_count);
    println!("    写入耗时: {:?} ({:.0} ops/s)",
        write_elapsed,
        user_count as f64 / write_elapsed.as_secs_f64()
    );
    println!("    读取耗时: {:?} ({:.0} ops/s)",
        read_elapsed,
        user_count as f64 / read_elapsed.as_secs_f64()
    );
    println!("    成功读取: {} 个对象", users.len());
    println!();

    // 测试 4: 大值存储
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💾 测试 4: 大值存储");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let sizes = vec![1024, 10240, 102400]; // 1KB, 10KB, 100KB
    for size in sizes {
        let large_value = vec![0u8; size];
        let count = 100;

        let start = Instant::now();
        for i in 0..count {
            let key = format!("large:{}:{}", size, i);
            swarm.put(&key, &large_value).await?;
        }
        let elapsed = start.elapsed();

        let throughput_mb = (size * count) as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64();

        println!("  {} 字节 x {} 次:", size, count);
        println!("    耗时: {:?}", elapsed);
        println!("    吞吐: {:.2} MB/s", throughput_mb);
        println!();
    }

    // 测试 5: 并发写入
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("⚡ 测试 5: 并发写入");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let concurrent_tasks = 10;
    let ops_per_task = 100;

    let start = Instant::now();
    let mut handles = vec![];

    for task_id in 0..concurrent_tasks {
        let swarm_clone = swarm.clone();
        let handle = tokio::spawn(async move {
            for i in 0..ops_per_task {
                let key = format!("concurrent:{}:{}", task_id, i);
                let value = format!("value_{}_{}", task_id, i);
                swarm_clone.put(&key, value.as_bytes()).await.unwrap();
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await?;
    }

    let elapsed = start.elapsed();
    let total_ops = concurrent_tasks * ops_per_task;
    let ops_per_sec = total_ops as f64 / elapsed.as_secs_f64();

    println!("  {} 个并发任务 x {} 次操作:", concurrent_tasks, ops_per_task);
    println!("    总操作数: {}", total_ops);
    println!("    耗时: {:?}", elapsed);
    println!("    吞吐: {:.0} ops/s", ops_per_sec);
    println!();

    // 总结
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ 基准测试完成");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("💡 提示:");
    println!("  - 使用 --features rocksdb-backend 测试 RocksDB 性能");
    println!("  - 使用 --features sqlite-backend 测试 SQLite 性能");
    println!("  - 数据存储在 ./bench_data/ 目录");
    println!();

    swarm.shutdown().await?;

    // 清理测试数据
    println!("🧹 清理测试数据...");
    std::fs::remove_dir_all("./bench_data").ok();
    println!("✓ 完成");

    Ok(())
}
