//! egui panels: menu bar, paint canvas + palette (left), status bar.
//!
//! The canvas uses egui's custom painter to draw a 2D grid and consume
//! pointer events. Left-click paints with the selected palette color,
//! right-click erases. Painting mutates the [`Grid`] resource directly;
//! the mesh rebuild system (in [`crate::grid`]) then picks up the change.

use bevy::color::Hsla;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use maquette::export::{ExportFormat, ExportOptions, ExportRequest, OutlineConfig};
use maquette::grid::{
    DeleteColorMode, Grid, Palette, DEFAULT_GRID_H, DEFAULT_GRID_W, MAX_GRID, MAX_HEIGHT, MIN_GRID,
    MIN_HEIGHT,
};

use bevy::window::PrimaryWindow;

use crate::camera::{FitPreviewToModel, ResetPreviewView};
use crate::float_window::FloatPreviewState;
use crate::history::{EditHistory, HistoryAction, PaintOp};
use crate::multiview::{pip_logical_rects, MultiViewState};
use crate::session::{CurrentProject, ProjectAction};

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
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            show_about: false,
            new_project: None,
            export_modal: None,
            delete_color_modal: None,
            brush_height: MIN_HEIGHT,
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
    windows: Query<&Window, With<PrimaryWindow>>,
    mut project_ev: MessageWriter<ProjectAction>,
    mut reset_view_ev: MessageWriter<ResetPreviewView>,
    mut fit_view_ev: MessageWriter<FitPreviewToModel>,
    mut history_ev: MessageWriter<HistoryAction>,
    mut export_ev: MessageWriter<ExportRequest>,
) -> Result {
    let ctx = ctx.ctx_mut()?;

    handle_shortcuts(
        ctx,
        &mut ui_state,
        &mut palette,
        &mut multiview,
        &mut project_ev,
        &mut reset_view_ev,
        &mut fit_view_ev,
        &mut history_ev,
    );

    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New…").clicked() {
                    ui_state.new_project = Some(NewProjectDraft::default());
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    project_ev.write(ProjectAction::Open);
                    ui.close();
                }
                ui.separator();
                if ui.button("Save").clicked() {
                    project_ev.write(ProjectAction::Save);
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    project_ev.write(ProjectAction::SaveAs);
                    ui.close();
                }
                ui.separator();
                if ui.button("Export…").clicked() {
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
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(filter_name, &[ext])
                .set_file_name(default_name)
                .save_file()
            {
                let [r, g, b] = draft.outline_color;
                export_ev.write(ExportRequest(ExportOptions {
                    path,
                    format: draft.format,
                    outline: OutlineConfig {
                        enabled: draft.outline_enabled,
                        width_pct: draft.outline_width_pct,
                        color: Color::srgb(r, g, b),
                    },
                }));
            }
            ui_state.export_modal = None;
        } else if cancelled || !open {
            ui_state.export_modal = None;
        } else {
            ui_state.export_modal = Some(draft);
        }
    }

    delete_color_modal(ctx, &mut ui_state, &mut grid, &mut palette, &mut current);

    if ui_state.show_about {
        let mut open = ui_state.show_about;
        egui::Window::new("About Maquette")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Maquette — a block-style asset editor with a toon look.");
                ui.add_space(6.0);
                ui.label(
                    "Paint on the 2D canvas on the left; the 3D preview on the right updates in real time.",
                );
                ui.add_space(6.0);
                ui.label("• Left-click a cell to paint with the selected color.");
                ui.label("• Right-click a cell to erase.");
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

    egui::SidePanel::left("canvas_panel")
        .default_width(520.0)
        .min_width(360.0)
        .show(ctx, |ui| {
            ui.heading("Canvas");
            ui.separator();
            paint_canvas(
                ui,
                &mut grid,
                &palette,
                &mut current,
                &mut history,
                ui_state.brush_height,
            );
            ui.add_space(10.0);
            ui.separator();
            palette_bar(ui, &mut palette, &mut ui_state);
            ui.add_space(10.0);
            ui.separator();
            brush_bar(ui, &mut ui_state.brush_height);
        });

    if multiview.enabled {
        if let Ok(window) = windows.single() {
            paint_pip_labels(ctx, window, &multiview);
        }
    }

    preview_toolbar(
        ctx,
        &mut reset_view_ev,
        &mut fit_view_ev,
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
            ui.label(
                "Left-click: paint  •  Right-click: erase  •  Drag preview: turn  •  Scroll preview: zoom",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label("Maquette · v0.8-dev");
            });
        });
    });

    Ok(())
}

fn paint_canvas(
    ui: &mut egui::Ui,
    grid: &mut Grid,
    palette: &Palette,
    current: &mut CurrentProject,
    history: &mut EditHistory,
    brush_height: u8,
) {
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
    }

    // Input: paint on primary drag/click, erase on secondary.
    let paint = response.dragged_by(egui::PointerButton::Primary)
        || response.clicked_by(egui::PointerButton::Primary);
    let erase = response.dragged_by(egui::PointerButton::Secondary)
        || response.clicked_by(egui::PointerButton::Secondary);

    if paint || erase {
        if let Some(pos) = response.interact_pointer_pos() {
            let local = pos - rect.min;
            if local.x >= 0.0 && local.y >= 0.0 {
                let gx = (local.x / cell_px) as usize;
                let gy = (local.y / cell_px) as usize;
                let change = if paint {
                    grid.paint(gx, gy, palette.selected, brush_height)
                } else {
                    grid.erase(gx, gy)
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

    // Close the stroke when the pointer is released. Plain clicks
    // never open a stroke (`drag_started` doesn't fire for them), so
    // single-cell edits fall through the `record` single-op fallback.
    if response.drag_stopped_by(egui::PointerButton::Primary)
        || response.drag_stopped_by(egui::PointerButton::Secondary)
    {
        history.end_stroke();
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
        // F2 toggles the multi-view PIP overlay. Plain F-key (no
        // cmd) because it's a viewport preference, not a document
        // action, and should feel as light as tab-switching.
        if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F2)) {
            multiview.enabled = !multiview.enabled;
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

fn palette_bar(ui: &mut egui::Ui, palette: &mut Palette, ui_state: &mut UiState) {
    ui.label(egui::RichText::new("Palette").strong());
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let swatch_size = egui::vec2(32.0, 32.0);

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
                if ui.button("Delete…").clicked() {
                    ui_state.delete_color_modal = Some(DeleteColorDraft {
                        idx: slot_idx,
                        remap_to: None,
                    });
                    ui.close();
                }
            });
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
            egui::FontId::proportional(18.0),
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

fn brush_bar(ui: &mut egui::Ui, brush_height: &mut u8) {
    ui.label(egui::RichText::new("Brush").strong());
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label("Height");
        let mut h = *brush_height as u32;
        let response = ui.add(
            egui::Slider::new(&mut h, (MIN_HEIGHT as u32)..=(MAX_HEIGHT as u32))
                .suffix(" cells"),
        );
        if response.changed() {
            *brush_height = h as u8;
        }
    });
    ui.small(
        egui::RichText::new(
            "Controls how many cells tall each paint stroke extrudes in the preview.",
        )
        .color(egui::Color32::from_gray(150)),
    );
}

/// A small floating button row anchored to the top-right corner of
/// the window. Hosts viewport actions (reset, fit, multi-view
/// toggle) so they're discoverable without hunting through the
/// menu bar — the v0.7 status bar hint "Drag preview: turn · Scroll
/// preview: zoom" was doing discovery work the UI itself should do.
#[allow(clippy::too_many_arguments)]
fn preview_toolbar(
    ctx: &egui::Context,
    reset_view_ev: &mut MessageWriter<ResetPreviewView>,
    fit_view_ev: &mut MessageWriter<FitPreviewToModel>,
    multiview: &mut MultiViewState,
    float_state: &mut FloatPreviewState,
) {
    egui::Area::new(egui::Id::new("preview_toolbar"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 40.0))
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(140))
                .corner_radius(6.0)
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(70)))
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
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
        ("Right-click", "erase"),
        ("1–9", "select a palette color"),
        ("Right-click a swatch", "edit or delete"),
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
fn paint_pip_labels(ctx: &egui::Context, window: &Window, state: &MultiViewState) {
    let rects = pip_logical_rects(window, state);
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("multiview_labels"));
    let painter = ctx.layer_painter(layer);
    let frame_stroke = egui::Stroke::new(1.0, egui::Color32::from_gray(80));
    let label_bg = egui::Color32::from_black_alpha(170);
    let label_fg = egui::Color32::from_gray(220);

    for r in rects {
        let rect = egui::Rect::from_min_size(
            egui::pos2(r.x, r.y),
            egui::vec2(r.size, r.size),
        );
        painter.rect_stroke(rect, 0.0, frame_stroke, egui::epaint::StrokeKind::Outside);

        let badge_size = egui::vec2(48.0, 18.0);
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
