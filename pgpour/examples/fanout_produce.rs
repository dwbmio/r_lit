//! Fan-out produce with per-target table filtering.
//!
//! Demonstrates:
//!   - primary target  → only public.table1 → produces to test_broker_1
//!   - secondary target → only public.table2 → produces to test_broker_2
//!   - unmatched table  → silently dropped
//!
//! Prerequisites: topics `test_broker_1` and `test_broker_2` must exist on the
//! StarLink-kafka-test cluster.
//!
//! ```bash
//! cargo run --example fanout_produce
//! ```

use std::collections::HashSet;
use std::time::Duration;

use futures::future::try_join_all;
use rdkafka::error::KafkaError;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json::json;

const BROKERS: &str = "10.9.169.2:9092,10.9.131.68:9092,10.9.103.246:9092";

struct Target {
    name: &'static str,
    producer: FutureProducer,
    topic: &'static str,
    tables: Option<HashSet<&'static str>>,
}

impl Target {
    fn accepts(&self, table: &str) -> bool {
        self.tables.as_ref().map_or(true, |s| s.contains(table))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let targets = vec![
        Target {
            name: "primary",
            producer: make_producer()?,
            topic: "test_broker_1",
            tables: Some(HashSet::from(["public.table1"])),
        },
        Target {
            name: "secondary",
            producer: make_producer()?,
            topic: "test_broker_2",
            tables: Some(HashSet::from(["public.table2"])),
        },
    ];

    // --- Scenario 1: table1 event → only primary ---
    println!("--- table1 event → only primary ---");
    fan_out(&targets, "public.table1", &json!({"id": 1, "data": "for table1"})).await?;

    // --- Scenario 2: table2 event → only secondary ---
    println!("\n--- table2 event → only secondary ---");
    fan_out(&targets, "public.table2", &json!({"id": 2, "data": "for table2"})).await?;

    // --- Scenario 3: table3 event → no match, dropped ---
    println!("\n--- table3 event → no match, dropped ---");
    fan_out(&targets, "public.table3", &json!({"id": 3, "data": "for table3"})).await?;

    println!("\ndone");
    Ok(())
}

async fn fan_out(
    targets: &[Target],
    table: &str,
    after: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let matching: Vec<_> = targets.iter().filter(|t| t.accepts(table)).collect();

    if matching.is_empty() {
        println!("  (no target matches {table} — event dropped)");
        return Ok(());
    }

    let payload = serde_json::to_vec(&json!({
        "op": "insert",
        "table": table,
        "after": after,
    }))?;

    let futs = matching.iter().map(|t| {
        let producer = t.producer.clone();
        let topic = t.topic;
        let name = t.name;
        let payload = payload.clone();
        async move {
            let record = FutureRecord::to(topic)
                .key("demo-key")
                .payload(&payload);
            let (partition, offset) = producer
                .send(record, Duration::from_secs(10))
                .await
                .map_err(|(e, _)| e)?;
            println!("  [{name}] → {topic}  partition={partition}  offset={offset}");
            Ok::<_, KafkaError>(())
        }
    });

    try_join_all(futs).await?;
    Ok(())
}

fn make_producer() -> Result<FutureProducer, Box<dyn std::error::Error>> {
    Ok(ClientConfig::new()
        .set("bootstrap.servers", BROKERS)
        .set("message.timeout.ms", "10000")
        .create()?)
}
