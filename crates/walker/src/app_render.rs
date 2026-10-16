//! Egui rendering for the Walker application shell.
//!
//! Split from `app.rs` so its state and event coordinator stays within the
//! source-file size limit.

use eframe::egui;

use crate::chrome::{self, ExportContext, StatusInfo};
use crate::import::RfdPicker;
use crate::navigator;
use crate::transport;
use crate::view;

use super::app_view::renderer_label;
use super::{navigator_renderer, WalkerApp};

impl WalkerApp {
    fn show_history_panel(&mut self, ui: &mut egui::Ui) {
        let Some(file) = self.history_file.clone() else {
            return;
        };
        let Some(source) = self.source() else {
            return;
        };
        let mut export_markdown = None;
        egui::Panel::right("history").show(ui, |ui| {
            let action = crate::history_view::history_panel(ui, source, &file);
            export_markdown = action.export_markdown;
        });
        if let Some(markdown) = export_markdown {
            self.status = super::app_mutate::save_history_report(&file, &markdown);
        }
    }

    fn export_context(&self) -> Option<(Vec<walrus_fmt::export::ExportFormat>, String, String)> {
        let source = self.source()?;
        let active = self.navigator.active()?;
        let (mime, name) = source.content_hint(active);
        let offered = walrus_fmt::export::offered_formats(&mime);
        Some((offered, mime, name))
    }

    /// Render the shell's menu, panels, status, and cached document bytes.
    pub(super) fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Top menu bar + toolbar.
        let ctx_data = self.export_context();
        egui::Panel::top("menu").show(ui, |ui| {
            let export = ctx_data
                .as_ref()
                .map(|(offered, mime, name)| ExportContext {
                    offered,
                    doc: &self.current_bytes,
                    mime,
                    document_name: name,
                });
            let action = chrome::top_bar(ui, &mut self.theme_mode, export);
            if action.open {
                self.prompt_open();
            }
            if action.quit {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if let Some(status) = action.export_status {
                self.status = status;
            }
        });

        if self.read_source.is_some() {
            let mut activated = None;
            let mut import_outcome = None;
            let mut import_error = None;
            let mut history_for = None;
            let mut file_action = None;
            let picker = RfdPicker;
            egui::Panel::left("navigator").show(ui, |ui| {
                let source = self.read_source.as_deref().expect("read source is Some");
                let action =
                    navigator::panels(ui, source, &picker, &mut self.import, &mut self.navigator);
                activated = action.activated;
                import_outcome = action.import;
                import_error = action.import_error;
                history_for = action.history_for;
                file_action = action.file_action;
            });
            if let Some(file) = history_for {
                self.history_file = Some(file);
            }
            if let Some(message) = import_error {
                self.status = message;
            }
            if let Some(outcome) = import_outcome {
                self.apply_import(outcome);
            }
            if let Some(fa) = file_action {
                self.handle_file_action(fa);
            }
            if activated.is_some() {
                if let Some(source) = self.source() {
                    self.renderer = navigator_renderer(source, &self.navigator);
                }
                self.refresh_bytes();
            }
        }

        self.show_history_panel(ui);

        let variant = self.variant_str();
        let renderer_label = renderer_label(&self.renderer);
        let status = self.status.clone();
        let active = self.navigator.active().cloned();
        let hint = self
            .source()
            .zip(active.as_ref())
            .map(|(source, file)| source.content_hint(file));
        let note = self
            .source()
            .zip(active.as_ref())
            .and_then(|(source, file)| source.status_note(file));
        egui::Panel::bottom("status").show(ui, |ui| {
            let info = self
                .player
                .as_ref()
                .zip(hint.as_ref())
                .map(|(player, (mime, name))| StatusInfo {
                    file_name: name,
                    variant,
                    index: player.index(),
                    total: player.len(),
                    renderer: renderer_label,
                    mime,
                    note: note.as_deref(),
                });
            chrome::status_bar(ui, info.as_ref(), &status);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let mut changed = false;
            if let Some(player) = self.player.as_mut() {
                changed = transport::controls(ui, player, &mut self.speed);
            } else {
                ui.label("No file loaded.");
            }
            if changed {
                self.refresh_bytes();
            }

            ui.separator();
            if self.player.is_some() {
                let active = self.navigator.active().cloned();
                egui::ScrollArea::both().show(ui, |ui| match (self.source(), active.as_ref()) {
                    (Some(source), Some(active)) => {
                        if let Some(error) = self.current_error.as_ref() {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!("Reconstruction error: {error}"),
                            );
                        } else {
                            let renderer = view::present::select_renderer_for(source, active);
                            view::paint(ui, &renderer, &self.current_bytes);
                        }
                    }
                    _ => view::paint(ui, &self.renderer, &self.current_bytes),
                });
            }
        });
    }
}
