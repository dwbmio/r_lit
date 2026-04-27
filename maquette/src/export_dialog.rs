//! Non-blocking native "Save As…" dialog for File → Export.
//!
//! ## Why this exists
//!
//! The previous implementation called `rfd::FileDialog::save_file()`
//! synchronously from inside `ui_system` (an `EguiPrimaryContextPass`
//! system). On macOS 26+, `rfd`'s sync path ends in
//! `NSSavePanel.runModal()`, which nests a modal run-loop under
//! winit's own run-loop callback. The two loops compete for the
//! main-thread dispatch queue and the whole app wedges — the log
//! shows `ui: opening save dialog for export` and nothing after it,
//! with no "dispatching ExportRequest" or "save dialog cancelled"
//! ever following.
//!
//! `rfd::AsyncFileDialog` takes the `beginSheetModalForWindow:
//! completionHandler:` path instead, which is non-modal and runs
//! entirely through GCD blocks on the main thread — exactly the
//! integration pattern Bevy's reactive event loop wants.
//!
//! ## Flow
//!
//! 1. User clicks **Choose file & export** in the Export modal.
//! 2. `ui.rs` calls `PendingExportDialog::open(...)`; the Export
//!    modal closes, so egui doesn't keep redrawing the now-empty
//!    dialog box.
//! 3. `open` stores the chosen `ExportFormat` + `OutlineConfig`
//!    alongside an `AsyncComputeTaskPool` task that awaits
//!    `AsyncFileDialog::save_file()`.
//! 4. `poll_pending_export_dialog` polls that task every frame with
//!    `future::poll_once`, pumping a `RequestRedraw` so
//!    `WinitSettings::desktop_app()`'s 5 s heartbeat isn't the only
//!    thing driving polls.
//! 5. When the future resolves, the system either writes an
//!    `ExportRequest` (user picked a path) or logs a cancel and
//!    drops the job (user hit Cancel or dismissed the sheet).
//!
//! The live-exporting path in `export.rs` (snapshot → write on the
//! async compute pool → emit `ExportOutcome`) is unchanged — this
//! module only replaces the upstream file-picker half.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy::window::RequestRedraw;

use maquette::export::{ExportFormat, ExportOptions, ExportRequest, OutlineConfig};

pub struct ExportDialogPlugin;

impl Plugin for ExportDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingExportDialog>()
            .add_systems(Update, poll_pending_export_dialog);
    }
}

/// Live state for an in-flight native save-panel.
///
/// `Some` while the user has the sheet open (or it's about to show);
/// flipped back to `None` the frame its future resolves. Exposed so
/// `ui.rs` can disable the "Export…" menu item while a dialog is
/// already up, mirroring how `ExportInProgress::is_running()` guards
/// re-entrancy on the write side.
#[derive(Resource, Default)]
pub struct PendingExportDialog {
    inner: Option<Pending>,
}

struct Pending {
    task: Task<Option<PathBuf>>,
    format: ExportFormat,
    outline: OutlineConfig,
}

impl PendingExportDialog {
    /// Is a save-panel currently on screen (or spawning)? UI uses
    /// this to avoid stacking two dialogs if the user keeps clicking
    /// "Choose file & export".
    pub fn is_pending(&self) -> bool {
        self.inner.is_some()
    }

    /// Spawn the native save-panel. Returns immediately; the panel
    /// shows up when the main-thread dispatch queue next drains.
    ///
    /// `default_name` is the suggested filename (pre-filled in the
    /// panel's text field). `filter_name` / `filter_ext` wire the
    /// format dropdown — passing `.glb` here lets the OS hide files
    /// with other extensions by default.
    pub fn open(
        &mut self,
        format: ExportFormat,
        outline: OutlineConfig,
        default_name: String,
        filter_name: String,
        filter_ext: String,
    ) {
        if self.inner.is_some() {
            // Extremely unlikely (the UI already guards via
            // `is_pending`), but a stray re-entry shouldn't leak
            // orphan tasks or stack two NSSavePanels.
            log::warn!("export dialog: ignoring open() — a dialog is already pending");
            return;
        }

        let task = AsyncComputeTaskPool::get().spawn(async move {
            // The closures we hand to rfd need owned strings — it
            // stores them across the await boundary.
            let filter_ext = filter_ext;
            let filter_name = filter_name;
            rfd::AsyncFileDialog::new()
                .add_filter(filter_name, &[filter_ext.as_str()])
                .set_file_name(default_name)
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        });

        self.inner = Some(Pending {
            task,
            format,
            outline,
        });
    }
}

/// Drain the pending save-panel task. Emits `ExportRequest` on OK,
/// drops the job on Cancel, and pumps a `RequestRedraw` every frame
/// a dialog is up so the reactive event loop keeps polling instead
/// of waiting for the 5 s heartbeat.
fn poll_pending_export_dialog(
    mut pending: ResMut<PendingExportDialog>,
    mut export_ev: MessageWriter<ExportRequest>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(job) = pending.inner.as_mut() else {
        return;
    };

    // Keep the loop awake while the user is in the native panel —
    // without this the reactive scheduler could sleep for 5 s and
    // the dialog's completion wouldn't be noticed until the next
    // heartbeat tick.
    redraw.write(RequestRedraw);

    let Some(result) = block_on(future::poll_once(&mut job.task)) else {
        return;
    };

    let Pending { format, outline, .. } = pending.inner.take().expect("just polled");

    match result {
        Some(path) => {
            log::info!(
                "export dialog: user picked {} ({:?}) — dispatching ExportRequest",
                path.display(),
                format,
            );
            export_ev.write(ExportRequest(ExportOptions {
                path,
                format,
                outline,
            }));
        }
        None => {
            log::info!("export dialog: user cancelled — no ExportRequest dispatched");
        }
    }
}
