//! The egui application shell for Walker.
//!
//! Responsibilities:
//!   - open a `.wal` / `.wbl` file via a native dialog,
//!   - derive the renderer per-document from `select_renderer(mime, name)` and
//!     show the current version through the content-aware viewer tree,
//!   - transport controls (step/play/direction/loop/speed) via `transport`,
//!   - a seek slider across the version range,
//!   - export the selected version via `export_ui` (menu in `chrome`).
//!
//! The heavy lifting (reconstruct/playback) lives in `walrus_fmt` and `player`;
//! chrome, transport, theming, and export live in their own modules so this
//! shell stays thin.

use std::time::Instant;

use eframe::egui;
use walrus_fmt::{Variant, WalrusFile};

use crate::chrome::{self, ExportContext, StatusInfo};
use crate::player::Player;
use crate::theme::{self, ThemeMode};
use crate::transport;
use crate::view::{self, Renderer};

/// The Walker application state.
pub struct WalkerApp {
    player: Option<Player>,
    /// Cached current-version bytes, refreshed on change.
    current_bytes: Vec<u8>,
    /// The renderer derived from the loaded document's MIME and name.
    renderer: Renderer,
    status: String,
    last_frame: Instant,
    /// Current speed multiplier, held across frames and files.
    speed: f64,
    /// The selected theme mode.
    theme_mode: ThemeMode,
    /// Whether the theme has been applied to the context yet.
    theme_applied: bool,
}

impl Default for WalkerApp {
    fn default() -> Self {
        Self {
            player: None,
            current_bytes: Vec::new(),
            renderer: Renderer::Text,
            status: "Open a .wal or .wbl file to begin.".to_string(),
            last_frame: Instant::now(),
            speed: 1.0,
            theme_mode: ThemeMode::default(),
            theme_applied: false,
        }
    }
}

impl WalkerApp {
    /// Construct the app for `eframe`.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// Open a Walrus file, position at the current version, and derive the
    /// renderer.
    fn open_file(&mut self, path: std::path::PathBuf) {
        match WalrusFile::open(&path) {
            Ok(file) => {
                let doc = file.manifest().document.clone();
                self.renderer = view::select_renderer(&doc.mime, &doc.name);
                let mut player = Player::new(file);
                player.seek(0);
                transport::set_speed_multiplier(&mut player, self.speed);
                self.refresh_bytes(&player);
                self.status = format!("Opened {} ({} versions)", path.display(), player.len());
                self.player = Some(player);
            }
            Err(e) => {
                self.status = format!("Failed to open {}: {e}", path.display());
            }
        }
    }

    /// Refresh the cached current-version bytes from the player.
    fn refresh_bytes(&mut self, player: &Player) {
        match player.current_bytes() {
            Ok(bytes) => self.current_bytes = bytes,
            Err(e) => {
                self.current_bytes = format!("<reconstruction error: {e}>").into_bytes();
            }
        }
    }

    /// The variant string of the loaded file, if any.
    fn variant_str(&self) -> &'static str {
        match self.player.as_ref().map(|p| p.variant()) {
            Some(Variant::Wal) => "wal",
            Some(Variant::Wbl) => "wbl",
            None => "-",
        }
    }

    /// Open the native file dialog and load the chosen file.
    fn prompt_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Walrus", &["wal", "wbl"])
            .pick_file()
        {
            self.open_file(path);
        }
    }

    /// Build the export context for the chrome menu, if a file is loaded.
    fn export_context(&self) -> Option<(Vec<walrus_fmt::export::ExportFormat>, String, String)> {
        let player = self.player.as_ref()?;
        let doc = &player.manifest().document;
        let offered = walrus_fmt::export::offered_formats(&doc.mime);
        Some((offered, doc.mime.clone(), doc.name.clone()))
    }
}

impl eframe::App for WalkerApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            theme::apply(ctx, self.theme_mode);
            self.theme_applied = true;
        }

        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame);
        self.last_frame = now;

        let mut version_changed = false;
        if let Some(player) = self.player.as_mut() {
            version_changed = player.tick(elapsed);
            if player.is_playing() {
                ctx.request_repaint();
            }
        }
        if version_changed {
            if let Some(player) = self.player.as_ref() {
                match player.current_bytes() {
                    Ok(bytes) => self.current_bytes = bytes,
                    Err(e) => {
                        self.current_bytes = format!("<reconstruction error: {e}>").into_bytes();
                    }
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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

        // Bottom status bar.
        let variant = self.variant_str();
        let renderer_label = renderer_label(&self.renderer);
        let status = self.status.clone();
        egui::Panel::bottom("status").show(ui, |ui| {
            let info = self.player.as_ref().map(|player| {
                let doc = &player.manifest().document;
                StatusInfo {
                    file_name: &doc.name,
                    variant,
                    index: player.index(),
                    total: player.len(),
                    renderer: renderer_label,
                    mime: &doc.mime,
                }
            });
            chrome::status_bar(ui, info.as_ref(), &status);
        });

        // Central content: transport strip + the content-aware viewer.
        egui::CentralPanel::default().show(ui, |ui| {
            let mut changed = false;
            if let Some(player) = self.player.as_mut() {
                changed = transport::controls(ui, player, &mut self.speed);
            } else {
                ui.label("No file loaded.");
            }
            if changed {
                if let Some(player) = self.player.as_ref() {
                    match player.current_bytes() {
                        Ok(bytes) => self.current_bytes = bytes,
                        Err(e) => {
                            self.current_bytes =
                                format!("<reconstruction error: {e}>").into_bytes();
                        }
                    }
                }
            }

            ui.separator();
            if self.player.is_some() {
                egui::ScrollArea::both().show(ui, |ui| {
                    view::paint(ui, &self.renderer, &self.current_bytes);
                });
            }
        });
    }
}

/// A short label for a renderer, for the status bar.
fn renderer_label(renderer: &Renderer) -> &'static str {
    match renderer {
        Renderer::Html => "HTML",
        Renderer::Markdown => "Markdown",
        Renderer::Csv => "CSV",
        Renderer::Code(_) => "Code",
        Renderer::Text => "Text",
    }
}
