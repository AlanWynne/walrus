//! The EXTERNAL navigator pane: the filesystem-browser SCAFFOLD (FR-K4).
//!
//! FR-K4 ships the LAYOUT only. The actual import action and the native `rfd`
//! file dialog are FR-K5; this pane lays out the frame and a disabled
//! "Import..." button so the two-pane shape is in place without blocking FR-K4
//! on the OS dialog.

use eframe::egui;

use super::NavigatorState;

/// Draw the external import-browser scaffold.
///
/// Layout only: a heading and a disabled Import button. No `rfd` call and no
/// filesystem traversal happen in FR-K4.
pub fn external_pane(ui: &mut egui::Ui, _state: &NavigatorState) {
    ui.label("Import from filesystem");
    // TODO(FR-K5): rfd dialog + add/update import action here. The OS-native
    // file dialog is left MANUAL (not headless-drivable); FR-K4 is the scaffold
    // only, so the button is disabled until FR-K5 wires the import flow.
    ui.add_enabled(false, egui::Button::new("Import..."));
}
