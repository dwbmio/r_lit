use super::*;

// ── YAML parsing ────────────────────────────────────────────────

const FULL_YAML: &str = r#"
postgres:
  host: db.example.com
  port: 5432
  database: mydb
  username: admin
  password: secret
  publication: my_pub
  ssh_host: bastion.example.com
  ssh_port: 2222
  ssh_username: tunnel_user
  ssh_password: tunnel_pass
  ssh_private_key_path: /keys/id_rsa
  ssh_private_key_passphrase: keypass

kafka:
  brokers: broker1:9092,broker2:9092
  security_protocol: sasl_ssl
  sasl_mechanism: scram-sha-256
  sasl_username: kafka_user
  sasl_password: kafka_pass
  ssl_ca_location: /certs/ca.pem
  ssl_certificate_location: /certs/client.pem
  ssl_key_location: /certs/client.key

  targets:
    - name: default
      topic_prefix: events

pipeline:
  batch_max_fill_ms: 3000
  max_table_sync_workers: 8

otel:
  endpoint: http://collector:4317
"#;

const MINIMAL_YAML: &str = r#"
postgres:
  host: localhost
kafka:
  brokers: localhost:9092
  targets:
    - name: default
      topic_prefix: cdc
"#;

const MULTI_TARGET_YAML: &str = r#"
kafka:
  brokers: primary:9092
  security_protocol: sasl_ssl
  sasl_mechanism: scram-sha-512
  sasl_username: user_shared

  targets:
    - name: main
      topic_prefix: cdc
      tables:
        - public.orders
    - name: cluster-b
      brokers: backup:9092
      topic_prefix: cdc-backup
      security_protocol: sasl_ssl
      sasl_username: user_b
      sasl_password: pass_b
    - name: analytics
      topic_prefix: analytics
      tables:
        - public.events
        - public.logs
"#;

/// Multi-kafka: no shared brokers, each target brings its own cluster connection.
const MULTI_KAFKA_YAML: &str = r#"
kafka:
  targets:
    - name: cluster-a
      brokers: kafka-a1:9092,kafka-a2:9092
      topic_prefix: cdc.cluster-a
      security_protocol: plaintext
      tables:
        - public.orders

    - name: cluster-b
      brokers: kafka-b1:9092,kafka-b2:9092,kafka-b3:9092
      topic_prefix: cdc.cluster-b
      security_protocol: plaintext
      tables:
        - public.orders
        - public.user_events

    - name: cluster-c-prod
      brokers: kafka-prod.example.com:9094
      topic_prefix: prod.analytics
      security_protocol: sasl_ssl
      sasl_mechanism: scram-sha-512
      sasl_username: pgpour_producer
      sasl_password: secret
      ssl_ca_location: /etc/pgpour/ca-prod.pem
      tables:
        - public.user_events
        - public.analytics_log
"#;

#[test]
fn parse_full_config() {
    let cfg: FileConfig = serde_yaml::from_str(FULL_YAML).unwrap();

    assert_eq!(cfg.postgres.host.as_deref(), Some("db.example.com"));
    assert_eq!(cfg.postgres.port, Some(5432));
    assert_eq!(cfg.postgres.database.as_deref(), Some("mydb"));
    assert_eq!(cfg.postgres.ssh_host.as_deref(), Some("bastion.example.com"));
    assert_eq!(cfg.postgres.ssh_port, Some(2222));
    assert_eq!(cfg.postgres.ssh_username.as_deref(), Some("tunnel_user"));
    assert_eq!(
        cfg.postgres.ssh_private_key_path.as_deref(),
        Some("/keys/id_rsa")
    );

    assert_eq!(
        cfg.kafka.brokers.as_deref(),
        Some("broker1:9092,broker2:9092")
    );
    assert_eq!(cfg.kafka.security_protocol.as_deref(), Some("sasl_ssl"));
    assert_eq!(cfg.kafka.sasl_mechanism.as_deref(), Some("scram-sha-256"));
    assert_eq!(cfg.kafka.sasl_username.as_deref(), Some("kafka_user"));
    assert_eq!(cfg.kafka.ssl_ca_location.as_deref(), Some("/certs/ca.pem"));

    assert_eq!(cfg.kafka.targets.len(), 1);
    assert_eq!(cfg.kafka.targets[0].name.as_deref(), Some("default"));
    assert_eq!(cfg.kafka.targets[0].topic_prefix.as_deref(), Some("events"));

    assert_eq!(cfg.pipeline.batch_max_fill_ms, Some(3000));
    assert_eq!(cfg.pipeline.max_table_sync_workers, Some(8));
    assert_eq!(cfg.otel.endpoint.as_deref(), Some("http://collector:4317"));
}

#[test]
fn parse_minimal_config_defaults_to_none() {
    let cfg: FileConfig = serde_yaml::from_str(MINIMAL_YAML).unwrap();

    assert_eq!(cfg.postgres.host.as_deref(), Some("localhost"));
    assert!(cfg.postgres.port.is_none());
    assert!(cfg.postgres.ssh_host.is_none());
    assert!(cfg.kafka.security_protocol.is_none());
    assert!(cfg.kafka.sasl_mechanism.is_none());
    assert!(cfg.pipeline.batch_max_fill_ms.is_none());
    assert!(cfg.otel.endpoint.is_none());
}

#[test]
fn parse_empty_config() {
    let cfg: FileConfig = serde_yaml::from_str("").unwrap();
    assert!(cfg.postgres.host.is_none());
    assert!(cfg.kafka.brokers.is_none());
    assert!(cfg.kafka.targets.is_empty());
}

#[test]
fn load_example_config_file() {
    let cfg = FileConfig::load("examples/config.example.yml")
        .expect("examples/config.example.yml should be parseable");
    assert!(cfg.kafka.brokers.is_some());
    assert_eq!(cfg.kafka.targets.len(), 1);
}

#[test]
fn load_nonexistent_file_returns_error() {
    assert!(FileConfig::load("/tmp/nonexistent_pgpour_config.yml").is_err());
}

#[test]
fn parse_multi_target_config() {
    let cfg: FileConfig = serde_yaml::from_str(MULTI_TARGET_YAML).unwrap();
    assert_eq!(cfg.kafka.brokers.as_deref(), Some("primary:9092"));
    assert_eq!(cfg.kafka.targets.len(), 3);

    let main = &cfg.kafka.targets[0];
    assert_eq!(main.name.as_deref(), Some("main"));
    assert_eq!(main.topic_prefix.as_deref(), Some("cdc"));
    assert_eq!(
        main.tables.as_deref(),
        Some(["public.orders".to_string()].as_slice())
    );
    assert!(main.brokers.is_none());

    let b = &cfg.kafka.targets[1];
    assert_eq!(b.name.as_deref(), Some("cluster-b"));
    assert_eq!(b.brokers.as_deref(), Some("backup:9092"));
    assert_eq!(b.topic_prefix.as_deref(), Some("cdc-backup"));
    assert_eq!(b.security_protocol.as_deref(), Some("sasl_ssl"));
    assert!(b.tables.is_none());

    let a = &cfg.kafka.targets[2];
    assert_eq!(a.name.as_deref(), Some("analytics"));
    assert!(a.brokers.is_none());
    assert_eq!(a.tables.as_ref().unwrap().len(), 2);
    assert!(a.tables.as_ref().unwrap().contains(&"public.events".to_string()));
}

#[test]
fn parse_multi_kafka_config() {
    let cfg: FileConfig = serde_yaml::from_str(MULTI_KAFKA_YAML).unwrap();
    assert!(cfg.kafka.brokers.is_none(), "no shared brokers in multi-kafka mode");
    assert_eq!(cfg.kafka.targets.len(), 3);

    let a = &cfg.kafka.targets[0];
    assert_eq!(a.name.as_deref(), Some("cluster-a"));
    assert_eq!(a.brokers.as_deref(), Some("kafka-a1:9092,kafka-a2:9092"));
    assert_eq!(a.security_protocol.as_deref(), Some("plaintext"));

    let b = &cfg.kafka.targets[1];
    assert_eq!(b.name.as_deref(), Some("cluster-b"));
    assert_eq!(b.brokers.as_deref(), Some("kafka-b1:9092,kafka-b2:9092,kafka-b3:9092"));

    let c = &cfg.kafka.targets[2];
    assert_eq!(c.name.as_deref(), Some("cluster-c-prod"));
    assert_eq!(c.brokers.as_deref(), Some("kafka-prod.example.com:9094"));
    assert_eq!(c.security_protocol.as_deref(), Some("sasl_ssl"));
    assert_eq!(c.sasl_mechanism.as_deref(), Some("scram-sha-512"));
    assert_eq!(c.sasl_username.as_deref(), Some("pgpour_producer"));
    assert_eq!(c.ssl_ca_location.as_deref(), Some("/etc/pgpour/ca-prod.pem"));
}

#[test]
fn load_multi_kafka_example_config_file() {
    let cfg = FileConfig::load("examples/config.multi-kafka.example.yml")
        .expect("multi-kafka example should parse");
    assert!(cfg.kafka.targets.len() >= 2, "multi-kafka example must have at least 2 targets");
    for t in &cfg.kafka.targets {
        assert!(t.brokers.is_some(), "each target in multi-kafka example must have its own brokers");
    }
    assert!(validate_kafka_targets(&cfg.kafka).is_ok());
}

#[test]
fn empty_targets_defaults_to_empty_vec() {
    let cfg: FileConfig = serde_yaml::from_str("kafka:\n  brokers: x:9092\n").unwrap();
    assert!(cfg.kafka.targets.is_empty());
}

// ── Validation ──────────────────────────────────────────────────

fn make_kafka(targets: Vec<KafkaTarget>) -> KafkaConfig {
    KafkaConfig {
        brokers: Some("localhost:9092".into()),
        targets,
        ..Default::default()
    }
}

fn target(name: &str, prefix: &str) -> KafkaTarget {
    KafkaTarget {
        name: Some(name.into()),
        topic_prefix: Some(prefix.into()),
        ..Default::default()
    }
}

#[test]
fn validate_accepts_single_target() {
    let kafka = make_kafka(vec![target("default", "cdc")]);
    assert!(validate_kafka_targets(&kafka).is_ok());
}

#[test]
fn validate_accepts_multi_target() {
    let kafka = make_kafka(vec![
        target("main", "cdc"),
        KafkaTarget {
            name: Some("backup".into()),
            brokers: Some("backup:9092".into()),
            topic_prefix: Some("cdc-backup".into()),
            ..Default::default()
        },
        KafkaTarget {
            name: Some("analytics".into()),
            topic_prefix: Some("analytics".into()),
            tables: Some(vec!["public.events".into()]),
            ..Default::default()
        },
    ]);
    assert!(validate_kafka_targets(&kafka).is_ok());
}

#[test]
fn validate_rejects_empty_targets() {
    let kafka = make_kafka(vec![]);
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("at least one target"), "{err}");
}

#[test]
fn validate_rejects_unqualified_table_name() {
    let kafka = make_kafka(vec![KafkaTarget {
        name: Some("t".into()),
        topic_prefix: Some("cdc".into()),
        tables: Some(vec!["orders".into()]),
        ..Default::default()
    }]);
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("invalid table reference 'orders'"), "{err}");
}

#[test]
fn validate_rejects_empty_schema_or_table() {
    for bad in &[".orders", "public.", ".", ""] {
        let kafka = make_kafka(vec![KafkaTarget {
            name: Some("t".into()),
            topic_prefix: Some("cdc".into()),
            tables: Some(vec![bad.to_string()]),
            ..Default::default()
        }]);
        assert!(
            validate_kafka_targets(&kafka).is_err(),
            "should reject '{bad}'"
        );
    }
}

#[test]
fn validate_rejects_missing_name() {
    let kafka = make_kafka(vec![KafkaTarget {
        topic_prefix: Some("backup".into()),
        ..Default::default()
    }]);
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("'name' is required"), "{err}");
}

#[test]
fn validate_rejects_missing_topic_prefix() {
    let kafka = make_kafka(vec![KafkaTarget {
        name: Some("backup".into()),
        ..Default::default()
    }]);
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("'topic_prefix' is required"), "{err}");
}

#[test]
fn validate_rejects_duplicate_name() {
    let kafka = make_kafka(vec![
        target("dup", "a"),
        target("dup", "b"),
    ]);
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("duplicate target name"), "{err}");
}

#[test]
fn validate_accepts_multi_kafka_no_shared_brokers() {
    let kafka = KafkaConfig {
        brokers: None,
        targets: vec![
            KafkaTarget {
                name: Some("cluster-a".into()),
                brokers: Some("kafka-a:9092".into()),
                topic_prefix: Some("cdc-a".into()),
                ..Default::default()
            },
            KafkaTarget {
                name: Some("cluster-b".into()),
                brokers: Some("kafka-b:9092".into()),
                topic_prefix: Some("cdc-b".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert!(validate_kafka_targets(&kafka).is_ok());
}

#[test]
fn validate_rejects_missing_brokers_everywhere() {
    let kafka = KafkaConfig {
        brokers: None,
        targets: vec![KafkaTarget {
            name: Some("orphan".into()),
            topic_prefix: Some("cdc".into()),
            // no brokers here, no shared brokers either
            ..Default::default()
        }],
        ..Default::default()
    };
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("'brokers' is required"), "{err}");
}

#[test]
fn validate_multi_kafka_yaml() {
    let cfg: FileConfig = serde_yaml::from_str(MULTI_KAFKA_YAML).unwrap();
    assert!(validate_kafka_targets(&cfg.kafka).is_ok());
}

#[test]
fn validate_rejects_duplicate_broker_prefix_combo() {
    let kafka = make_kafka(vec![
        target("a", "cdc"),
        target("b", "cdc"),
    ]);
    let err = validate_kafka_targets(&kafka).unwrap_err();
    assert!(err.contains("duplicate (brokers, topic_prefix)"), "{err}");
}

#[test]
fn validate_same_prefix_different_brokers_ok() {
    let kafka = make_kafka(vec![
        target("a", "cdc"),
        KafkaTarget {
            name: Some("b".into()),
            brokers: Some("other-cluster:9092".into()),
            topic_prefix: Some("cdc".into()),
            ..Default::default()
        },
    ]);
    assert!(validate_kafka_targets(&kafka).is_ok());
}

#[test]
fn validate_fanout_example_config() {
    let cfg = FileConfig::load("examples/config.fanout.example.yml")
        .expect("fanout example should parse");
    assert!(validate_kafka_targets(&cfg.kafka).is_ok());
}

// ── with_defaults ───────────────────────────────────────────────

#[test]
fn with_defaults_inherits_connection_fields() {
    let defaults = KafkaConfig {
        brokers: Some("primary:9092".into()),
        security_protocol: Some("sasl_ssl".into()),
        sasl_username: Some("admin".into()),
        sasl_password: Some("secret".into()),
        ssl_ca_location: Some("/ca.pem".into()),
        ..Default::default()
    };
    let t = KafkaTarget {
        name: Some("backup".into()),
        topic_prefix: Some("backup".into()),
        ..Default::default()
    };
    let resolved = t.with_defaults(&defaults);

    assert_eq!(resolved.brokers.as_deref(), Some("primary:9092"));
    assert_eq!(resolved.security_protocol.as_deref(), Some("sasl_ssl"));
    assert_eq!(resolved.sasl_username.as_deref(), Some("admin"));
    assert_eq!(resolved.sasl_password.as_deref(), Some("secret"));
    assert_eq!(resolved.ssl_ca_location.as_deref(), Some("/ca.pem"));
    assert_eq!(resolved.topic_prefix.as_deref(), Some("backup"));
}

#[test]
fn with_defaults_target_overrides_defaults() {
    let defaults = KafkaConfig {
        brokers: Some("primary:9092".into()),
        sasl_username: Some("primary-user".into()),
        ..Default::default()
    };
    let t = KafkaTarget {
        name: Some("custom".into()),
        brokers: Some("custom:9092".into()),
        topic_prefix: Some("custom".into()),
        sasl_username: Some("custom-user".into()),
        ..Default::default()
    };
    let resolved = t.with_defaults(&defaults);
    assert_eq!(resolved.brokers.as_deref(), Some("custom:9092"));
    assert_eq!(resolved.sasl_username.as_deref(), Some("custom-user"));
}

#[test]
fn with_defaults_does_not_touch_routing_fields() {
    let defaults = KafkaConfig {
        brokers: Some("primary:9092".into()),
        ..Default::default()
    };
    let t = KafkaTarget {
        name: Some("x".into()),
        topic_prefix: Some("my-prefix".into()),
        tables: Some(vec!["public.t".into()]),
        ..Default::default()
    };
    let resolved = t.with_defaults(&defaults);
    assert_eq!(resolved.name.as_deref(), Some("x"));
    assert_eq!(resolved.topic_prefix.as_deref(), Some("my-prefix"));
    assert_eq!(resolved.tables.as_ref().unwrap().len(), 1);
}

#[test]
fn with_defaults_no_shared_brokers_keeps_target_brokers() {
    let defaults = KafkaConfig {
        brokers: None,
        ..Default::default()
    };
    let t = KafkaTarget {
        name: Some("cluster-x".into()),
        brokers: Some("kafka-x:9092".into()),
        topic_prefix: Some("prefix".into()),
        security_protocol: Some("sasl_ssl".into()),
        ..Default::default()
    };
    let resolved = t.with_defaults(&defaults);
    assert_eq!(resolved.brokers.as_deref(), Some("kafka-x:9092"));
    assert_eq!(resolved.security_protocol.as_deref(), Some("sasl_ssl"));
}

#[test]
fn with_defaults_no_shared_no_target_brokers_is_none() {
    let defaults = KafkaConfig {
        brokers: None,
        ..Default::default()
    };
    let t = KafkaTarget {
        name: Some("orphan".into()),
        topic_prefix: Some("cdc".into()),
        ..Default::default()
    };
    let resolved = t.with_defaults(&defaults);
    assert!(resolved.brokers.is_none());
}

// ── to_auth ─────────────────────────────────────────────────────

#[test]
fn to_auth_defaults_to_plaintext() {
    let t = KafkaTarget::default();
    let auth = t.to_auth();
    assert_eq!(auth.security_protocol, "plaintext");
    assert!(auth.sasl_mechanism.is_none());
}

#[test]
fn to_auth_uses_configured_values() {
    let t = KafkaTarget {
        security_protocol: Some("sasl_ssl".into()),
        sasl_mechanism: Some("scram-sha-256".into()),
        sasl_username: Some("user".into()),
        sasl_password: Some("pass".into()),
        ssl_ca_location: Some("/ca.pem".into()),
        ..Default::default()
    };
    let auth = t.to_auth();
    assert_eq!(auth.security_protocol, "sasl_ssl");
    assert_eq!(auth.sasl_mechanism.as_deref(), Some("scram-sha-256"));
    assert_eq!(auth.sasl_username.as_deref(), Some("user"));
    assert_eq!(auth.ssl_ca_location.as_deref(), Some("/ca.pem"));
}

// ── helpers ─────────────────────────────────────────────────────

#[test]
fn qualified_table_name_check() {
    assert!(is_qualified_table_name("public.orders"));
    assert!(is_qualified_table_name("myschema.my_table"));
    assert!(!is_qualified_table_name("orders"));
    assert!(!is_qualified_table_name(".orders"));
    assert!(!is_qualified_table_name("public."));
    assert!(!is_qualified_table_name("."));
    assert!(!is_qualified_table_name(""));
    assert!(!is_qualified_table_name("a.b.c"));
}

#[test]
fn normalize_brokers_is_order_independent() {
    assert_eq!(
        normalize_brokers("b:9092,a:9092"),
        normalize_brokers("a:9092,b:9092")
    );
    assert_eq!(
        normalize_brokers("a:9092, b:9092"),
        normalize_brokers("b:9092,a:9092")
    );
}
