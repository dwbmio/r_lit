//! Tests for the Redis direct queue — requires a local redis on 127.0.0.1:6379.
//!
//! Run with: `redis-server --port 6379 --daemonize yes` then `cargo test --test redis_integration`
//! These tests are gated by env AUDIOGEN_TEST_REDIS=1 to avoid CI failures.

use audiogen::redis_q::{RedisQueue, DEFAULT_QUEUE, DEFAULT_RESULT};

fn redis_enabled() -> bool {
    std::env::var("AUDIOGEN_TEST_REDIS").ok().as_deref() == Some("1")
}

fn new_queue() -> RedisQueue {
    RedisQueue::new("redis://127.0.0.1:6379", Some(DEFAULT_QUEUE), Some(DEFAULT_RESULT)).unwrap()
}

#[tokio::test]
async fn redis_ping() {
    if !redis_enabled() { eprintln!("skip (AUDIOGEN_TEST_REDIS != 1)"); return; }
    let r = new_queue();
    assert_eq!(r.ping().await.unwrap(), "PONG");
}

#[tokio::test]
async fn redis_lpush_and_brpop_roundtrip() {
    if !redis_enabled() { eprintln!("skip (AUDIOGEN_TEST_REDIS != 1)"); return; }
    let r = new_queue();
    // Drain leftover from previous test.
    while r.brpop(1).await.unwrap().is_some() {}
    // Push 2 envelopes.
    let n1 = r.lpush(r#"{"cache_key":"test/audio/x","audio_b64":"YWJj"}"#).await.unwrap();
    let n2 = r.lpush(r#"{"cache_key":"test/audio/y","audio_b64":"ZGVm"}"#).await.unwrap();
    assert!(n1 >= 1 && n2 > n1, "queue length should grow: n1={} n2={}", n1, n2);
    // BRPOP returns the most recent first (LPUSH pushes to head, BRPOP pops from tail).
    let got = r.brpop(5).await.unwrap();
    let (key, val) = got.expect("expected a result");
    assert_eq!(key, DEFAULT_RESULT);
    assert!(val.contains("test/audio/"));
}