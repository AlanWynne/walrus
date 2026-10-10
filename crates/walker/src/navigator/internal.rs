//! The INTERNAL navigator pane: the archive's tracked files (FR-K4).
//!
//! This is the GUI form of `walrus query -l`: one selectable row per tracked
//! file showing its name, index size (version count), generation cap, and
//! current size. The listing is driven through the format-agnostic seam
//! (`source.files()` + `source.file_info(&f)`), so it learns nothing about the
//! concrete archive format.

use eframe::egui;

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

    ui.label("Tracked files");
    for file in source.files() {
        let info = source.file_info(&file);
        let selected = state.is_selected(file.id());

        // TODO(FR-K7): colour this row band from FileInfo.log_bytes / size_bytes
        // (R = log_bytes / size_bytes), green/amber/red with an index-count
        // floor. FR-K4 only lays out the row; the bloat colour band is FR-K7.
        let label = row_label(file.name(), &info);
        // The row carries a stable egui id derived from the file id so a
        // headless harness (and ctrl-click bookkeeping) can target it; the
        // label text is also unique per file for accessibility-tree queries.
        let response = ui
            .push_id(("nav-file", file.id()), |ui| {
                ui.selectable_label(selected, label)
            })
            .inner;

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
    batch_action_bar(ui, state);

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

/// Draw the batch-action affordance. The button is ENABLED only when two or
/// more files are selected (AC3); the actual export/remove/condense operations
/// are FR-K6.
fn batch_action_bar(ui: &mut egui::Ui, state: &NavigatorState) {
    let count = state.selected_count();
    let enabled = state.batch_enabled();
    // TODO(FR-K6): wire export/remove/condense batch ops to this button; it is a
    // disabled-or-enabled affordance only in FR-K4.
    ui.add_enabled(
        enabled,
        egui::Button::new(format!("Batch actions ({count})")),
    );
}
