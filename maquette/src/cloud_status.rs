//! Cloud availability probe for the future v0.11 "hfrog as 云硬盘"
//! workstream.
//!
//! This slice is deliberately UI-only: it tells the user whether the
//! configured hfrog node appears reachable, but it does **not** change
//! File → Open / Save / Export behaviour. Local disk remains the
//! source-of-truth until v0.11.B introduces explicit "Push to cloud"
//! operations.
//!
//! Behaviour:
//!
//! * Startup spawns one probe against
//!   `<MAQUETTE_HFROG_BASE_URL>/api/artifactory/list?runtime=ping`.
//! * Probe timeout is short (1.5s). If hfrog is slow or offline, the
//!   app lands in `Offline` mode quickly and the UI keeps moving.
//! * The status-bar chip can write [`ProbeCloud`] to retry manually.
//! * Probe results are stored in [`CloudStatus`] for the UI to render.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy::window::RequestRedraw;

use maquette::block_meta::hfrog::HfrogConfig;

/// A deliberately short timeout: cloud availability is a UX hint, not a
/// blocker for local-first editing.
const CLOUD_PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Manual retry request. The UI writes this when the user clicks the
/// status-bar chip while offline (or while online and wanting to verify
/// the connection again).
#[derive(Message, Clone, Copy, Debug, Default)]
pub struct ProbeCloud;

/// Public mode enum rendered by the status chip.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AppCloudMode {
    /// Probe is currently running. Existing IO still stays local-first.
    #[default]
    Probing,
    /// hfrog responded successfully within the short timeout.
    Online {
        base_url: String,
        checked_at: i64,
    },
    /// hfrog was unreachable / slow / returned an error.
    Offline {
        base_url: String,
        last_error: String,
        checked_at: i64,
    },
}

impl AppCloudMode {
    pub fn chip_label(&self) -> &'static str {
        match self {
            AppCloudMode::Probing => "Cloud: checking...",
            AppCloudMode::Online { .. } => "Cloud: online",
            AppCloudMode::Offline { .. } => "Local mode",
        }
    }

    pub fn chip_color(&self) -> Color {
        match self {
            AppCloudMode::Probing => Color::srgb(0.55, 0.65, 0.85),
            AppCloudMode::Online { .. } => Color::srgb(0.35, 0.85, 0.55),
            AppCloudMode::Offline { .. } => Color::srgb(0.95, 0.72, 0.35),
        }
    }

    pub fn tooltip(&self) -> String {
        match self {
            AppCloudMode::Probing => {
                "Checking hfrog cloud status. Local files remain usable.".to_string()
            }
            AppCloudMode::Online {
                base_url,
                checked_at,
            } => format!(
                "hfrog is reachable at {base_url}\nlast checked: {checked_at}\n\nClick to probe again."
            ),
            AppCloudMode::Offline {
                base_url,
                last_error,
                checked_at,
            } => format!(
                "Working locally. hfrog probe failed at {base_url}.\nlast checked: {checked_at}\nerror: {last_error}\n\nClick to try online mode again."
            ),
        }
    }
}

/// Resource consumed by the UI. `in_flight` prevents duplicate probes
/// if the user clicks the chip repeatedly.
#[derive(Resource, Debug, Clone)]
pub struct CloudStatus {
    pub mode: AppCloudMode,
    pub in_flight: bool,
}

impl Default for CloudStatus {
    fn default() -> Self {
        Self {
            mode: AppCloudMode::Probing,
            in_flight: false,
        }
    }
}

#[derive(Component)]
struct PendingCloudProbe {
    base_url: String,
    task: Task<ProbeOutcome>,
}

#[derive(Debug)]
struct ProbeOutcome {
    ok: bool,
    message: String,
    checked_at: i64,
}

pub struct CloudStatusPlugin;

impl Plugin for CloudStatusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CloudStatus>()
            .add_message::<ProbeCloud>()
            .add_systems(Startup, start_initial_probe)
            .add_systems(Update, (handle_probe_requests, poll_probe_tasks).chain());
    }
}

fn start_initial_probe(mut requests: MessageWriter<ProbeCloud>) {
    requests.write(ProbeCloud);
}

fn handle_probe_requests(
    mut requests: MessageReader<ProbeCloud>,
    mut status: ResMut<CloudStatus>,
    mut commands: Commands,
) {
    if requests.is_empty() {
        return;
    }
    requests.clear();
    if status.in_flight {
        return;
    }

    let cfg = HfrogConfig::from_env();
    let base_url = cfg.base_url.clone();
    status.mode = AppCloudMode::Probing;
    status.in_flight = true;

    let task = AsyncComputeTaskPool::get().spawn(async move { probe_hfrog(&cfg) });
    commands.spawn(PendingCloudProbe { base_url, task });
}

fn poll_probe_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PendingCloudProbe)>,
    mut status: ResMut<CloudStatus>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    for (entity, mut pending) in &mut tasks {
        let Some(outcome) = block_on(future::poll_once(&mut pending.task)) else {
            continue;
        };
        commands.entity(entity).despawn();
        status.in_flight = false;
        let base_url = pending.base_url.clone();
        status.mode = if outcome.ok {
            AppCloudMode::Online {
                base_url,
                checked_at: outcome.checked_at,
            }
        } else {
            AppCloudMode::Offline {
                base_url,
                last_error: outcome.message,
                checked_at: outcome.checked_at,
            }
        };
        redraw.write(RequestRedraw);
    }
}

fn probe_hfrog(cfg: &HfrogConfig) -> ProbeOutcome {
    let checked_at = unix_seconds();
    let url = probe_url(&cfg.base_url);
    let response = ureq::AgentBuilder::new()
        .timeout(CLOUD_PROBE_TIMEOUT)
        .build()
        .get(&url)
        .call();

    match response {
        Ok(resp) if (200..300).contains(&resp.status()) => ProbeOutcome {
            ok: true,
            message: format!("HTTP {}", resp.status()),
            checked_at,
        },
        Ok(resp) => ProbeOutcome {
            ok: false,
            message: format!("HTTP {}", resp.status()),
            checked_at,
        },
        Err(err) => ProbeOutcome {
            ok: false,
            message: err.to_string(),
            checked_at,
        },
    }
}

fn probe_url(base_url: &str) -> String {
    format!(
        "{}/api/artifactory/list?runtime=ping",
        base_url.trim_end_matches('/')
    )
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_url_trims_trailing_slash() {
        assert_eq!(
            probe_url("https://hfrog.gamesci-lite.com/"),
            "https://hfrog.gamesci-lite.com/api/artifactory/list?runtime=ping"
        );
    }

    #[test]
    fn chip_labels_are_stable() {
        assert_eq!(AppCloudMode::Probing.chip_label(), "Cloud: checking...");
        assert_eq!(
            AppCloudMode::Online {
                base_url: "u".into(),
                checked_at: 1,
            }
            .chip_label(),
            "Cloud: online"
        );
        assert_eq!(
            AppCloudMode::Offline {
                base_url: "u".into(),
                last_error: "e".into(),
                checked_at: 1,
            }
            .chip_label(),
            "Local mode"
        );
    }
}
