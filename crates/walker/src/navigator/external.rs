//! The EXTERNAL navigator pane: the filesystem import affordance (FR-K5).
//!
//! The pane wires the one "Import" action: a click invokes the injected
//! [`FilePicker`] (the real one opens an OS-native dialog; tests inject a fake),
//! reads the chosen file's bytes, and resolves the import against the archive's
//! current file list. A NEW name resolves to an add with no prompt; an EXISTING
//! name surfaces a prompt ("<name> exists -- import as a new version?") with a
//! confirm and an add-under-a-different-name alternative.
//!
//! The pane only drives the [`ImportState`] model and reports a resolved
//! [`ImportOutcome`] back to the app via [`NavigatorAction`]; the actual
//! byte-mutation + transactional write happen in the app shell against the
//! concrete `WalrusSource`. Everything here is headless-testable: only
//! `FilePicker::pick` (the OS dialog) is MANUAL.

use eframe::egui;

use super::NavigatorAction;
use crate::import::{update_prompt_message, FilePicker, ImportState};
use crate::source::HistorySource;

/// Draw the external import affordance and drive the [`ImportState`].
///
/// On an "Import..." click the `picker` yields a path; its bytes are read and
/// an import is begun, resolved against `source.files()`. While a prompt is
/// pending the pane renders the prompt plus confirm / add-as / cancel controls.
/// A resolved import is drained into [`NavigatorAction::import`] for the app to
/// apply. The pane never names a `walrus_fmt` type (FR-K1 AC2).
pub fn external_pane(
    ui: &mut egui::Ui,
    source: &dyn HistorySource,
    picker: &dyn FilePicker,
    import: &mut ImportState,
    action: &mut NavigatorAction,
) {
    ui.label("Import from filesystem");

    // The import button is live now (FR-K5): a click runs the picker seam. The
    // native dialog inside the real `RfdPicker` is the one MANUAL carve-out;
    // tests inject a `FakePicker` so the whole flow after the click is headless.
    if ui.button("Import...").clicked() {
        if let Some(path) = picker.pick() {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let names = source.files();
                    let existing: Vec<&str> = names.iter().map(|f| f.name()).collect();
                    import.begin(path, bytes, &existing);
                }
                Err(e) => {
                    action.import_error = Some(format!("reading {}: {e}", path.display()));
                }
            }
        }
    }

    // Surface the prompt for an existing-name collision, with the confirm and
    // the add-under-a-different-name alternative (FR-K5).
    if import.has_prompt() {
        render_prompt(ui, import);
    }

    // A resolved import (a fresh add, or an answered prompt) drains to the app
    // to apply + persist.
    if import.is_ready() {
        if let Some(outcome) = import.take_outcome() {
            action.import = Some(outcome);
        }
    }
}

/// Render the update prompt for a pending existing-name collision.
///
/// Shows the FR-K5 message and three choices: confirm the update (append a new
/// version), add the bytes under a different name, or cancel the import. The
/// add-as name field lives in egui temp memory keyed to a stable id so the pane
/// stays stateless between frames.
fn render_prompt(ui: &mut egui::Ui, import: &mut ImportState) {
    let name = import
        .pending()
        .map(|p| p.resolution.target_name().to_string())
        .unwrap_or_default();
    ui.label(update_prompt_message(&name));

    let name_id = ui.id().with("import-add-as-name");
    let mut new_name = ui
        .data_mut(|d| d.get_temp::<String>(name_id))
        .unwrap_or_default();

    ui.horizontal(|ui| {
        if ui.button("Import as new version").clicked() {
            import.confirm_update();
        }
        if ui.button("Cancel").clicked() {
            import.clear();
        }
    });

    ui.horizontal(|ui| {
        ui.label("or add as:");
        let edit = ui.add(egui::TextEdit::singleline(&mut new_name).hint_text("different name"));
        if edit.changed() {
            ui.data_mut(|d| d.insert_temp(name_id, new_name.clone()));
        }
        let can_add_as = !new_name.trim().is_empty();
        if ui
            .add_enabled(can_add_as, egui::Button::new("Add"))
            .clicked()
        {
            import.add_as(new_name.trim());
            ui.data_mut(|d| d.remove::<String>(name_id));
        }
    });
}
