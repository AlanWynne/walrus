//! The INTERNAL navigator pane: the archive's tracked files (FR-K4).
//!
//! This is the GUI form of `walrus query -l`: one selectable row per tracked
//! file showing its name, index size (version count), generation cap, and
//! current size. The listing is driven through the format-agnostic seam
//! (`source.files()` + `source.file_info(&f)`), so it learns nothing about the
//! concrete archive format.

use eframe::egui;

use super::bloat;
use super::{NavigatorAction, NavigatorState};
use crate::source::{FileInfo, HistorySource};

/// Draw the internal file list and return the frame's [`NavigatorAction`].
///
/// Each row is a selectable widget carrying a stable egui id
/// (`ui.id().with(("nav-file", file.id()))`) so a headless harness can target
/// it. A plain click selects the file (binds its timeline, AC2); the row also
/// honours ctrl/command-click to toggle multi-select membership (AC3). A
/// "Batch actions (N)" button reflects the multi-select enabled state.
pub fn internal_pane(
    ui: &mut egui::Ui,
    source: &dyn HistorySource,
    state: &mut NavigatorState,
) -> NavigatorAction {
    let mut action = NavigatorAction::default();

    // The overridable FR-K7 thresholds; copied out so the per-row borrow of
    // `state` for selection bookkeeping does not conflict.
    let thresholds = *state.bloat_thresholds();

    ui.label("Tracked files");
    for file in source.files() {
        let info = source.file_info(&file);
        let selected = state.is_selected(file.id());

        // FR-K7: advisory bloat band from R = log_bytes / size_bytes, green/
        // amber/red with the <50-version floor. Advisory only -- a
        // non-interactive coloured tag plus a tooltip; it never mutates.
        let band = bloat::classify(
            info.log_bytes,
            info.size_bytes,
            info.index_size,
            &thresholds,
        );
        let label = row_label(file.name(), &info);
        // The row carries a stable egui id derived from the file id so a
        // headless harness (and ctrl-click bookkeeping) can target it; the
        // label text is also unique per file for accessibility-tree queries.
        let (response, history_clicked) = ui
            .push_id(("nav-file", file.id()), |ui| {
                ui.horizontal(|ui| {
                    // Non-interactive advisory tag: coloured band name before
                    // the selectable label. Exact shade is a MANUAL check; the
                    // text carries the band for the headless harness.
                    ui.colored_label(bloat::band_color(band), bloat::band_label(band));
                    let selected = ui.selectable_label(selected, label);
                    // FR-K9: a per-row "History" action opens the change-log
                    // viewer for this file without rebinding the active timeline.
                    let history = ui.button("History").clicked();
                    (selected, history)
                })
                .inner
            })
            .inner;
        let response = response.on_hover_text(bloat::tooltip_text(
            info.index_size,
            info.log_bytes,
            info.size_bytes,
            band,
        ));

        if history_clicked {
            action.history_for = Some(file.clone());
        }

        if response.clicked() {
            if ui.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                // Ctrl/command-click toggles multi-select without rebinding.
                state.toggle(file.id());
            } else {
                state.select(&file);
                action.activated = Some(file.clone());
            }
        }
    }

    ui.separator();
    // FR-K6: the batch-action bar now wires export/condense/remove with the
    // explicit batch-remove confirmation (controls_ui::batch_bar), replacing the
    // FR-K4 enabled-affordance-only stub.
    super::controls_ui::batch_bar(ui, state, &mut action);

    action
}

/// The one-line summary shown on a file's row: name, index size, generation
/// cap (`"unlimited"` when `None`), and current size in bytes.
fn row_label(name: &str, info: &FileInfo) -> String {
    let generations = match info.generations {
        Some(cap) => cap.to_string(),
        None => "unlimited".to_string(),
    };
    format!(
        "{name}  --  {} versions, gen {generations}, {} bytes",
        info.index_size, info.size_bytes
    )
}
