//! FFWB-style chrome: a top menu bar + toolbar and a bottom status bar
//! (design section 5). There is NO command line and NO `ff-*` dependency --
//! FFWB is layout inspiration only.
//!
//! The chrome is drawn by free functions that return the user's intent as a
//! `ChromeAction`, so `app.rs` stays thin and holds the model.

use eframe::egui;
use walrus_fmt::export::ExportFormat;

use crate::export_ui;
use crate::theme::{self, ThemeMode};

/// What the user asked the chrome to do this frame.
#[derive(Debug, Default)]
pub struct ChromeAction {
    /// The user asked to open a file.
    pub open: bool,
    /// The user asked to quit.
    pub quit: bool,
    /// A status message produced by an export, to show in the status bar.
    pub export_status: Option<String>,
}

/// Summary of the loaded document for the status bar.
pub struct StatusInfo<'a> {
    /// The file name.
    pub file_name: &'a str,
    /// The variant string (`wal` / `wbl`).
    pub variant: &'a str,
    /// Current version index.
    pub index: usize,
    /// Total versions.
    pub total: usize,
    /// The detected renderer label.
    pub renderer: &'a str,
    /// The document MIME type.
    pub mime: &'a str,
    /// An optional presentation note (e.g. a structured-view caveat), shown
    /// after the mime when `Some`. `None` leaves the bar visually unchanged.
    pub note: Option<&'a str>,
}

/// Context the menu bar needs to render the per-document export menu.
pub struct ExportContext<'a> {
    /// Offered formats for the current document.
    pub offered: &'a [ExportFormat],
    /// The reconstructed current-version bytes.
    pub doc: &'a [u8],
    /// The document MIME type.
    pub mime: &'a str,
    /// The stored document name (Original export uses this).
    pub document_name: &'a str,
}

/// Draw the top menu bar + toolbar. Returns the user's actions.
///
/// `export` is `None` when no file is loaded (the Export menu is then disabled).
pub fn top_bar(
    ui: &mut egui::Ui,
    theme_mode: &mut ThemeMode,
    export: Option<ExportContext<'_>>,
) -> ChromeAction {
    let mut action = ChromeAction::default();

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Open...").clicked() {
                action.open = true;
                ui.close();
            }
            ui.menu_button("Export...", |ui| match export.as_ref() {
                Some(ctx) => {
                    if let Some(status) =
                        export_ui::menu(ui, ctx.offered, ctx.doc, ctx.mime, ctx.document_name)
                    {
                        action.export_status = Some(status);
                    }
                }
                None => {
                    ui.label("Open a file first");
                }
            });
            ui.separator();
            if ui.button("Quit").clicked() {
                action.quit = true;
                ui.close();
            }
        });

        ui.menu_button("View", |ui| {
            ui.label("Renderer is chosen automatically from the document type.");
        });

        ui.menu_button("Theme", |ui| {
            theme::picker(ui, theme_mode);
        });
    });

    // Thin toolbar row with quick-access controls, mirroring FFWB density.
    ui.horizontal(|ui| {
        if ui.button("Open").clicked() {
            action.open = true;
        }
        ui.separator();
        theme::picker(ui, theme_mode);
    });

    action
}

/// Draw the bottom status bar (non-interactive labels), plus any current error.
///
/// When `info.note` is `Some`, the note is rendered after the mime (a separator
/// then the label); when `None` the bar is visually identical to the Phase-1
/// layout.
pub fn status_bar(ui: &mut egui::Ui, info: Option<&StatusInfo<'_>>, message: &str) {
    ui.horizontal(|ui| match info {
        Some(info) => {
            ui.label(info.file_name);
            ui.separator();
            ui.label(info.variant);
            ui.separator();
            ui.label(format!("{}/{}", info.index, info.total));
            ui.separator();
            ui.label(info.renderer);
            ui.separator();
            ui.label(info.mime);
            if let Some(note) = info.note {
                ui.separator();
                ui.label(note);
            }
            ui.separator();
            ui.label(message);
        }
        None => {
            ui.label(message);
        }
    });
}

// === Tests ==========================================================
#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    /// Build a `StatusInfo` with the given optional note, over fixed fields.
    fn info_with_note(note: Option<&'static str>) -> StatusInfo<'static> {
        StatusInfo {
            file_name: "doc.json",
            variant: "amrg",
            index: 0,
            total: 3,
            renderer: "Code",
            mime: "application/json",
            note,
        }
    }

    #[test]
    fn status_bar_renders_the_note_label_when_some() {
        let mut harness = Harness::new_ui(|ui| {
            let info = info_with_note(Some("Automerge (structured JSON view)"));
            status_bar(ui, Some(&info), "ready");
        });
        harness.run();
        assert!(
            harness
                .query_by_label("Automerge (structured JSON view)")
                .is_some(),
            "a Some(note) must reach the status bar as a label"
        );
    }

    #[test]
    fn status_bar_omits_the_note_label_when_none() {
        let mut harness = Harness::new_ui(|ui| {
            let info = info_with_note(None);
            status_bar(ui, Some(&info), "ready");
        });
        harness.run();
        assert!(
            harness
                .query_by_label("Automerge (structured JSON view)")
                .is_none(),
            "a None note must not render any extra label"
        );
    }
}
