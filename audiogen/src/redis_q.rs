//! Direct Redis client — bypass SSH, talk RESP straight to the rustyme queue.
//!
//! Why this exists: the original `racing_audiogen.py` SSHes to the rustyme VPS and runs
//! `redis-cli` locally. That works when port 22 is open outbound from your network,
//! but if VPS only exposes Redis (not SSH), or you don't have the SSH key, you're stuck.
//! Direct RESP requires only the Redis URL (often with TLS/password) and nothing more.
//!
//! Trade-off: a few features the SSH path had are awkward without shell scripting:
//!   - We can't run `redis-cli ping` style health checks as easily.
//!   - BRPOP is the only way to wait for results (we used to BRPOP 30s in a loop).
//! Both are fine and faster than the SSH wrapper.
//!
//! Usage:
//!   export AUDIOGEN_REDIS_URL="redis://152.32.210.127:6379"  # optional
//!   export AUDIOGEN_REDIS_QUEUE="rustyme:audgen-minimax:queue"
//!   export AUDIOGEN_REDIS_RESULT="rustyme:audgen-minimax:result"
//!   audiogen submit / collect / status

use anyhow::{Context, Result};
use redis::{aio::MultiplexedConnection, AsyncCommands, Client};

/// Default queue / result key names (match the python racing_audiogen).
pub const DEFAULT_QUEUE: &str = "rustyme:audgen-minimax:queue";
pub const DEFAULT_RESULT: &str = "rustyme:audgen-minimax:result";

/// Thin wrapper around redis::Client so we can swap the impl in tests.
pub struct RedisQueue {
    client: Client,
    queue_key: String,
    result_key: String,
}

impl RedisQueue {
    /// Open a connection from a URL like `redis://host:6379` or `rediss://...`.
    /// `queue` / `result` default to rustyme's known keys.
    pub fn new(url: &str, queue: Option<&str>, result: Option<&str>) -> Result<Self> {
        let client = Client::open(url).with_context(|| format!("parse redis URL {url}"))?;
        Ok(Self {
            client,
            queue_key: queue.unwrap_or(DEFAULT_QUEUE).to_string(),
            result_key: result.unwrap_or(DEFAULT_RESULT).to_string(),
        })
    }

    /// Get a multiplexed connection (cheap to clone).
    pub async fn conn(&self) -> Result<MultiplexedConnection> {
        self.client
            .get_multiplexed_async_connection()
            .await
            .context("connect redis")
    }

    /// LPUSH one JSON envelope. Returns the new list length (often useful for debugging).
    pub async fn lpush(&self, payload_json: &str) -> Result<i64> {
        let mut c = self.conn().await?;
        let n: i64 = c.lpush(&self.queue_key, payload_json).await
            .with_context(|| format!("LPUSH {}", self.queue_key))?;
        Ok(n)
    }

    /// BRPOP with timeout (seconds). Returns `Some((key, value))` or `None` on timeout.
    pub async fn brpop(&self, timeout_secs: u64) -> Result<Option<(String, String)>> {
        let mut c = self.conn().await?;
        let res: Option<[String; 2]> = c
            .brpop(&self.result_key, timeout_secs as f64)
            .await
            .context("BRPOP")?;
        Ok(res.map(|arr| {
            let mut it = arr.into_iter();
            let k = it.next().unwrap_or_default();
            let v = it.next().unwrap_or_default();
            (k, v)
        }))
    }

    /// PING — sanity check the connection.
    pub async fn ping(&self) -> Result<String> {
        let mut c = self.conn().await?;
        let pong: String = redis::cmd("PING").query_async(&mut c).await.context("PING")?;
        Ok(pong)
    }

    /// Read the current `.pending.json` from a local dir as a sanity probe — does NOT
    /// touch Redis. (Useful for the `status` subcommand.)
    pub fn queue_key(&self) -> &str { &self.queue_key }
    pub fn result_key(&self) -> &str { &self.result_key }
}