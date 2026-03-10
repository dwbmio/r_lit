use super::*;
use crate::config::KafkaTarget as KafkaTargetCfg;

fn plaintext_auth() -> KafkaAuth {
    KafkaTargetCfg::default().to_auth()
}

fn sasl_ssl_auth() -> KafkaAuth {
    KafkaTargetCfg {
        security_protocol: Some("sasl_ssl".into()),
        sasl_mechanism: Some("scram-sha-256".into()),
        sasl_username: Some("user".into()),
        sasl_password: Some("pass".into()),
        ..Default::default()
    }
    .to_auth()
}

#[test]
fn plaintext_producer_creates_ok() {
    let mut dest = KafkaDestination::new(MemoryStore::new());
    let result = dest.add_target("test", "localhost:9092", "test".into(), &plaintext_auth(), None);
    assert!(result.is_ok());
}

#[test]
fn sasl_ssl_producer_creates_ok() {
    let mut dest = KafkaDestination::new(MemoryStore::new());
    let result = dest.add_target("test", "localhost:9092", "test".into(), &sasl_ssl_auth(), None);
    assert!(result.is_ok(), "sasl_ssl create failed: {:?}", result.err());
}

#[test]
fn add_target_and_list_names() {
    let mut dest = KafkaDestination::new(MemoryStore::new());
    assert!(dest.target_names().is_empty());

    dest.add_target("main", "localhost:9092", "cdc".into(), &plaintext_auth(), None)
        .unwrap();
    assert_eq!(dest.target_names(), vec!["main"]);

    dest.add_target(
        "cluster-b",
        "localhost:9093",
        "cdc-backup".into(),
        &plaintext_auth(),
        None,
    )
    .unwrap();
    assert_eq!(dest.target_names(), vec!["main", "cluster-b"]);
}

#[test]
fn table_filter_accepts_and_rejects() {
    let mut dest = KafkaDestination::new(MemoryStore::new());

    dest.add_target(
        "orders",
        "localhost:9092",
        "cdc".into(),
        &plaintext_auth(),
        Some(vec!["public.orders".into()]),
    )
    .unwrap();
    dest.add_target(
        "analytics",
        "localhost:9093",
        "analytics".into(),
        &plaintext_auth(),
        Some(vec!["public.events".into(), "public.logs".into()]),
    )
    .unwrap();

    let orders = &dest.targets[0];
    let analytics = &dest.targets[1];

    assert!(orders.accepts_table("public.orders"));
    assert!(!orders.accepts_table("public.events"));

    assert!(!analytics.accepts_table("public.orders"));
    assert!(analytics.accepts_table("public.events"));
    assert!(analytics.accepts_table("public.logs"));
}

#[test]
fn empty_table_filter_means_all() {
    let mut dest = KafkaDestination::new(MemoryStore::new());
    dest.add_target("all", "localhost:9092", "cdc".into(), &plaintext_auth(), None)
        .unwrap();
    assert!(dest.targets[0].accepts_table("anything"));

    let mut dest2 = KafkaDestination::new(MemoryStore::new());
    dest2
        .add_target("all", "localhost:9092", "cdc".into(), &plaintext_auth(), Some(vec![]))
        .unwrap();
    assert!(dest2.targets[0].accepts_table("anything"));
}

/// Multi-kafka: each target connects to a different broker address
/// and maintains its own independent producer + table filter.
#[test]
fn multi_kafka_independent_clusters() {
    let mut dest = KafkaDestination::new(MemoryStore::new());

    dest.add_target(
        "cluster-a",
        "kafka-a:9092",
        "cdc.a".into(),
        &plaintext_auth(),
        Some(vec!["public.orders".into()]),
    )
    .unwrap();

    dest.add_target(
        "cluster-b",
        "kafka-b1:9092,kafka-b2:9092",
        "cdc.b".into(),
        &plaintext_auth(),
        Some(vec!["public.orders".into(), "public.user_events".into()]),
    )
    .unwrap();

    dest.add_target(
        "cluster-c",
        "kafka-prod:9094",
        "prod".into(),
        &sasl_ssl_auth(),
        Some(vec!["public.user_events".into()]),
    )
    .unwrap();

    assert_eq!(
        dest.target_names(),
        vec!["cluster-a", "cluster-b", "cluster-c"]
    );

    // public.orders → cluster-a, cluster-b (not cluster-c)
    assert!(dest.targets[0].accepts_table("public.orders"));
    assert!(dest.targets[1].accepts_table("public.orders"));
    assert!(!dest.targets[2].accepts_table("public.orders"));

    // public.user_events → cluster-b, cluster-c (not cluster-a)
    assert!(!dest.targets[0].accepts_table("public.user_events"));
    assert!(dest.targets[1].accepts_table("public.user_events"));
    assert!(dest.targets[2].accepts_table("public.user_events"));

    // public.unknown → nobody
    assert!(!dest.targets[0].accepts_table("public.unknown"));
    assert!(!dest.targets[1].accepts_table("public.unknown"));
    assert!(!dest.targets[2].accepts_table("public.unknown"));
}

/// Multi-kafka with mixed: some targets all-tables, some filtered.
#[test]
fn multi_kafka_mixed_filter_and_all() {
    let mut dest = KafkaDestination::new(MemoryStore::new());

    dest.add_target(
        "cluster-a-filtered",
        "kafka-a:9092",
        "cdc".into(),
        &plaintext_auth(),
        Some(vec!["public.orders".into()]),
    )
    .unwrap();

    dest.add_target(
        "cluster-b-all",
        "kafka-b:9092",
        "mirror".into(),
        &plaintext_auth(),
        None,
    )
    .unwrap();

    // cluster-a only gets orders
    assert!(dest.targets[0].accepts_table("public.orders"));
    assert!(!dest.targets[0].accepts_table("public.anything"));

    // cluster-b gets everything
    assert!(dest.targets[1].accepts_table("public.orders"));
    assert!(dest.targets[1].accepts_table("public.anything"));
    assert!(dest.targets[1].accepts_table("public.whatever"));
}

#[test]
fn hex_encode_works() {
    assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    assert_eq!(hex_encode(&[]), "");
}

#[test]
fn cell_to_json_coverage() {
    assert_eq!(KafkaDestination::cell_to_json(&Cell::Null), Value::Null);
    assert_eq!(
        KafkaDestination::cell_to_json(&Cell::Bool(true)),
        json!(true)
    );
    assert_eq!(
        KafkaDestination::cell_to_json(&Cell::String("hello".into())),
        json!("hello")
    );
    assert_eq!(KafkaDestination::cell_to_json(&Cell::I32(42)), json!(42));
    assert_eq!(KafkaDestination::cell_to_json(&Cell::I64(-1)), json!(-1));
    assert_eq!(
        KafkaDestination::cell_to_json(&Cell::F64(3.14)),
        json!(3.14)
    );
    assert_eq!(
        KafkaDestination::cell_to_json(&Cell::Numeric(PgNumeric::NaN)),
        json!("NaN")
    );
    assert_eq!(
        KafkaDestination::cell_to_json(&Cell::Bytes(vec![0xab, 0xcd])),
        json!("0xabcd")
    );
}

const TEST_BROKERS: &str = "10.9.169.2:9092,10.9.131.68:9092,10.9.103.246:9092";

/// Fan-out + per-target table filter integration test.
///
/// All targets are independent peers — no "primary" concept.
///
/// Run with: `cargo test fanout_table_filter -- --ignored`
#[tokio::test]
#[ignore]
async fn fanout_table_filter() {
    let mut dest = KafkaDestination::new(MemoryStore::new());
    dest.add_target(
        "target_a",
        TEST_BROKERS,
        "test_broker_1".into(),
        &plaintext_auth(),
        Some(vec!["public.table1".into()]),
    )
    .unwrap();
    dest.add_target(
        "target_b",
        TEST_BROKERS,
        "test_broker_2".into(),
        &plaintext_auth(),
        Some(vec!["public.table2".into()]),
    )
    .unwrap();

    assert_eq!(dest.target_names(), vec!["target_a", "target_b"]);

    let timeout = Duration::from_secs(10);

    async fn send(
        producer: &FutureProducer,
        topic: &str,
        key: &str,
        msg: &str,
    ) -> Result<(i32, i64), String> {
        let payload = serde_json::to_vec(&json!({
            "op": "insert", "table": topic, "after": {"key": key, "msg": msg}
        }))
        .unwrap();
        let record = FutureRecord::to(topic).key(key).payload(&payload);
        producer
            .send(record, Duration::from_secs(10))
            .await
            .map_err(|(e, _)| e.to_string())
    }

    // 1. table1 → only target_a
    assert!(dest.targets[0].accepts_table("public.table1"));
    assert!(!dest.targets[1].accepts_table("public.table1"));
    let (p, o) = send(
        &dest.targets[0].producer,
        "test_broker_1",
        "k1",
        "table1 event",
    )
    .await
    .expect("produce to test_broker_1 failed");
    println!("[target_a]  test_broker_1  partition={p} offset={o}");

    // 2. table2 → only target_b
    assert!(!dest.targets[0].accepts_table("public.table2"));
    assert!(dest.targets[1].accepts_table("public.table2"));
    let (p, o) = send(
        &dest.targets[1].producer,
        "test_broker_2",
        "k2",
        "table2 event",
    )
    .await
    .expect("produce to test_broker_2 failed");
    println!("[target_b]  test_broker_2  partition={p} offset={o}");

    // 3. table3 → dropped (no target matches)
    assert!(!dest.targets[0].accepts_table("public.table3"));
    assert!(!dest.targets[1].accepts_table("public.table3"));
    println!("[dropped]   public.table3  (no matching target)");

    // 4. concurrent fan-out to both targets
    let futs = [
        ("test_broker_1", dest.targets[0].producer.clone()),
        ("test_broker_2", dest.targets[1].producer.clone()),
    ]
    .map(|(topic, producer)| {
        let payload =
            serde_json::to_vec(&json!({"op":"insert","table":"shared","after":{"id":99}}))
                .unwrap();
        async move {
            let record = FutureRecord::to(topic).key("99").payload(&payload);
            producer
                .send(record, timeout)
                .await
                .map_err(|(e, _)| e.to_string())
        }
    });
    let results = futures::future::join_all(futs).await;
    for (i, r) in results.iter().enumerate() {
        let (p, o) =
            r.as_ref()
                .unwrap_or_else(|e| panic!("concurrent produce {i} failed: {e}"));
        println!("[concurrent] target={i} partition={p} offset={o}");
    }
}
