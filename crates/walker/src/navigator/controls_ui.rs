//! The egui rendering of the FR-K6 per-file control panel and the batch-action
//! bar.
//!
//! Split out of `internal.rs` so that file stays under the 400 non-test-line
//! cap and so all the FR-K6 control widgets live together. These render the
//! pure [`controls`](super::controls) model: the per-file panel edits a
//! generation-cap and a condense `keep N` field (held in egui temp memory keyed
//! by file id, the stateless-pane pattern from `external.rs`) and emits a
//! [`FileAction`] onto the frame's [`NavigatorAction`]; the batch bar routes a
//! batch remove of two or more files through the [`PendingBatchRemove`]
//! confirmation gate (AC2) and emits batch export / condense directly.
//!
//! Nothing here names a `walrus_fmt` type (FR-K1 AC2): the widgets pass only
//! plain `usize` / `u32` / `usize` values on the action. The `rfd` save/open
//! dialogs are NOT touched here -- export emits an intent the app runs through
//! the existing `export_ui` path, where the dialog is the one MANUAL carve-out.

use eframe::egui;

use super::controls::{gen_cap_from_input, FileAction};
use super::{NavigatorAction, NavigatorState};
use crate::source::HistorySource;

/// Render the per-file control panel for the active file, if any, emitting a
/// [`FileAction`] onto `action` when a control is activated.
///
/// Shows a generation-cap field (Apply -> [`FileAction::SetGenerations`]), a
/// condense `keep N` field + "Condense now" button (-> [`FileAction::Condense`]),
/// a "Remove" button (-> [`FileAction::Remove`], single file, no confirmation),
/// and an "Export" button (-> [`FileAction::ExportFile`]). A no-op when no file
/// is active. The field values live in egui temp memory keyed by the active
/// file id, so repeated frames keep what the user typed without any model state.
pub fn file_controls_panel(
    ui: &mut egui::Ui,
    source: &dyn HistorySource,
    state: &NavigatorState,
    action: &mut NavigatorAction,
) {
    let Some(active) = state.active().cloned() else {
        return;
    };
    let id = active.id();
    let info = source.file_info(&active);

    ui.label(format!("Controls: {}", active.name()));

    // Generation cap field + Apply. The field is prefilled from the current cap
    // (empty = unlimited) on first render and then held in temp memory.
    let gen_id = ui.id().with(("gen-cap", id));
    let mut gen_text = ui
        .data_mut(|d| d.get_temp::<String>(gen_id))
        .unwrap_or_else(|| match info.generations {
            Some(cap) => cap.to_string(),
            None => String::new(),
        });
    ui.horizontal(|ui| {
        // The field is labelled_by its caption so the accessibility tree exposes
        // a unique, queryable name for the headless harness (a hint text is NOT
        // surfaced in the tree, and the bare value would collide with the row
        // label "gen unlimited").
        let caption = ui.label("Generation cap:");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut gen_text)
                .hint_text("unlimited")
                .desired_width(80.0),
        );
        let edit = edit.labelled_by(caption.id);
        if edit.changed() {
            ui.data_mut(|d| d.insert_temp(gen_id, gen_text.clone()));
        }
        if ui.button("Apply cap").clicked() {
            action.file_action = Some(FileAction::SetGenerations {
                id,
                n: gen_cap_from_input(&gen_text),
            });
        }
    });

    // Condense keep N field + Condense now.
    let keep_id = ui.id().with(("condense-keep", id));
    let mut keep_text = ui
        .data_mut(|d| d.get_temp::<String>(keep_id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        // labelled_by its caption for a unique, queryable accessible name (see
        // the generation-cap field above).
        let caption = ui.label("Condense keep:");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut keep_text)
                .hint_text("N")
                .desired_width(60.0),
        );
        let edit = edit.labelled_by(caption.id);
        if edit.changed() {
            ui.data_mut(|d| d.insert_temp(keep_id, keep_text.clone()));
        }
        if ui.button("Condense now").clicked() {
            let keep = keep_text.trim().parse::<usize>().unwrap_or(0);
            action.file_action = Some(FileAction::Condense { id, keep });
        }
    });

    ui.horizontal(|ui| {
        // Single-file remove: no confirmation gate (AC2 applies to batches).
        if ui.button("Remove").clicked() {
            action.file_action = Some(FileAction::Remove { id });
        }
        // Export reuses the existing export path; the app runs the `rfd` save
        // dialog (the one MANUAL carve-out) off this intent.
        if ui.button("Export").clicked() {
            action.file_action = Some(FileAction::ExportFile { id });
        }
    });
}

/// Render the batch-action bar, replacing the FR-K4 enabled-affordance stub.
///
/// When two or more files are selected ([`NavigatorState::batch_enabled`]), it
/// offers batch Export / Condense (`keep N`) / Remove. Batch Remove does NOT
/// emit [`FileAction::BatchRemove`] directly: it parks the selected ids in the
/// [`PendingBatchRemove`] gate and renders a confirmation row; Confirm emits the
/// action with the parked ids and clears the gate, Cancel clears it (AC2).
/// Batch Export / Condense emit their actions directly.
pub fn batch_bar(ui: &mut egui::Ui, state: &mut NavigatorState, action: &mut NavigatorAction) {
    let count = state.selected_count();
    let enabled = state.batch_enabled();

    if !state.pending_batch_remove().is_pending() {
        ui.horizontal(|ui| {
            ui.label(format!("Batch actions ({count})"));
            if ui
                .add_enabled(enabled, egui::Button::new("Batch export"))
                .clicked()
            {
                action.file_action = Some(FileAction::BatchExport {
                    ids: state.selected_ids(),
                });
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Batch condense"))
                .clicked()
            {
                // keep = 0 collapses each selected file to current-only; the
                // batch condense keeps the simplest, explicit default.
                action.file_action = Some(FileAction::BatchCondense {
                    ids: state.selected_ids(),
                    keep: 0,
                });
            }
            if ui
                .add_enabled(enabled, egui::Button::new("Batch remove"))
                .clicked()
            {
                // AC2: park the ids and surface a confirmation; do NOT remove
                // yet. The confirmation row below renders next frame.
                let ids = state.selected_ids();
                state.pending_batch_remove_mut().request(ids);
            }
        });
    } else {
        let pending = state.pending_batch_remove().ids().len();
        ui.horizontal(|ui| {
            ui.label(format!("Remove {pending} files?"));
            if ui.button("Confirm remove").clicked() {
                let ids = state.pending_batch_remove_mut().confirm();
                action.file_action = Some(FileAction::BatchRemove { ids });
            }
            if ui.button("Cancel remove").clicked() {
                state.pending_batch_remove_mut().cancel();
            }
        });
    }
}
