//! Boot-time hfrog reachability probe + cloud / offline mode state.
//!
//! mj_atlas always works locally — hfrog is treated as a "cloud drive" that
//! the app *prefers* but does not require. On startup we fire one quick
//! probe (1.5 s timeout) at the configured endpoint; the result is surfaced
//! in the menubar status indicator and gates whether the Welcome screen
//! shows cloud-side projects.
//!
//! Design notes:
//!   - Probing happens on a worker thread so the UI never blocks. Result is
//!     posted back through an `mpsc::Receiver` that the eframe update loop
//!     polls each frame (same pattern as the pack worker thread).
//!   - The probe target is GET `/api/artifactory/runtime/list?index=0&cnt=1`
//!     — light, no auth, returns immediately when hfrog is up. Any 2xx with
//!     a `code:0` envelope is "Online"; everything else (including the 30 s
//!     hang of a dead DNS) collapses to "Offline" via the timeout.
//!   - We deliberately do NOT probe by writing — that would burn S3
//!     dispatches and falsely fail when the read path is healthy but the
//!     write path is broken (current state of hfrog.gamesci-lite.com).

use crate::config::HfrogConfig;
use std::sync::mpsc;
use std::time::Duration;

/// What the app currently believes about hfrog reachability. Drives the
/// menubar badge and gates cloud-side reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMode {
    /// Probe in flight — the UI shows a spinner. We never sit in this state
    /// for more than the probe timeout (1.5 s) before the worker resolves it.
    Probing,
    /// Hfrog responded healthily; cloud reads are enabled.
    Online,
    /// Probe failed (timeout / network / non-2xx) or mirror is disabled in
    /// the config. UI tells the user we're local-only and offers a retry.
    Offline,
}

/// Surface for the UI: current mode + last error string + last attempt time.
/// Cloned cheaply; the app holds one and the update loop replaces it when a
/// new probe result arrives.
#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub mode: ConnectionMode,
    /// Human-readable error from the last failed probe. Empty when Online or
    /// when the user hasn't probed yet.
    pub last_error: String,
    /// Endpoint we last probed against, for the badge tooltip.
    pub probed_endpoint: String,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            mode: ConnectionMode::Probing,
            last_error: String::new(),
            probed_endpoint: String::new(),
        }
    }
}

impl ConnectionState {
    /// True iff the probe is currently in flight — UI shows a spinner.
    pub fn is_probing(&self) -> bool {
        matches!(self.mode, ConnectionMode::Probing)
    }
    /// True iff we believe the cloud side is reachable for reads.
    pub fn is_online(&self) -> bool {
        matches!(self.mode, ConnectionMode::Online)
    }
}

/// Result of one probe attempt — sent back through the mpsc to the UI.
#[derive(Debug)]
pub struct ProbeResult {
    pub mode: ConnectionMode,
    pub error: String,
    pub endpoint: String,
}

/// Spawn a worker thread that probes the configured hfrog endpoint and posts
/// a single ProbeResult. Returns the receiver immediately. The app polls it
/// each frame; when a value arrives, the connection state flips.
///
/// When `cfg` is inactive (mirror disabled / endpoint blank) the probe still
/// resolves — but instantly to Offline with a "mirror disabled" reason. The
/// UI flow is uniform either way.
pub fn spawn_probe(cfg: &HfrogConfig) -> mpsc::Receiver<ProbeResult> {
    let (tx, rx) = mpsc::channel();
    let endpoint = cfg.endpoint.trim_end_matches('/').to_string();
    let token = cfg.token.clone();
    let enabled = cfg.enabled;

    std::thread::spawn(move || {
        let result = if !enabled {
            ProbeResult {
                mode: ConnectionMode::Offline,
                error: "mirror disabled in config".to_string(),
                endpoint,
            }
        } else if endpoint.is_empty() {
            ProbeResult {
                mode: ConnectionMode::Offline,
                error: "no endpoint configured".to_string(),
                endpoint,
            }
        } else {
            probe_blocking(&endpoint, &token)
        };
        let _ = tx.send(result);
    });

    rx
}

/// Synchronous probe — used directly by tests; spawn_probe wraps this on a
/// worker thread for the UI flow.
pub fn probe_blocking(endpoint: &str, token: &str) -> ProbeResult {
    let endpoint = endpoint.trim_end_matches('/').to_string();
    // 1.5 s budget — comfortably above same-region RTT but short enough that
    // the user doesn't feel the GUI is hung when hfrog is dead.
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(1500))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ProbeResult {
                mode: ConnectionMode::Offline,
                error: format!("client build failed: {}", e),
                endpoint,
            }
        }
    };

    let url = format!("{}/api/artifactory/runtime/list?index=0&cnt=1", endpoint);
    let mut req = client.get(&url);
    if !token.is_empty() {
        req = req.bearer_auth(token);
    }

    match req.send() {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                return ProbeResult {
                    mode: ConnectionMode::Offline,
                    error: format!("HTTP {} from {}", status, url),
                    endpoint,
                };
            }
            // Read body and check hfrog's RespVO envelope. Code 0 = healthy;
            // anything else is a server-side issue we still treat as "online
            // but degraded" — but for the boot probe we keep it strict and
            // require code:0 so the UI doesn't claim cloud reads work when
            // they don't.
            let body = resp.text().unwrap_or_default();
            #[derive(serde::Deserialize)]
            struct Env {
                code: i32,
                #[serde(default)]
                msg: String,
            }
            match serde_json::from_str::<Env>(&body) {
                Ok(env) if env.code == 0 => ProbeResult {
                    mode: ConnectionMode::Online,
                    error: String::new(),
                    endpoint,
                },
                Ok(env) => ProbeResult {
                    mode: ConnectionMode::Offline,
                    error: format!("hfrog envelope code={} msg={}", env.code, env.msg),
                    endpoint,
                },
                Err(_) => ProbeResult {
                    mode: ConnectionMode::Offline,
                    error: "non-JSON response from hfrog".to_string(),
                    endpoint,
                },
            }
        }
        Err(e) => ProbeResult {
            mode: ConnectionMode::Offline,
            // reqwest's timeout error stringifies as "operation timed out";
            // surface that as the user-visible reason so retry intuitions match.
            error: if e.is_timeout() {
                format!("timed out after 1.5s contacting {}", url)
            } else {
                format!("{}", e)
            },
            endpoint,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HfrogConfig;

    #[test]
    fn probe_with_disabled_config_resolves_to_offline_immediately() {
        let cfg = HfrogConfig {
            enabled: false,
            endpoint: "https://hfrog.test".to_string(),
            ..Default::default()
        };
        let rx = spawn_probe(&cfg);
        let result = rx
            .recv_timeout(Duration::from_secs(1))
            .expect("disabled-config probe must resolve");
        assert_eq!(result.mode, ConnectionMode::Offline);
        assert!(result.error.contains("disabled"), "got: {}", result.error);
    }

    #[test]
    fn probe_with_empty_endpoint_resolves_to_offline_immediately() {
        let cfg = HfrogConfig {
            enabled: true,
            endpoint: String::new(),
            ..Default::default()
        };
        let rx = spawn_probe(&cfg);
        let result = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(result.mode, ConnectionMode::Offline);
        assert!(result.error.contains("no endpoint"));
    }

    #[test]
    fn probe_with_unreachable_host_times_out_within_two_seconds() {
        // 192.0.2.0/24 is the IETF-reserved TEST-NET-1 range. Connections
        // there always black-hole, exercising the timeout branch without
        // depending on resolve-failure ergonomics.
        let cfg = HfrogConfig {
            enabled: true,
            endpoint: "http://192.0.2.1:65530".to_string(),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let rx = spawn_probe(&cfg);
        let result = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(result.mode, ConnectionMode::Offline);
        // Bound the test runtime so a regression in the timeout doesn't make
        // CI take 30 s. 1.5 s probe + small buffer.
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "probe took {:?}; timeout regression?",
            started.elapsed()
        );
    }
}
