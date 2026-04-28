//! egui panels: menu bar, paint canvas + palette (left), status bar.
//!
//! The canvas uses egui's custom painter to draw a 2D grid and consume
//! pointer events. Left-click paints with the selected palette color;
//! right-click cycles the block shape (placeholder: Cube ↔ Sphere, see
//! [`maquette::grid::ShapeKind`]); Backspace / Delete erases the cell
//! under the cursor. Painting mutates the [`Grid`] resource directly;
//! the mesh rebuild system (in [`crate::grid`]) then picks up the change.

use bevy::color::Hsla;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use maquette::export::{ExportFormat, ExportInProgress, OutlineConfig};
use maquette::grid::{
    DeleteColorMode, Grid, Palette, ShapeKind, DEFAULT_GRID_H, DEFAULT_GRID_W, MAX_GRID, MAX_HEIGHT,
    MIN_GRID, MIN_HEIGHT,
};

use bevy::window::PrimaryWindow;

use crate::block_composer::OpenBlockComposer;
use crate::block_library::{BlockBindAction, BlockLibraryState, SyncBlockLibrary};
use crate::camera::{
    egui_rect, FitPreviewToModel, PreviewViewportRect, ResetPreviewView, ZoomPreview, ZOOM_STEP,
};
use crate::export_dialog::PendingExportDialog;
use crate::float_window::FloatPreviewState;
use crate::history::{EditHistory, HistoryAction, PaintOp};
use crate::multiview::{pip_logical_rects, JumpToOrthoView, MultiViewState};
use crate::scene::WorldAxesState;
use crate::session::{CurrentProject, PendingProjectDialog, ProjectAction};

/// Bundle the outbound message writers for `ui_system` so we stay
/// under Bevy's 16-parameter limit on systems. Every field is a
/// plain `MessageWriter`; grouping them is pure book-keeping.
#[derive(SystemParam)]
pub struct UiMessages<'w> {
    pub project: MessageWriter<'w, ProjectAction>,
    pub reset_view: MessageWriter<'w, ResetPreviewView>,
    pub fit_view: MessageWriter<'w, FitPreviewToModel>,
    pub zoom_view: MessageWriter<'w, ZoomPreview>,
    pub history: MessageWriter<'w, HistoryAction>,
    pub jump_ortho: MessageWriter<'w, JumpToOrthoView>,
    /// Slot ↔ block id binding. UI right-click menus + Block
    /// Library cards write here; `block_library::handle_bind_action`
    /// applies the change through `Palette::set_block_id`.
    pub block_bind: MessageWriter<'w, BlockBindAction>,
    /// Trigger a hfrog catalog re-fetch.
    pub block_sync: MessageWriter<'w, SyncBlockLibrary>,
    /// Open the second-window block composer.
    pub composer_open: MessageWriter<'w, OpenBlockComposer>,
    /// Reactive-rendering wake-up (`WinitSettings::desktop_app()` only
    /// runs `Update` when winit fires an event or someone writes
    /// `RequestRedraw`). After any blocking native dialog we have to
    /// manually kick the loop, otherwise the message we just queued
    /// (Export / Save / Open …) sits idle until the 5 s heartbeat
    /// fires and the user thinks the app hung.
    pub redraw: MessageWriter<'w, bevy::window::RequestRedraw>,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiState>()
            .add_systems(EguiPrimaryContextPass, ui_system);
    }
}

/// Transient UI-only state: modal visibility, hover tooltips, brush
/// parameters, etc. Not persisted, not part of the project format.
#[derive(Resource)]
struct UiState {
    show_about: bool,
    new_project: Option<NewProjectDraft>,
    export_modal: Option<ExportDraft>,
    /// Delete-color confirmation modal (v0.6). The UI opens this from
    /// the swatch's right-click context menu; closing it (Cancel or
    /// Delete) resets to `None`.
    delete_color_modal: Option<DeleteColorDraft>,
    /// Current brush height in cell units. `1..=MAX_HEIGHT`. Carried on
    /// the UI (not the grid) because it's a tool setting, not data.
    brush_height: u8,
    /// Brush application mode. Overwrite replaces the cell wholesale
    /// (the v0.1 behavior); Additive stacks brush height on top of
    /// whatever is already there, preserving the existing color.
    paint_mode: PaintMode,
    /// Cells already hit by the current open stroke. Only consulted
    /// in `PaintMode::Additive` — without it, a drag across one cell
    /// at 60 fps would max out its height in a few milliseconds.
    /// Cleared on every `begin_stroke`.
    stroke_touched: std::collections::HashSet<(usize, usize)>,
}

/// Brush application mode — see `UiState::paint_mode`.
///
/// Exposed as `pub` so the `View → Paint Mode` submenu and the brush
/// bar widget (both in `ui.rs`) can speak the same enum. Default =
/// `Overwrite`, matching the v0.1–v0.8 behavior the user is used to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PaintMode {
    /// Click / drag replaces the cell with `(selected color,
    /// brush height)`. This is the historical default and what
    /// users expect when they want to "recolor" or "reset" a
    /// cell.
    #[default]
    Overwrite,
    /// Click / drag **adds** `brush_height` to the existing cell's
    /// height (clamped at `MAX_HEIGHT`) and keeps the existing
    /// color. Empty cells are painted fresh with the brush color +
    /// height — so a single click on a blank cell behaves the same
    /// under both modes.
    ///
    /// Each cell is only processed once per stroke; without that
    /// guard a 0.5-second drag would stack the cell to `MAX_HEIGHT`
    /// in one gesture.
    Additive,
}

impl PaintMode {
    pub fn label(self) -> &'static str {
        match self {
            PaintMode::Overwrite => "Overwrite",
            PaintMode::Additive => "Additive",
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            show_about: false,
            new_project: None,
            export_modal: None,
            delete_color_modal: None,
            brush_height: MIN_HEIGHT,
            paint_mode: PaintMode::default(),
            stroke_touched: std::collections::HashSet::new(),
        }
    }
}

/// Form state for the "Delete palette color" confirmation modal.
/// Kept alive only while the modal is open. `remap_to` starts as
/// `None` (meaning "erase those cells"); the UI can flip it to a
/// specific live slot to remap instead.
#[derive(Clone, Copy)]
struct DeleteColorDraft {
    idx: u8,
    /// `None` == erase. `Some(to)` == remap to target slot.
    remap_to: Option<u8>,
}

/// Working copy of the export form, kept in UI state while the modal
/// is open. Reset every time the modal is reopened — intentionally not
/// persistent, so stale settings don't surprise the user.
#[derive(Clone)]
struct ExportDraft {
    format: ExportFormat,
    outline_enabled: bool,
    outline_width_pct: f32,
    outline_color: [f32; 3],
}

impl Default for ExportDraft {
    fn default() -> Self {
        let defaults = OutlineConfig::default();
        let c = defaults.color.to_srgba();
        Self {
            format: ExportFormat::Glb,
            outline_enabled: defaults.enabled,
            outline_width_pct: defaults.width_pct,
            outline_color: [c.red, c.green, c.blue],
        }
    }
}

/// Mutable form-state for the "New Project" modal.
#[derive(Clone, Copy)]
struct NewProjectDraft {
    w: usize,
    h: usize,
}

impl Default for NewProjectDraft {
    fn default() -> Self {
        Self {
            w: DEFAULT_GRID_W,
            h: DEFAULT_GRID_H,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn ui_system(
    mut ctx: EguiContexts,
    mut grid: ResMut<Grid>,
    mut palette: ResMut<Palette>,
    mut current: ResMut<CurrentProject>,
    mut ui_state: ResMut<UiState>,
    mut history: ResMut<EditHistory>,
    mut multiview: ResMut<MultiViewState>,
    mut float_state: ResMut<FloatPreviewState>,
    mut axes: ResMut<WorldAxesState>,
    mut preview_viewport: ResMut<PreviewViewportRect>,
    export_state: Res<ExportInProgress>,
    mut pending_export_dialog: ResMut<PendingExportDialog>,
    pending_project_dialog: Res<PendingProjectDialog>,
    library: Res<BlockLibraryState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut msgs: UiMessages,
) -> Result {
    // Local aliases so the rest of the (quite long) function reads
    // the same as before the SystemParam refactor.
    let project_ev = &mut msgs.project;
    let reset_view_ev = &mut msgs.reset_view;
    let fit_view_ev = &mut msgs.fit_view;
    let zoom_view_ev = &mut msgs.zoom_view;
    let history_ev = &mut msgs.history;
    let jump_ortho_ev = &mut msgs.jump_ortho;
    let block_bind_ev = &mut msgs.block_bind;
    let block_sync_ev = &mut msgs.block_sync;
    let composer_open_ev = &mut msgs.composer_open;
    let redraw_ev = &mut msgs.redraw;
    let ctx = ctx.ctx_mut()?;

    handle_shortcuts(
        ctx,
        &mut ui_state,
        &mut palette,
        &mut multiview,
        &mut *project_ev,
        &mut *reset_view_ev,
        &mut *fit_view_ev,
        &mut *zoom_view_ev,
        &mut *history_ev,
    );

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                // A pending native Open / Save / Save As dialog
                // already owns the File-menu I/O path; stacking a
                // second sheet on top is both UX noise and, pre
                // v0.9, a real freeze risk on macOS. We disable the
                // whole File group while it's up rather than
                // pick-and-choose — New… mutates in-memory state
                // the pending dialog's I/O might race with.
                let project_dialog_busy = pending_project_dialog.is_pending();
                let new_btn = ui.add_enabled(
                    !project_dialog_busy,
                    egui::Button::new("New…"),
                );
                if new_btn.clicked() {
                    ui_state.new_project = Some(NewProjectDraft::default());
                    ui.close();
                }
                let open_btn = ui.add_enabled(
                    !project_dialog_busy,
                    egui::Button::new(if project_dialog_busy {
                        "Open… (choosing file)"
                    } else {
                        "Open…"
                    }),
                );
                if open_btn.clicked() {
                    project_ev.write(ProjectAction::Open);
                    ui.close();
                }
                ui.separator();
                let save_btn = ui.add_enabled(
                    !project_dialog_busy,
                    egui::Button::new("Save"),
                );
                if save_btn.clicked() {
                    project_ev.write(ProjectAction::Save);
                    ui.close();
                }
                let save_as_btn = ui.add_enabled(
                    !project_dialog_busy,
                    egui::Button::new(if project_dialog_busy {
                        "Save As… (choosing file)"
                    } else {
                        "Save As…"
                    }),
                );
                if save_as_btn.clicked() {
                    project_ev.write(ProjectAction::SaveAs);
                    ui.close();
                }
                ui.separator();
                // Export is gated on the project being saved *and*
                // clean. Two reasons:
                //   1. `ExportDraft` derives the default output
                //      filename from `current.display_name()` — an
                //      unsaved project would yield `Untitled.glb`
                //      and lose the paper-trail back to the source.
                //   2. After an export users tend to treat the .glb
                //      as the deliverable. If the `.maq` behind it
                //      only lives in memory, a later crash leaves
                //      an orphan model with no way to re-edit it.
                // Explicit save-first is the cheapest invariant
                // that rules both failure modes out.
                let export_needs_save = current.path.is_none() || current.unsaved;
                let export_busy = export_state.is_running() || pending_export_dialog.is_pending();
                let export_disabled = export_busy || export_needs_save;
                let export_label = if export_state.is_running() {
                    "Export… (running)"
                } else if pending_export_dialog.is_pending() {
                    "Export… (choosing file)"
                } else if export_needs_save {
                    "Export… (save project first)"
                } else {
                    "Export…"
                };
                let export_btn = ui.add_enabled(
                    !export_disabled,
                    egui::Button::new(export_label),
                );
                // `on_disabled_hover_text` still fires on a greyed-
                // out button, so the user who hovers to find out
                // *why* it's disabled gets the real reason instead
                // of silence.
                let export_btn = if export_needs_save && !export_busy {
                    export_btn.on_disabled_hover_text(
                        "Save the project first (File → Save). \
                         Exports are named after the saved .maq file — \
                         an unsaved project would produce Untitled.glb \
                         and lose the link back to its source.",
                    )
                } else {
                    export_btn
                };
                if export_btn.clicked() {
                    log::info!("ui: File → Export clicked — opening Export modal");
                    ui_state.export_modal = Some(ExportDraft::default());
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo_btn = ui.add_enabled(history.can_undo(), egui::Button::new("Undo"));
                if undo_btn.clicked() {
                    history_ev.write(HistoryAction::Undo);
                    ui.close();
                }
                let redo_btn = ui.add_enabled(history.can_redo(), egui::Button::new("Redo"));
                if redo_btn.clicked() {
                    history_ev.write(HistoryAction::Redo);
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Paint Mode", |ui| {
                    for mode in [PaintMode::Overwrite, PaintMode::Additive] {
                        if ui
                            .radio(ui_state.paint_mode == mode, mode.label())
                            .on_hover_text(match mode {
                                PaintMode::Overwrite => {
                                    "Replace the cell on every click / drag. (default)"
                                }
                                PaintMode::Additive => {
                                    "Stack the brush height onto the existing cell; \
                                     each cell grows at most once per drag."
                                }
                            })
                            .clicked()
                        {
                            ui_state.paint_mode = mode;
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.small(
                        egui::RichText::new("Shortcut: A to toggle")
                            .color(egui::Color32::from_gray(150)),
                    );
                });
                ui.separator();
                if ui.button("Clear Canvas").clicked() {
                    let size = (grid.w, grid.h);
                    *grid = Grid::with_size(size.0, size.1);
                    history.clear();
                    current.mark_dirty();
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                if ui
                    .button("Reset Preview")
                    .on_hover_text("Snap the preview back to the default angle and zoom.")
                    .clicked()
                {
                    reset_view_ev.write(ResetPreviewView);
                    ui.close();
                }
                if ui
                    .button("Fit to Model")
                    .on_hover_text(
                        "Centre the preview on the painted geometry without changing the angle. \
                         Shortcut: F",
                    )
                    .clicked()
                {
                    fit_view_ev.write(FitPreviewToModel);
                    ui.close();
                }
                ui.separator();
                let mut enabled = multiview.enabled;
                if ui
                    .checkbox(&mut enabled, "Multi-view Preview")
                    .on_hover_text(
                        "Show Top / Front / Side orthographic views in the bottom-right corner. \
                         Toggle with F2.",
                    )
                    .changed()
                {
                    multiview.enabled = enabled;
                    ui.close();
                }
                let mut axes_visible = axes.visible;
                if ui
                    .checkbox(&mut axes_visible, "Show World Axes")
                    .on_hover_text(
                        "Overlay the X (red) / Y (green) / Z (blue) world axes at the \
                         origin of the canvas so you can read orientation at a glance.",
                    )
                    .changed()
                {
                    axes.visible = axes_visible;
                    ui.close();
                }
            });
            ui.menu_button("Window", |ui| {
                if ui
                    .button("New Block Composer…")
                    .on_hover_text(
                        "Open a second window to design a new block: \
                         pick a shape, iterate on a texgen prompt, save \
                         the result locally or publish it to hfrog.",
                    )
                    .clicked()
                {
                    composer_open_ev.write(OpenBlockComposer);
                    redraw_ev.write(bevy::window::RequestRedraw);
                    ui.close();
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About Maquette").clicked() {
                    ui_state.show_about = true;
                    ui.close();
                }
            });
        });
    });

    if let Some(mut draft) = ui_state.new_project {
        let mut open = true;
        let mut submitted = false;
        let mut cancelled = false;
        egui::Window::new("New Project")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Pick a canvas size. You can only set this when creating a project.");
                ui.add_space(8.0);
                egui::Grid::new("new_project_fields")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Width");
                        ui.add(
                            egui::Slider::new(&mut draft.w, MIN_GRID..=MAX_GRID).suffix(" cells"),
                        );
                        ui.end_row();

                        ui.label("Height");
                        ui.add(
                            egui::Slider::new(&mut draft.h, MIN_GRID..=MAX_GRID).suffix(" cells"),
                        );
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() {
                        submitted = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });
        if submitted {
            project_ev.write(ProjectAction::New {
                w: draft.w,
                h: draft.h,
            });
            ui_state.new_project = None;
        } else if cancelled || !open {
            ui_state.new_project = None;
        } else {
            ui_state.new_project = Some(draft);
        }
    }

    if let Some(mut draft) = ui_state.export_modal.clone() {
        let mut open = true;
        let mut confirmed = false;
        let mut cancelled = false;
        egui::Window::new("Export")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Writes a game-engine-ready mesh. No toon shader, no preview-only tricks.");
                ui.add_space(8.0);

                ui.label(egui::RichText::new("Format").strong());
                ui.horizontal(|ui| {
                    ui.radio_value(&mut draft.format, ExportFormat::Glb, ".glb (binary, single file)");
                    ui.radio_value(&mut draft.format, ExportFormat::Gltf, ".gltf (+ .bin, text)");
                });
                ui.small(
                    egui::RichText::new(
                        "GLB is the drop-into-engine default. glTF is text-friendly for git.",
                    )
                    .color(egui::Color32::from_gray(150)),
                );

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label(egui::RichText::new("Outline").strong());
                ui.checkbox(&mut draft.outline_enabled, "Bake an inverted-hull outline mesh");
                ui.add_enabled_ui(draft.outline_enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Width");
                        ui.add(
                            egui::Slider::new(&mut draft.outline_width_pct, 0.0..=10.0)
                                .suffix(" %")
                                .fixed_decimals(1),
                        )
                        .on_hover_text("Percent of the model's bounding diagonal.");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Color");
                        ui.color_edit_button_rgb(&mut draft.outline_color);
                    });
                });
                ui.small(
                    egui::RichText::new(
                        "Outline ships as a second mesh node — delete it in-engine if you don't want it.",
                    )
                    .color(egui::Color32::from_gray(150)),
                );

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Choose file & export").clicked() {
                        confirmed = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            let (ext, filter_name) = match draft.format {
                ExportFormat::Glb => ("glb", "glTF Binary"),
                ExportFormat::Gltf => ("gltf", "glTF JSON"),
            };
            let default_name = format!("{}.{ext}", current.display_name());
            log::info!(
                "ui: opening save dialog for export (.{ext}, outline={})",
                draft.outline_enabled,
            );
            // Hand the native "Save As…" sheet off to the async
            // dispatch pipeline. The old synchronous path wedged
            // on macOS 26 because `NSSavePanel.runModal()` nested
            // under winit's event-loop callback; the async pipeline
            // uses `beginSheetModalForWindow:completionHandler:`,
            // which is the integration Cocoa actually supports from
            // inside another event handler. See
            // `export_dialog.rs` header for the full rationale.
            let [r, g, b] = draft.outline_color;
            pending_export_dialog.open(
                draft.format,
                OutlineConfig {
                    enabled: draft.outline_enabled,
                    width_pct: draft.outline_width_pct,
                    color: Color::srgb(r, g, b),
                },
                default_name,
                filter_name.to_string(),
                ext.to_string(),
            );
            // Kick one redraw so the first `poll_once` happens on
            // the very next frame instead of after the 5 s reactive
            // heartbeat; the poll system keeps the loop awake for
            // the dialog's lifetime after that.
            redraw_ev.write(bevy::window::RequestRedraw);
            ui_state.export_modal = None;
        } else if cancelled || !open {
            log::debug!("ui: Export modal dismissed without confirming");
            ui_state.export_modal = None;
        } else {
            ui_state.export_modal = Some(draft);
        }
    }

    // Progress modal: appears for as long as the async export task
    // is alive. We render it *outside* the export_modal block so that
    // closing the options dialog (which happens the same frame we
    // dispatch the request) doesn't tear down the progress UI.
    if export_state.is_running() {
        let path_label = export_state
            .current_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(unknown path)".to_string());
        let elapsed = export_state.elapsed();
        egui::Window::new("Exporting…")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(18.0));
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Writing mesh…").strong(),
                        );
                        ui.small(
                            egui::RichText::new(&path_label)
                                .color(egui::Color32::from_gray(170)),
                        );
                        ui.small(format!("elapsed {:.1}s", elapsed.as_secs_f32()));
                    });
                });
                ui.add_space(4.0);
                ui.small(
                    egui::RichText::new(
                        "Window stays responsive — the UI keeps drawing \
                         while the export runs on a background thread.",
                    )
                    .color(egui::Color32::from_gray(150)),
                );
            });
    }

    delete_color_modal(ctx, &mut ui_state, &mut grid, &mut palette, &mut current);

    if ui_state.show_about {
        let mut open = ui_state.show_about;
        egui::Window::new("About Maquette")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Maquette — a block-style asset editor with a toon look.");
                ui.add_space(6.0);
                ui.label(
                    "Paint on the 2D canvas on the left; the 3D preview on the right updates in real time.",
                );
                ui.add_space(6.0);
                ui.label("• Left-click a cell to paint with the selected color.");
                ui.label("• Right-click a painted cell to cycle its shape (Cube ↔ Sphere).");
                ui.label("• Hover a cell and press Backspace / Delete to erase it.");
                ui.label("• Drag the preview to turn the model; scroll to zoom.");
                ui.add_space(6.0);
                ui.label("File → Save writes a .maq project you can reopen later.");
                ui.label("File → Export writes a .glb / .gltf you can drop into any engine.");
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Preview is not the export.").strong(),
                );
                ui.label(
                    "The toon shading and outlines you see here are for the editor view only. \
                     Exports ship plain geometry with vertex color plus an optional outline \
                     mesh that any engine (Godot, Unity, Blender) can render as-is.",
                );
            });
        ui_state.show_about = open;
    }

    // Layout decision (2026-04-28): the Block Library used to live
    // in a right SidePanel, but right-side anchored floats — preview
    // toolbar, PIP labels, toasts — kept colliding with it visually
    // (and even the post-fix layout left the central 3-D area
    // squeezed). Solution: stack everything on the left side. The
    // left SidePanel now carries Canvas + Palette + a *collapsible*
    // Block Library drawer at the bottom; the entire right half of
    // the window goes back to being the 3-D preview + PIPs + toolbar.
    let canvas_rect = egui::SidePanel::left("canvas_panel")
        .default_width(520.0)
        .min_width(360.0)
        .show(ctx, |ui| {
            ui.heading("Canvas");
            ui.separator();
            let rect = paint_canvas(
                ui,
                &mut grid,
                &palette,
                &mut current,
                &mut history,
                &mut ui_state,
            );
            ui.add_space(10.0);
            ui.separator();
            palette_bar(ui, &mut palette, &mut ui_state, &library, &mut *block_bind_ev);
            ui.add_space(10.0);
            ui.separator();
            // Drawer style: default open so the user sees their
            // catalog at a glance; collapsing it reclaims the
            // bottom of the left column for the canvas above.
            egui::CollapsingHeader::new(
                egui::RichText::new(format!("Block Library  ({})", library.blocks.len()))
                    .strong(),
            )
            .id_salt("block_library_drawer")
            .default_open(true)
            .show(ui, |ui| {
                block_library_drawer(
                    ui,
                    &palette,
                    &library,
                    &mut *block_bind_ev,
                    &mut *block_sync_ev,
                    &mut *redraw_ev,
                );
            });
            rect
        })
        .inner;

    // Brush tools float on top of the canvas in its top-left
    // corner, Blender-style, instead of living at the bottom of
    // the sidebar. Anchoring to `canvas_rect.min` means the
    // overlay tracks the canvas when the user resizes the left
    // panel.
    brush_overlay(ctx, canvas_rect, &mut ui_state);

    // Snapshot the central area *before* any floating overlay
    // (PIP labels, preview toolbar, toasts) draws — this is what's
    // left after every SidePanel + TopBottomPanel claimed its slice.
    // Right-anchored overlays must read this and *not*
    // `ctx.screen_rect()`, otherwise they sit on top of the right
    // SidePanel (Block Library) — the v0.10 C-2 regression that
    // produced the "right side is chaos" report.
    let central = ctx.available_rect();

    if multiview.enabled {
        if let Ok(window) = windows.single() {
            paint_pip_labels(ctx, window, &multiview, central, &mut *jump_ortho_ev);
        }
    }

    preview_toolbar(
        ctx,
        central,
        &mut *reset_view_ev,
        &mut *fit_view_ev,
        &mut *zoom_view_ev,
        &mut multiview,
        &mut float_state,
    );

    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
        let painted = grid.painted_count();
        let total = grid.cells.len();
        let dirty_mark = if current.unsaved { "•" } else { " " };
        ui.horizontal(|ui| {
            ui.label(format!("{dirty_mark} {}", current.display_name()));
            ui.separator();
            ui.label(format!("Painted: {painted} / {total}"));
            ui.separator();
            let paint_verb = match ui_state.paint_mode {
                PaintMode::Overwrite => "paint (overwrite)",
                PaintMode::Additive => "paint (stack +)",
            };
            ui.label(format!(
                "Left-click: {paint_verb}  •  Right-click: cycle shape  •  Del/Back: erase hovered  •  A: toggle mode  •  Drag preview: turn"
            ));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label("Maquette · v0.8-dev");
            });
        });
    });

    // Report the "empty central area" (= everything the panels above
    // didn't claim) to the camera plugin so the 3D preview viewport
    // lines up with what the user sees. `available_rect` is computed
    // after every SidePanel / TopBottomPanel runs, so by this point
    // it reflects the usable preview region.
    let avail = ctx.available_rect();
    preview_viewport.rect = Some(egui_rect::Rect {
        min_x: avail.min.x,
        min_y: avail.min.y,
        max_x: avail.max.x,
        max_y: avail.max.y,
    });

    Ok(())
}

fn paint_canvas(
    ui: &mut egui::Ui,
    grid: &mut Grid,
    palette: &Palette,
    current: &mut CurrentProject,
    history: &mut EditHistory,
    ui_state: &mut UiState,
) -> egui::Rect {
    let brush_height = ui_state.brush_height;
    let paint_mode = ui_state.paint_mode;
    let available = ui.available_size_before_wrap();
    let budget = available.x.min(available.y).min(520.0);
    let cell_px = (budget / grid.w.max(grid.h) as f32).floor().max(6.0);
    let canvas_size = egui::vec2(cell_px * grid.w as f32, cell_px * grid.h as f32);

    let (rect, response) = ui.allocate_exact_size(canvas_size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);

    painter.rect_filled(rect, 0.0, egui::Color32::from_gray(18));

    for y in 0..grid.h {
        for x in 0..grid.w {
            let cell_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(x as f32 * cell_px, y as f32 * cell_px),
                egui::vec2(cell_px, cell_px),
            );
            if let Some(cell) = grid.get(x, y) {
                if let Some(ci) = cell.color_idx {
                    if let Some(fill) = palette.get(ci) {
                        painter.rect_filled(cell_rect, 0.0, to_egui_color(fill));
                        // Shape indicator — glyph on top of the
                        // colored square so the 2D canvas reads
                        // the same shape the 3D preview ships.
                        // Kept deliberately minimal (single stroke,
                        // no fill) so the underlying palette
                        // swatch is still unambiguous.
                        draw_shape_glyph(&painter, cell_rect, cell.shape);
                        // Height badge (top-left corner). Only
                        // shows for stacked cells — height == 1 is
                        // the base case and would add noise to
                        // 100 % of a freshly painted canvas.
                        draw_height_badge(&painter, cell_rect, cell.height);
                    }
                }
            }
            painter.rect_stroke(
                cell_rect,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::from_gray(38)),
                egui::epaint::StrokeKind::Middle,
            );
        }
    }

    // Empty-state onboarding. Fires only when the canvas holds no
    // painted cells — the moment the user lands their first click
    // it vanishes and never comes back until `File → New` or
    // `Clear Canvas`. Static panel, not a tutorial popup, so
    // veteran users aren't forced through a dialog they've seen
    // before.
    if grid.painted_count() == 0 {
        paint_empty_canvas_hint(&painter, rect);
    }

    // Pointer-down on the canvas opens a stroke; pointer-up closes
    // it. Every `history.record` call in between appends to that
    // stroke, so a drag across 30 cells undoes in a single Ctrl+Z
    // (see `history::Stroke`).
    if response.drag_started_by(egui::PointerButton::Primary)
        || response.drag_started_by(egui::PointerButton::Secondary)
    {
        history.begin_stroke();
        // Reset the per-stroke "already processed" set. Used by both
        //   * `PaintMode::Additive` left-click (stack at most once
        //     per cell per drag), and
        //   * right-click shape cycling (cycle at most once per cell
        //     per drag — without it a 0.5s hover would cycle a cell
        //     through every shape at 60 Hz).
        ui_state.stroke_touched.clear();
    }

    // Left button → paint. Right button → cycle block shape
    // (placeholder: Cube ↔ Sphere). Erase is keyboard-only
    // (Backspace / Delete below) since right-click now has a
    // higher-value job.
    let paint = response.dragged_by(egui::PointerButton::Primary)
        || response.clicked_by(egui::PointerButton::Primary);
    let shape_cycle = response.dragged_by(egui::PointerButton::Secondary)
        || response.clicked_by(egui::PointerButton::Secondary);

    if paint || shape_cycle {
        if let Some(pos) = response.interact_pointer_pos() {
            let local = pos - rect.min;
            if local.x >= 0.0 && local.y >= 0.0 {
                let gx = (local.x / cell_px) as usize;
                let gy = (local.y / cell_px) as usize;
                if gx < grid.w && gy < grid.h {
                    let change = if paint {
                        apply_paint(
                            grid,
                            gx,
                            gy,
                            palette.selected,
                            brush_height,
                            paint_mode,
                            &mut ui_state.stroke_touched,
                        )
                    } else {
                        // Same per-stroke guard keeps a drag from
                        // re-cycling the same cell. One cell per
                        // gesture, same as Additive paint.
                        if !ui_state.stroke_touched.insert((gx, gy)) {
                            None
                        } else {
                            grid.cycle_shape(gx, gy)
                        }
                    };
                    if let Some((before, after)) = change {
                        history.record(PaintOp {
                            x: gx,
                            y: gy,
                            before,
                            after,
                        });
                        current.mark_dirty();
                    }
                }
            }
        }
    }

    // Delete / Backspace erases the cell under the cursor. Keyboard
    // erase is deliberately *not* stroke-grouped across frames —
    // each key press is its own undo entry, since the user is
    // unlikely to want to Ctrl+Z all their deletes at once.
    //
    // Note: `input_mut` + `consume_key` so the key event doesn't
    // leak to other widgets (e.g. a future name field or egui text
    // input that might want Backspace for its own purposes).
    if let Some(hover) = response.hover_pos() {
        let local = hover - rect.min;
        if local.x >= 0.0 && local.y >= 0.0 {
            let gx = (local.x / cell_px) as usize;
            let gy = (local.y / cell_px) as usize;
            if gx < grid.w && gy < grid.h {
                let erase_key = ui.ctx().input_mut(|i| {
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                        || i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                });
                if erase_key {
                    if let Some((before, after)) = grid.erase(gx, gy) {
                        history.begin_stroke();
                        history.record(PaintOp {
                            x: gx,
                            y: gy,
                            before,
                            after,
                        });
                        history.end_stroke();
                        current.mark_dirty();
                    }
                }
            }
        }
    }

    // Close the stroke when the pointer is released.
    //
    // A "stroke" ends either on drag release OR on a plain click
    // (no drag). We used to listen only to `drag_stopped_by`, which
    // left `stroke_touched` stale after a single click — the second
    // click on the same cell would hit the "already touched this
    // stroke" guard in `apply_paint` / `cycle_shape` and silently
    // no-op. Symptoms: Additive paint refusing to stack past the
    // first +1, right-click refusing to cycle Cube→Sphere after the
    // cell was already painted, etc.
    //
    // `end_stroke` is safe against missing matching `begin_stroke`
    // (see `history::end_stroke` — it just `take`s the Option).
    if response.drag_stopped_by(egui::PointerButton::Primary)
        || response.drag_stopped_by(egui::PointerButton::Secondary)
        || response.clicked_by(egui::PointerButton::Primary)
        || response.clicked_by(egui::PointerButton::Secondary)
    {
        history.end_stroke();
        ui_state.stroke_touched.clear();
    }

    rect
}

/// Overlay a small shape-indicator glyph on top of a painted cell
/// so the 2D canvas matches the 3D preview. Cube is the default
/// shape and intentionally *unmarked* — the colored square itself
/// is the Cube affordance, and a glyph on every cell would add
/// visual noise for no information. Non-cube shapes get a single-
/// stroke overlay sized to fit inside the cell with a comfortable
/// margin.
///
/// Stroke colours: black outer ring to match the 3D cel-shader
/// outline (so the 2D glyph reads as "same shape, top-down view"),
/// plus a thin inner white ring so the glyph stays visible on
/// dark palette colours.
fn draw_shape_glyph(painter: &egui::Painter, cell_rect: egui::Rect, shape: ShapeKind) {
    match shape {
        ShapeKind::Cube => {}
        ShapeKind::Sphere => {
            let side = cell_rect.width().min(cell_rect.height());
            // Skip glyph on tiny cells — below ~10px the ring
            // becomes indistinguishable from noise. Users on
            // 128×128 canvases will just have to rely on the 3D
            // preview. Paint cells usually stay well above this
            // floor.
            if side < 10.0 {
                return;
            }
            let radius = side * 0.38;
            let centre = cell_rect.center();
            painter.circle_stroke(
                centre,
                radius,
                egui::Stroke::new(1.6, egui::Color32::from_black_alpha(220)),
            );
            painter.circle_stroke(
                centre,
                radius - 1.4,
                egui::Stroke::new(0.9, egui::Color32::from_white_alpha(140)),
            );
        }
    }
}

/// Paint a small height-count badge in the top-left corner of
/// `cell_rect`. Only shown for stacked cells (`height >= 2`) —
/// displaying "1" on every freshly-painted cell would add visual
/// noise without conveying new information, since `height == 1`
/// is the default brush value.
///
/// Rendered as a dark rounded pill with white text inside so the
/// badge stays legible over any palette colour. The pill is
/// deliberately small (font ≈ cell × 0.42, clamped to the 8–14 px
/// range) to fit comfortably even on tight 16-px cells — on
/// anything smaller we give up and skip the badge, which is also
/// where the shape glyph bails out.
fn draw_height_badge(painter: &egui::Painter, cell_rect: egui::Rect, height: u8) {
    if height < 2 {
        return;
    }
    let side = cell_rect.width().min(cell_rect.height());
    if side < 12.0 {
        return;
    }
    let text = format!("{height}");
    let font_size = (side * 0.42).clamp(8.0, 14.0);
    // First measure so we can size the pill to the digits (a "10"
    // is visibly wider than a "2" and a fixed background would
    // either crop the former or float over the latter).
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(font_size),
        egui::Color32::WHITE,
    );
    let pad = egui::vec2(3.0, 1.0);
    let badge_size = galley.size() + pad * 2.0;
    let origin = cell_rect.min + egui::vec2(2.0, 2.0);
    let badge_rect = egui::Rect::from_min_size(origin, badge_size);
    painter.rect_filled(badge_rect, 3.0, egui::Color32::from_black_alpha(200));
    painter.galley(origin + pad, galley, egui::Color32::WHITE);
}

/// Apply one paint tick to `(gx, gy)` according to `mode`.
///
/// Returns `Some((before, after))` if the cell actually changed, so
/// the caller can push a single undo entry. `None` means either the
/// cell already equals the target state, or `Additive` mode has
/// already processed this cell in the current stroke (tracked via
/// `stroke_touched`).
fn apply_paint(
    grid: &mut Grid,
    gx: usize,
    gy: usize,
    selected_color: u8,
    brush_height: u8,
    mode: PaintMode,
    stroke_touched: &mut std::collections::HashSet<(usize, usize)>,
) -> Option<(maquette::grid::Cell, maquette::grid::Cell)> {
    match mode {
        PaintMode::Overwrite => grid.paint(gx, gy, selected_color, brush_height),
        PaintMode::Additive => {
            // Guard: each cell can only grow once per stroke. A
            // drag at 60 fps hits the same cell many times; without
            // this, the user would see their 3-cell stroke balloon
            // to MAX_HEIGHT in half a second.
            if !stroke_touched.insert((gx, gy)) {
                return None;
            }
            let existing = grid.get(gx, gy).copied().unwrap_or_default();
            let (color, height) = match existing.color_idx {
                Some(existing_color) => {
                    // Already painted: keep its color, stack
                    // height. `u16` intermediate so the sum never
                    // wraps before the `MAX_HEIGHT` clamp.
                    let stacked = (existing.height as u16 + brush_height as u16)
                        .min(MAX_HEIGHT as u16) as u8;
                    (existing_color, stacked)
                }
                None => {
                    // Empty cell — Additive collapses to a fresh
                    // paint so a single click on blank canvas
                    // still does something sensible.
                    (selected_color, brush_height)
                }
            };
            grid.paint(gx, gy, color, height)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_shortcuts(
    ctx: &egui::Context,
    ui_state: &mut UiState,
    palette: &mut Palette,
    multiview: &mut MultiViewState,
    project_ev: &mut MessageWriter<ProjectAction>,
    reset_view_ev: &mut MessageWriter<ResetPreviewView>,
    fit_view_ev: &mut MessageWriter<FitPreviewToModel>,
    zoom_view_ev: &mut MessageWriter<ZoomPreview>,
    history_ev: &mut MessageWriter<HistoryAction>,
) {
    use egui::{Key, KeyboardShortcut, Modifiers};

    // `Modifiers::COMMAND` is Cmd on macOS and Ctrl elsewhere — the right
    // thing for file/edit shortcuts on every platform.
    let cmd = Modifiers::COMMAND;
    let cmd_shift = cmd | Modifiers::SHIFT;

    ctx.input_mut(|i| {
        if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::N)) {
            ui_state.new_project = Some(NewProjectDraft::default());
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::O)) {
            project_ev.write(ProjectAction::Open);
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::S)) {
            project_ev.write(ProjectAction::Save);
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd_shift, Key::S)) {
            project_ev.write(ProjectAction::SaveAs);
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Z)) {
            history_ev.write(HistoryAction::Undo);
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd_shift, Key::Z))
            || i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Y))
        {
            history_ev.write(HistoryAction::Redo);
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::R)) {
            reset_view_ev.write(ResetPreviewView);
        }
        // Plain-F: "frame / fit to model". Matches the convention in
        // Blender, Maya, and most DCCs and is the one-handed shortcut
        // the user wants while painting with the other hand.
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F)) {
            fit_view_ev.write(FitPreviewToModel);
        }
        // +/− zoom the preview one step. Accept both the standalone
        // Plus/Minus keys and the Equals key (which shares a
        // glyph with + on US layouts without requiring Shift), so
        // either shoulder-tap works without reaching for a modifier.
        let zoom_in = i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Plus))
            || i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Equals));
        if zoom_in {
            zoom_view_ev.write(ZoomPreview {
                factor: 1.0 / ZOOM_STEP,
            });
        }
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::Minus)) {
            zoom_view_ev.write(ZoomPreview { factor: ZOOM_STEP });
        }
        // F2 toggles the multi-view PIP overlay. Plain F-key (no
        // cmd) because it's a viewport preference, not a document
        // action, and should feel as light as tab-switching.
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F2)) {
            multiview.enabled = !multiview.enabled;
        }
        // Plain-A swaps between Overwrite and Additive paint modes.
        // Ungated (no Cmd) because this is a tool, not a document
        // action, and the muscle-memory should match "hit A to
        // alternate" rather than a chord.
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::A)) {
            ui_state.paint_mode = match ui_state.paint_mode {
                PaintMode::Overwrite => PaintMode::Additive,
                PaintMode::Additive => PaintMode::Overwrite,
            };
        }
        if i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::E))
            && ui_state.export_modal.is_none()
        {
            ui_state.export_modal = Some(ExportDraft::default());
        }

        // Digit keys 1..=9 select the nth palette swatch. `consume_key`
        // returns false when a text widget owns the event, so we don't
        // need an explicit focus guard here.
        for (key, idx) in [
            (Key::Num1, 0usize),
            (Key::Num2, 1),
            (Key::Num3, 2),
            (Key::Num4, 3),
            (Key::Num5, 4),
            (Key::Num6, 5),
            (Key::Num7, 6),
            (Key::Num8, 7),
            (Key::Num9, 8),
        ] {
            // Map the nth number key to the nth *live* color, so
            // deleted holes don't produce dead shortcuts.
            if i.consume_key(Modifiers::NONE, key) {
                let slot = palette.iter_live().nth(idx).map(|(i, _)| i);
                if let Some(slot_idx) = slot {
                    palette.selected = slot_idx;
                }
            }
        }
    });
}

fn palette_bar(
    ui: &mut egui::Ui,
    palette: &mut Palette,
    ui_state: &mut UiState,
    library: &BlockLibraryState,
    block_bind_ev: &mut MessageWriter<BlockBindAction>,
) {
    ui.label(egui::RichText::new("Palette").strong());
    ui.add_space(2.0);
    // Compact swatches — 22 px is the smallest size where a 1.5 px
    // selection ring + a 3 px draft/block-binding badge still read
    // cleanly. Smaller and the badge swallows the colour itself.
    let original_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
    ui.horizontal_wrapped(|ui| {
        let swatch_size = egui::vec2(22.0, 22.0);

        // Snapshot the live slots upfront so we can mutably borrow
        // `palette` inside per-slot callbacks (context menus, the
        // color editor) without fighting the iterator's immutable
        // borrow.
        let live: Vec<(u8, Color)> = palette.iter_live().collect();

        for (slot_idx, color) in live {
            let (rect, response) =
                ui.allocate_exact_size(swatch_size, egui::Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 4.0, to_egui_color(color));

            let is_selected = palette.selected == slot_idx;
            if is_selected {
                painter.rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(2.5, egui::Color32::WHITE),
                    egui::epaint::StrokeKind::Outside,
                );
            } else if response.hovered() {
                painter.rect_stroke(
                    rect,
                    4.0,
                    egui::Stroke::new(1.5, egui::Color32::from_gray(180)),
                    egui::epaint::StrokeKind::Outside,
                );
            }

            if response.clicked() {
                palette.selected = slot_idx;
            }

            let response = response.on_hover_text(format!(
                "Slot #{slot_idx} · left-click to select · right-click to edit / delete"
            ));

            // Right-click context menu: inline color picker + Delete.
            // We pass the *current* color (copied above) into
            // `color_edit_button_srgb`; if the user tweaks it, we
            // write back via `palette.update` so the change persists
            // after the menu closes. `Delete…` opens a modal in the
            // main `ui_system` so the confirmation UI has full access
            // to grid state (for showing "N cells affected").
            //
            // egui renamed `.context_menu` in the 0.3x series; if a
            // future bevy_egui bump breaks this call site, swap to
            // `response.context_menu_with_ctx` without changing the
            // body.
            // Snapshot any current block binding for this slot
            // before we hand `palette` into the closure mutably.
            let bound_block_id = palette
                .meta(slot_idx)
                .and_then(|m| m.block_id.as_deref().map(|s| s.to_string()));
            response.context_menu(|ui| {
                ui.label(format!("Slot #{slot_idx}"));
                let mut rgb = [
                    color.to_srgba().red,
                    color.to_srgba().green,
                    color.to_srgba().blue,
                ];
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    palette.update(slot_idx, Color::srgb(rgb[0], rgb[1], rgb[2]));
                }
                ui.separator();

                // Block binding: nested submenu showing every known
                // block id (local + cached hfrog), with the current
                // binding marked.
                let bind_label = match bound_block_id.as_deref() {
                    Some(id) => format!("Block: {id}"),
                    None => "Bind block…".to_string(),
                };
                ui.menu_button(bind_label, |ui| {
                    if library.blocks.is_empty() {
                        ui.label(
                            egui::RichText::new("(no blocks — sync hfrog or rebuild)")
                                .small()
                                .italics(),
                        );
                    }
                    for b in &library.blocks {
                        let is_current = bound_block_id.as_deref() == Some(&b.id);
                        let label = if is_current {
                            format!("✔ {} · {}", b.id, b.source.label())
                        } else {
                            format!("  {} · {}", b.id, b.source.label())
                        };
                        if ui
                            .button(label)
                            .on_hover_text(&b.description)
                            .clicked()
                        {
                            block_bind_ev.write(BlockBindAction::Bind {
                                slot: slot_idx,
                                block_id: b.id.clone(),
                            });
                            ui.close();
                        }
                    }
                    if bound_block_id.is_some() {
                        ui.separator();
                        if ui.button("Unbind").clicked() {
                            block_bind_ev.write(BlockBindAction::Unbind { slot: slot_idx });
                            ui.close();
                        }
                    }
                });

                ui.separator();
                if ui.button("Delete…").clicked() {
                    ui_state.delete_color_modal = Some(DeleteColorDraft {
                        idx: slot_idx,
                        remap_to: None,
                    });
                    ui.close();
                }
            });

            // Bottom-right corner badge "B" if a block is bound —
            // visible at-a-glance, no hover. Sized to read at the
            // 22 px swatch without dominating the colour fill.
            if bound_block_id.is_some() {
                let badge_radius = 3.0;
                let badge_pos = rect.right_bottom()
                    - egui::vec2(badge_radius + 1.5, badge_radius + 1.5);
                painter.circle_filled(badge_pos, badge_radius, egui::Color32::from_rgb(120, 180, 255));
            }
        }

        // "+" slot. Adds a new color — hue-shifted from the currently
        // selected color so the new swatch is visually distinct but
        // stays in the same colour family. Falls back to mid-grey if
        // the palette is empty (fresh project or everything deleted).
        let (add_rect, add_response) =
            ui.allocate_exact_size(swatch_size, egui::Sense::click());
        let painter = ui.painter_at(add_rect);
        painter.rect_filled(add_rect, 4.0, egui::Color32::from_gray(40));
        painter.rect_stroke(
            add_rect,
            4.0,
            egui::Stroke::new(
                1.5,
                if add_response.hovered() {
                    egui::Color32::from_gray(200)
                } else {
                    egui::Color32::from_gray(120)
                },
            ),
            egui::epaint::StrokeKind::Inside,
        );
        painter.text(
            add_rect.center(),
            egui::Align2::CENTER_CENTER,
            "+",
            egui::FontId::proportional(14.0),
            egui::Color32::from_gray(220),
        );
        add_response
            .on_hover_text("Add a new palette color")
            .clicked()
            .then(|| {
                let base = palette
                    .get(palette.selected)
                    .unwrap_or(Color::srgb(0.5, 0.5, 0.5));
                let hsla = Hsla::from(base);
                let new_color = Color::from(Hsla {
                    hue: (hsla.hue + 45.0).rem_euclid(360.0),
                    ..hsla
                });
                if let Some(new_idx) = palette.add(new_color) {
                    palette.selected = new_idx;
                }
            });
    });
    // Restore the surrounding panel's spacing for whatever lays
    // out next.
    ui.spacing_mut().item_spacing = original_spacing;
}

/// Delete-color confirmation modal. Called once per frame from
/// `ui_system`; renders only when `ui_state.delete_color_modal` is
/// `Some(_)`. Owns the modal's own open/close lifecycle.
fn delete_color_modal(
    ctx: &egui::Context,
    ui_state: &mut UiState,
    grid: &mut Grid,
    palette: &mut Palette,
    current: &mut CurrentProject,
) {
    let Some(mut draft) = ui_state.delete_color_modal.take() else {
        return;
    };
    let mut decision: Option<bool> = None; // Some(true)=delete, Some(false)=cancel
    let mut open = true;

    egui::Window::new("Delete palette color")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut open)
        .show(ctx, |ui| {
            let Some(color) = palette.get(draft.idx) else {
                ui.label("That slot no longer exists.");
                if ui.button("Close").clicked() {
                    decision = Some(false);
                }
                return;
            };

            ui.horizontal(|ui| {
                ui.label(format!("Slot #{}", draft.idx));
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(32.0, 20.0), egui::Sense::hover());
                ui.painter_at(rect).rect_filled(rect, 3.0, to_egui_color(color));
            });

            let usage = palette.usage_count(grid, draft.idx);
            if usage > 0 {
                ui.label(format!("{usage} cell(s) currently use this color."));
            } else {
                ui.label("No cells currently use this color.");
            }
            ui.separator();
            ui.label("What should happen to those cells?");

            let is_erase = draft.remap_to.is_none();
            if ui.radio(is_erase, "Erase — cells become empty").clicked() {
                draft.remap_to = None;
            }

            let alternatives: Vec<(u8, Color)> = palette
                .iter_live()
                .filter(|(i, _)| *i != draft.idx)
                .collect();

            ui.horizontal(|ui| {
                let is_remap = draft.remap_to.is_some();
                let enabled = !alternatives.is_empty();
                ui.add_enabled_ui(enabled, |ui| {
                    if ui.radio(is_remap, "Remap to:").clicked() && draft.remap_to.is_none() {
                        draft.remap_to = alternatives.first().map(|(i, _)| *i);
                    }
                });
                if let Some(current_target) = draft.remap_to {
                    egui::ComboBox::from_id_salt(("delete_color_target", draft.idx))
                        .selected_text(format!("#{current_target}"))
                        .show_ui(ui, |ui| {
                            for (i, _) in &alternatives {
                                ui.selectable_value(&mut draft.remap_to, Some(*i), format!("#{i}"));
                            }
                        });
                    if let Some(c) = palette.get(current_target) {
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(20.0, 20.0),
                            egui::Sense::hover(),
                        );
                        ui.painter_at(rect).rect_filled(rect, 3.0, to_egui_color(c));
                    }
                }
                if alternatives.is_empty() {
                    ui.small(
                        egui::RichText::new("(no other colors to remap into)")
                            .color(egui::Color32::from_gray(150)),
                    );
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Delete").color(egui::Color32::from_rgb(240, 100, 100)),
                    ))
                    .clicked()
                {
                    decision = Some(true);
                }
            });
        });

    if !open {
        decision = Some(false);
    }

    match decision {
        Some(true) => {
            let mode = match draft.remap_to {
                Some(to) => DeleteColorMode::Remap { to },
                None => DeleteColorMode::Erase,
            };
            if palette.delete(draft.idx, grid, mode) {
                current.mark_dirty();
            }
        }
        Some(false) => { /* drop the draft → close the modal */ }
        None => {
            // No decision this frame → keep the draft alive so the
            // modal reappears next frame in the same state.
            ui_state.delete_color_modal = Some(draft);
        }
    }
}

/// Block Library drawer that hangs off the bottom of the left
/// SidePanel inside an `egui::CollapsingHeader`. Lists every block
/// known to `BlockLibraryState` (LocalProvider + cached hfrog +
/// LocalDraft via `LocalDraftProvider`), shows source / color /
/// texture-hint per row, and surfaces the `Sync hfrog` button on
/// top.
///
/// Earlier revisions (v0.10 C-2) lived in a right SidePanel; the
/// refactor to "drawer in the left column" came after the
/// preview-toolbar / PIP / toast collisions reported on the main
/// editor screenshot. Keeping all panels on the left lets the right
/// half of the window be 100 % 3-D + its right-anchored floats.
///
/// Read-only on the resource — actual mutations go through the
/// `BlockBindAction` / `SyncBlockLibrary` messages handled in
/// `block_library.rs`. Keeps this drawer a pure projection of state.
fn block_library_drawer(
    ui: &mut egui::Ui,
    palette: &Palette,
    library: &BlockLibraryState,
    block_bind_ev: &mut MessageWriter<BlockBindAction>,
    block_sync_ev: &mut MessageWriter<SyncBlockLibrary>,
    redraw_ev: &mut MessageWriter<bevy::window::RequestRedraw>,
) {
    // Heading is owned by the outer `CollapsingHeader`, so we
    // dive straight into the action row.
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !library.is_in_flight(),
                egui::Button::new(if library.is_in_flight() {
                    "Syncing…"
                } else {
                    "Sync hfrog"
                }),
            )
            .on_hover_text(
                "Pull every maquette-block/v1 record from the hfrog \
                 artifact server (default: \
                 https://starlink.youxi123.com/hfrog) and merge with \
                 the bundled local set. Override the URL with \
                 MAQUETTE_HFROG_BASE_URL.",
            )
            .clicked()
        {
            block_sync_ev.write(SyncBlockLibrary);
            redraw_ev.write(bevy::window::RequestRedraw);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} blocks", library.blocks.len()))
                    .small(),
            );
        });
    });
    if let Some(err) = &library.last_error {
        ui.label(
            egui::RichText::new(format!("Last sync failed: {err}"))
                .small()
                .color(egui::Color32::from_rgb(220, 90, 90)),
        );
    }
    ui.separator();

    // Currently-selected slot. We highlight the bound block (if any)
    // and let the user re-bind without leaving the panel.
    let selected_slot = palette.selected;
    let bound_block_id = palette
        .meta(selected_slot)
        .and_then(|m| m.block_id.as_deref().map(|s| s.to_string()));
    ui.label(
        egui::RichText::new(format!(
            "Selected slot: #{selected_slot}{}",
            match bound_block_id.as_deref() {
                Some(id) => format!(" (bound to {id})"),
                None => String::new(),
            }
        ))
        .small(),
    );
    ui.add_space(4.0);

    // Cap the scroll viewport so the canvas above isn't pushed
    // off-screen on a tall library: 300 logical px ≈ 4 cards. The
    // canvas above will still get visible scroll bars on a small
    // window — that's intentional, the canvas itself is the most
    // important thing in the editor.
    egui::ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(300.0)
        .show(ui, |ui| {
            if library.blocks.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("(library is empty)").italics(),
                    );
                    ui.label(
                        egui::RichText::new("Click Sync hfrog or rebuild — the bundled set should never be empty in production.")
                            .small(),
                    );
                });
                return;
            }
            // Tighten vertical spacing inside the list so each row
            // feels like a single-line entry rather than a card.
            ui.spacing_mut().item_spacing.y = 2.0;
            for b in &library.blocks {
                let is_current_for_selected =
                    bound_block_id.as_deref() == Some(&b.id);
                let row_resp = ui.horizontal(|ui| {
                    // 16 px swatch — readable at-a-glance, doesn't
                    // dominate the row.
                    let (sw_rect, _) = ui.allocate_exact_size(
                        egui::vec2(16.0, 16.0),
                        egui::Sense::hover(),
                    );
                    let painter = ui.painter_at(sw_rect);
                    let c = b.default_color;
                    painter.rect_filled(
                        sw_rect,
                        3.0,
                        egui::Color32::from_rgba_unmultiplied(
                            (c.r.clamp(0.0, 1.0) * 255.0) as u8,
                            (c.g.clamp(0.0, 1.0) * 255.0) as u8,
                            (c.b.clamp(0.0, 1.0) * 255.0) as u8,
                            255,
                        ),
                    );
                    if is_current_for_selected {
                        painter.rect_stroke(
                            sw_rect,
                            3.0,
                            egui::Stroke::new(
                                1.5,
                                egui::Color32::from_rgb(120, 180, 255),
                            ),
                            egui::epaint::StrokeKind::Outside,
                        );
                    }
                    // Name + id · source on a single line.
                    ui.label(
                        egui::RichText::new(b.label()).strong(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · {}",
                            b.id,
                            b.source.label()
                        ))
                        .small()
                        .color(egui::Color32::from_gray(150)),
                    );
                    // Right-aligned action button so labels don't
                    // jitter when the button text changes.
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if is_current_for_selected {
                                if ui
                                    .small_button("Unbind")
                                    .on_hover_text("Clear this slot's block binding")
                                    .clicked()
                                {
                                    block_bind_ev.write(BlockBindAction::Unbind {
                                        slot: selected_slot,
                                    });
                                }
                            } else if ui
                                .small_button("Bind")
                                .on_hover_text(format!(
                                    "Bind to slot #{selected_slot}"
                                ))
                                .clicked()
                            {
                                block_bind_ev.write(BlockBindAction::Bind {
                                    slot: selected_slot,
                                    block_id: b.id.clone(),
                                });
                            }
                        },
                    );
                });
                // Tooltip on the whole row so users don't need to
                // hunt for a hover target. Carries the long-form
                // information (description + texture hint + tags)
                // that used to crowd the card layout.
                let row = row_resp.response;
                let tooltip = compose_block_tooltip(b);
                if !tooltip.is_empty() {
                    row.on_hover_text(tooltip);
                }
            }
        });
}

/// Build the multi-line tooltip shown on each Block Library row.
/// Pulled out as a helper so the row layout stays readable.
fn compose_block_tooltip(b: &maquette::block_meta::BlockMeta) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !b.description.is_empty() {
        parts.push(b.description.clone());
    }
    if !b.tags.is_empty() {
        parts.push(format!("tags: {}", b.tags.join(", ")));
    }
    if !b.texture_hint.is_empty() {
        parts.push(format!("hint: {}", b.texture_hint));
    }
    parts.join("\n\n")
}

/// Floating brush HUD — sits in the canvas's top-left corner
/// like Blender's paint-tool overlay. Anchored via `fixed_pos` to
/// the canvas `rect` so it tracks automatic repositioning when
/// the user resizes the left side panel. The overlay is fixed
/// (non-movable) by design: users consistently pick "it stays
/// where I expect" over "I can drag it but now it's covering my
/// work" in similar DCC HUDs.
///
/// Hit-test note: the overlay lives in egui's floating layer and
/// intercepts clicks over its own rect. Cells the brush panel
/// covers can't be painted until the user moves the panel or
/// shrinks the brush — acceptable for a tool HUD, and matches
/// Blender's behaviour with its Tool Settings floater.
fn brush_overlay(ctx: &egui::Context, canvas_rect: egui::Rect, ui_state: &mut UiState) {
    // Canvas may not have been laid out yet on the first frame
    // (width 0). Bail so we don't anchor at (0,0) of the screen.
    if canvas_rect.width() < 1.0 {
        return;
    }

    let anchor = canvas_rect.min + egui::vec2(8.0, 8.0);
    egui::Area::new(egui::Id::new("brush_overlay"))
        .fixed_pos(anchor)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(180))
                .corner_radius(6.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(70)))
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    // Hard-cap the overlay width so it doesn't
                    // balloon across a wide left panel — the
                    // brush HUD is meant to occupy a corner, not
                    // a strip.
                    ui.set_max_width(240.0);

                    ui.label(egui::RichText::new("Brush").strong());
                    ui.add_space(2.0);

                    ui.horizontal(|ui| {
                        ui.label("Height");
                        let mut h = ui_state.brush_height as u32;
                        let response = ui.add(
                            egui::Slider::new(
                                &mut h,
                                (MIN_HEIGHT as u32)..=(MAX_HEIGHT as u32),
                            )
                            .suffix(" cells"),
                        );
                        if response.changed() {
                            ui_state.brush_height = h as u8;
                        }
                    });

                    // Paint mode toggle. Rendered as a
                    // `SelectableLabel` pair rather than a radio
                    // group because the two options are mutually
                    // exclusive *and* we want the chosen one to
                    // read as "currently active" at a glance —
                    // radio bullets get lost at the small sizes
                    // a floating HUD uses.
                    ui.horizontal(|ui| {
                        ui.label("Mode");
                        if ui
                            .selectable_label(
                                ui_state.paint_mode == PaintMode::Overwrite,
                                PaintMode::Overwrite.label(),
                            )
                            .on_hover_text(
                                "Replace the cell with the current color + brush height. \
                                 Default mode; this is how v0.1–v0.8 worked.",
                            )
                            .clicked()
                        {
                            ui_state.paint_mode = PaintMode::Overwrite;
                        }
                        if ui
                            .selectable_label(
                                ui_state.paint_mode == PaintMode::Additive,
                                PaintMode::Additive.label(),
                            )
                            .on_hover_text(
                                "Stack the brush height on top of what's already there; \
                                 keep the existing color. Empty cells still use the brush \
                                 color. Each cell is only grown once per drag.",
                            )
                            .clicked()
                        {
                            ui_state.paint_mode = PaintMode::Additive;
                        }
                    });

                    ui.small(
                        egui::RichText::new(match ui_state.paint_mode {
                            PaintMode::Overwrite => {
                                "Overwrite: replace the cell with the selected color."
                            }
                            PaintMode::Additive => {
                                "Additive: stack onto existing cells. One stack per cell per stroke."
                            }
                        })
                        .color(egui::Color32::from_gray(170)),
                    );
                });
        });
}

/// A small floating button row anchored to the top-right corner of
/// the **central preview area** (NOT the entire window — that
/// would put it on top of the Block Library right SidePanel that
/// landed in v0.10 C-2). Hosts viewport actions (reset, fit,
/// multi-view toggle) so they're discoverable without hunting
/// through the menu bar.
#[allow(clippy::too_many_arguments)]
fn preview_toolbar(
    ctx: &egui::Context,
    central: egui::Rect,
    reset_view_ev: &mut MessageWriter<ResetPreviewView>,
    fit_view_ev: &mut MessageWriter<FitPreviewToModel>,
    zoom_view_ev: &mut MessageWriter<ZoomPreview>,
    multiview: &mut MultiViewState,
    float_state: &mut FloatPreviewState,
) {
    // We anchor the toolbar `LEFT_TOP` and *position* it via
    // `fixed_pos`. Anchoring `RIGHT_TOP` would line it up to the
    // window's right edge, not the central rect's; even with a
    // negative offset egui's anchor math doesn't know about the
    // SidePanel.
    let toolbar_pos = egui::pos2(central.max.x - 12.0, central.min.y + 8.0);
    egui::Area::new(egui::Id::new("preview_toolbar"))
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(toolbar_pos)
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(140))
                .corner_radius(6.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(70)))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Zoom controls come first so Fit / Reset still
                        // land in the historical right-hand slot users
                        // learned in v0.7. Smaller buttons because
                        // they're the high-frequency click, and the
                        // hover tooltip names the accelerator key.
                        if ui
                            .small_button("−")
                            .on_hover_text("Zoom out · scroll down / −")
                            .clicked()
                        {
                            zoom_view_ev.write(ZoomPreview { factor: ZOOM_STEP });
                        }
                        if ui
                            .small_button("+")
                            .on_hover_text("Zoom in · scroll up / =")
                            .clicked()
                        {
                            zoom_view_ev.write(ZoomPreview {
                                factor: 1.0 / ZOOM_STEP,
                            });
                        }
                        ui.separator();
                        if ui
                            .button("Fit")
                            .on_hover_text("Frame the painted geometry · F")
                            .clicked()
                        {
                            fit_view_ev.write(FitPreviewToModel);
                        }
                        if ui
                            .button("Reset")
                            .on_hover_text("Default angle and zoom · Cmd+R")
                            .clicked()
                        {
                            reset_view_ev.write(ResetPreviewView);
                        }
                        let mut enabled = multiview.enabled;
                        if ui
                            .toggle_value(&mut enabled, "Multi")
                            .on_hover_text("Top / Front / Side PIPs · F2")
                            .changed()
                        {
                            multiview.enabled = enabled;
                        }
                        let mut floating = float_state.floating;
                        let label = if floating { "Dock" } else { "Float" };
                        if ui
                            .toggle_value(&mut floating, label)
                            .on_hover_text(
                                "Pop the preview into a separate OS window you can move \
                                 to a second monitor. Close it to dock back.",
                            )
                            .changed()
                        {
                            float_state.floating = floating;
                        }
                    });
                });
        });
}

/// Draw the first-launch onboarding hint centered on the empty
/// canvas. Kept deliberately terse — four one-liners, no graphics —
/// so it reads as a quick reference, not a tutorial wall.
fn paint_empty_canvas_hint(painter: &egui::Painter, canvas_rect: egui::Rect) {
    let lines = [
        ("Left-click", "paint with the selected color"),
        ("Right-click", "cycle shape (Cube ↔ Sphere)"),
        ("Delete / Backspace", "erase the hovered cell"),
        ("1–9", "select a palette color"),
    ];

    let line_height = 22.0;
    let padding = egui::vec2(22.0, 16.0);
    let width = 360.0_f32.min(canvas_rect.width() - 24.0).max(220.0);
    let height = (lines.len() as f32) * line_height + padding.y * 2.0 + 26.0;

    let centre = canvas_rect.center();
    let panel_rect = egui::Rect::from_center_size(centre, egui::vec2(width, height));

    painter.rect_filled(panel_rect, 8.0, egui::Color32::from_black_alpha(180));
    painter.rect_stroke(
        panel_rect,
        8.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
        egui::epaint::StrokeKind::Middle,
    );

    let mut cursor = panel_rect.min + padding;
    painter.text(
        egui::pos2(panel_rect.center().x, cursor.y + 8.0),
        egui::Align2::CENTER_CENTER,
        "Start painting",
        egui::FontId::proportional(15.0),
        egui::Color32::from_gray(230),
    );
    cursor.y += 26.0;

    for (key, action) in lines {
        painter.text(
            egui::pos2(panel_rect.min.x + padding.x, cursor.y + line_height * 0.5),
            egui::Align2::LEFT_CENTER,
            key,
            egui::FontId::monospace(12.0),
            egui::Color32::from_rgb(180, 200, 240),
        );
        painter.text(
            egui::pos2(panel_rect.min.x + padding.x + 140.0, cursor.y + line_height * 0.5),
            egui::Align2::LEFT_CENTER,
            action,
            egui::FontId::proportional(12.0),
            egui::Color32::from_gray(200),
        );
        cursor.y += line_height;
    }
}

/// Paint a small label + frame on top of each ortho PIP viewport.
/// egui runs on top of Bevy's render, so this overlay shows through
/// any edge of the viewport that would otherwise be unlabeled.
/// Draw labels + borders on the PIPs *and* make each PIP clickable —
/// clicking snaps the main preview to that angle.
///
/// The PIPs themselves are rendered by raw `Camera3d`s outside of
/// egui, so we have to synthesise interaction: drop an invisible
/// clickable widget per PIP in a dedicated `egui::Area`. `Area`
/// anchors in screen coordinates and participates in egui's pointer
/// pipeline, so hovering / clicking works even though the PIP pixels
/// themselves are drawn by Bevy's renderer behind egui's layer.
fn paint_pip_labels(
    ctx: &egui::Context,
    window: &Window,
    state: &MultiViewState,
    central: egui::Rect,
    jump_ortho_ev: &mut MessageWriter<JumpToOrthoView>,
) {
    // PIPs sit at the bottom-right of the *central* (non-panel)
    // area. The egui-side layout match what `multiview::sync_viewports`
    // does on the renderer side; both read from the same available
    // rect so labels and 3-D content stay aligned.
    let rects = pip_logical_rects(window, state, central.max.x, central.max.y);
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("multiview_labels"));
    let painter = ctx.layer_painter(layer);
    let hover_stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(120, 180, 255));
    let label_bg = egui::Color32::from_black_alpha(170);
    let label_fg = egui::Color32::from_gray(230);

    for (i, r) in rects.iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(r.x, r.y),
            egui::vec2(r.size, r.size),
        );

        let mut clicked = false;
        let mut hovered = false;
        egui::Area::new(egui::Id::new(("pip_hit", i)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                let resp = ui.allocate_rect(rect, egui::Sense::click());
                clicked = resp.clicked();
                hovered = resp.hovered();
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            });

        // Each PIP gets its own tinted accent so the three adjacent
        // thumbnails read as separate views rather than a single
        // striped panel. The accent hue also matches the dominant
        // axis each view *doesn't* show (Top hides Y → green,
        // Front hides Z → blue, Side hides X → red), which gives
        // the user a second visual cue beyond the text label.
        let accent = pip_accent_color(r.kind);
        let frame_stroke = if hovered {
            hover_stroke
        } else {
            egui::Stroke::new(1.5, accent)
        };
        painter.rect_stroke(rect, 0.0, frame_stroke, egui::epaint::StrokeKind::Outside);

        // Thin accent bar along the bottom edge acts as a "tab"
        // indicator; cheaper than a tinted full-frame fill and
        // keeps the PIP interior itself the dark Bevy clear color.
        let bar_height = 3.0;
        let bar_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.bottom() - bar_height),
            egui::vec2(rect.width(), bar_height),
        );
        painter.rect_filled(bar_rect, 0.0, accent);

        let badge_size = egui::vec2(52.0, 18.0);
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(r.x + 6.0, r.y + 6.0),
            badge_size,
        );
        painter.rect_filled(badge_rect, 3.0, label_bg);
        painter.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            r.kind.label(),
            egui::FontId::proportional(12.0),
            label_fg,
        );

        draw_pip_axes(&painter, rect, r.kind);

        if clicked {
            jump_ortho_ev.write(JumpToOrthoView { kind: r.kind });
        }
    }
}

/// Per-PIP accent hue used for the border and the bottom strip.
/// Kept subtle (desaturated, mid-alpha) so the dark 3D render stays
/// the dominant surface; the accent just labels the panel.
fn pip_accent_color(kind: crate::multiview::OrthoKind) -> egui::Color32 {
    use crate::multiview::OrthoKind;
    match kind {
        // Top view shows XZ (looks down +Y) → accent the hidden axis.
        OrthoKind::Top => egui::Color32::from_rgba_unmultiplied(120, 210, 140, 200),
        // Front view shows XY → accent the hidden +Z.
        OrthoKind::Front => egui::Color32::from_rgba_unmultiplied(110, 170, 240, 200),
        // Side view shows ZY → accent the hidden +X.
        OrthoKind::Side => egui::Color32::from_rgba_unmultiplied(230, 130, 130, 200),
    }
}

/// Draw a small 2-arrow gizmo in the top-right corner of a PIP
/// showing the two world axes that *are* in the projection plane.
/// Each arrow is color-coded X=red, Y=green, Z=blue (matching
/// Blender / Unity / Godot) with a tiny text label, so the user can
/// answer "which way is +X in this view?" at a glance. The gizmo
/// sits under an alpha-backed plate so it stays readable over any
/// mesh without needing a depth-aware 3D overlay.
fn draw_pip_axes(
    painter: &egui::Painter,
    pip_rect: egui::Rect,
    kind: crate::multiview::OrthoKind,
) {
    use crate::multiview::OrthoKind;
    // Screen-space unit vectors for each world axis, per PIP.
    // `None` = axis points into/out of the screen for this view
    // and isn't drawn. Mapping is the same one `spawn_ortho_cameras`
    // encodes — keep these in sync if you re-orient a PIP.
    let (x_dir, y_dir, z_dir): (
        Option<egui::Vec2>,
        Option<egui::Vec2>,
        Option<egui::Vec2>,
    ) = match kind {
        // Top: up = -Z → world +X = screen right, +Z = screen down.
        OrthoKind::Top => (
            Some(egui::vec2(1.0, 0.0)),
            None,
            Some(egui::vec2(0.0, 1.0)),
        ),
        // Front: up = +Y → world +X = screen right, +Y = screen up.
        OrthoKind::Front => (
            Some(egui::vec2(1.0, 0.0)),
            Some(egui::vec2(0.0, -1.0)),
            None,
        ),
        // Side: camera at +X, up = +Y → world +Z = screen left
        // (camera right is -Z), +Y = screen up.
        OrthoKind::Side => (
            None,
            Some(egui::vec2(0.0, -1.0)),
            Some(egui::vec2(-1.0, 0.0)),
        ),
    };

    let axes: [(Option<egui::Vec2>, &str, egui::Color32); 3] = [
        (x_dir, "X", egui::Color32::from_rgb(230, 90, 90)),
        (y_dir, "Y", egui::Color32::from_rgb(120, 210, 120)),
        (z_dir, "Z", egui::Color32::from_rgb(120, 170, 245)),
    ];

    // Small, top-right corner. Size scales slightly with PIP size so
    // the gizmo stays readable when the PIPs flex with the window.
    let radius = (pip_rect.width() * 0.13).clamp(16.0, 24.0);
    let pad = 10.0;
    let centre = egui::pos2(pip_rect.right() - radius - pad, pip_rect.top() + radius + pad);

    // Backing plate — low-alpha round disc so the gizmo is legible
    // over any mesh color without yelling at the user.
    painter.circle_filled(
        centre,
        radius + 4.0,
        egui::Color32::from_black_alpha(90),
    );

    for (dir, label, color) in axes.iter() {
        let Some(d) = dir else { continue };
        let tip = centre + *d * radius;
        painter.line_segment(
            [centre, tip],
            egui::Stroke::new(2.0, *color),
        );
        // Arrowhead: two short segments rotated ±25° from the tip
        // back toward the centre. Cheap and renders crisply at any
        // scale because it's pure line geometry.
        let back = -*d;
        let (sin25, cos25) = (0.4226_f32, 0.9063_f32);
        let head_len = radius * 0.35;
        let left = egui::vec2(
            back.x * cos25 - back.y * sin25,
            back.x * sin25 + back.y * cos25,
        ) * head_len;
        let right = egui::vec2(
            back.x * cos25 + back.y * sin25,
            -back.x * sin25 + back.y * cos25,
        ) * head_len;
        painter.line_segment([tip, tip + left], egui::Stroke::new(2.0, *color));
        painter.line_segment([tip, tip + right], egui::Stroke::new(2.0, *color));
        // Label sits one step beyond the tip, slightly offset along
        // the arrow direction so "X" doesn't overlap the arrowhead.
        let label_pos = tip + *d * 10.0;
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(11.0),
            *color,
        );
    }
}

fn to_egui_color(c: Color) -> egui::Color32 {
    let s = c.to_srgba();
    egui::Color32::from_rgba_unmultiplied(
        (s.red.clamp(0.0, 1.0) * 255.0) as u8,
        (s.green.clamp(0.0, 1.0) * 255.0) as u8,
        (s.blue.clamp(0.0, 1.0) * 255.0) as u8,
        (s.alpha.clamp(0.0, 1.0) * 255.0) as u8,
    )
}
