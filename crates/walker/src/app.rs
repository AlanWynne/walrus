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
use crate::import::{ImportState, RfdPicker};
use crate::navigator::{self, NavigatorState};
use crate::player::Player;
use crate::source::{HistorySource, VersionId, WalrusSource};
use crate::theme::{self, ThemeMode};
use crate::transport;
use crate::view::{self, Renderer};

/// The Walker application state.
pub struct WalkerApp {
    player: Option<Player>,
    /// The history source for the open archive, if any.
    ///
    /// Reads go through the format-agnostic [`HistorySource`] trait (via the
    /// `source()` accessor), so the navigator and the viewer path stay coded to
    /// the seam. The field is the CONCRETE `WalrusSource` rather than a
    /// `Box<dyn HistorySource>` because FR-K5 introduces a WRITE path
    /// (`WalrusSource::import`) that the read-only trait deliberately does not
    /// carry: Phase 1 has exactly one writable backend. When a second,
    /// read-only backend arrives, reintroduce a boxed trait object for reads and
    /// keep a separate concrete handle only for the writable source.
    source: Option<WalrusSource>,
    /// The navigator's selection model (active file + multi-select set).
    navigator: NavigatorState,
    /// The FR-K5 pending-import model, threaded through the navigator.
    import: ImportState,
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
            source: None,
            navigator: NavigatorState::default(),
            import: ImportState::default(),
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
                let (name, mime) = document_name_and_mime(file.manifest());
                self.renderer = view::select_renderer(&mime, &name);
                let mut player = Player::new(file);
                player.seek(0);
                transport::set_speed_multiplier(&mut player, self.speed);
                self.refresh_bytes(&player);
                self.status = format!("Opened {} ({} versions)", path.display(), player.len());
                self.player = Some(player);

                // Also build the format-agnostic source for the same archive so
                // the navigator can list every tracked file and bind selection
                // through the seam. This is the ONLY app-shell site that names
                // `WalrusSource`; the navigator and viewer path see `&dyn
                // HistorySource` only.
                match WalrusSource::open(&path) {
                    Ok(source) => {
                        // Default the active selection to the first tracked file
                        // so the viewer binds through the seam immediately.
                        if let Some(first) = source.files().first() {
                            self.navigator.select(first);
                            self.renderer = navigator_renderer(&source, &self.navigator);
                        }
                        self.source = Some(source);
                    }
                    Err(e) => {
                        self.source = None;
                        self.status =
                            format!("Opened {} (navigator unavailable: {e})", path.display());
                    }
                }
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

    /// The open source as a read-only [`HistorySource`], if any.
    ///
    /// Reads borrow the concrete `WalrusSource` as `&dyn HistorySource` so the
    /// navigator and viewer stay coded to the format-agnostic seam even though
    /// the field is concrete for the FR-K5 write path.
    fn source(&self) -> Option<&dyn HistorySource> {
        self.source.as_ref().map(|s| s as &dyn HistorySource)
    }

    /// Apply a resolved FR-K5 import through the concrete source, then refresh
    /// the navigator list and the active selection so the new/updated file is
    /// shown. Sets a status string on success or failure.
    ///
    /// The byte-mutation + transactional write live behind the `import-mutate`
    /// feature (the `walrus_fmt` mutate API, FR-W7, is not yet merged on this
    /// branch). Without the feature the import resolves and this records that the
    /// mutate path is pending that cross-stream work, leaving the archive
    /// untouched.
    fn apply_import(&mut self, outcome: crate::import::ImportOutcome) {
        if self.source.is_none() {
            return;
        }
        let target = outcome.resolution.target_name().to_string();
        #[cfg(feature = "import-mutate")]
        {
            let source = self.source.as_mut().expect("source is Some");
            let result = source.import(&outcome.resolution, &outcome.bytes, &outcome.path);
            match result {
                Ok(()) => {
                    self.status = format!("Imported {target}");
                    self.refresh_after_import(&target);
                }
                Err(e) => {
                    self.status = format!("Import of {target} failed: {e}");
                }
            }
        }
        #[cfg(not(feature = "import-mutate"))]
        {
            self.status = format!(
                "Import of {target} resolved; byte-mutation pending the walrus-fmt mutate API (FR-W7)"
            );
        }
    }

    /// Re-select the imported/updated file so the navigator list and the viewer
    /// binding reflect it after a successful mutate (AC1 list refresh).
    #[cfg(feature = "import-mutate")]
    fn refresh_after_import(&mut self, name: &str) {
        if let Some(source) = self.source.as_ref() {
            if let Some(file) = source.files().into_iter().find(|f| f.name() == name) {
                self.navigator.select(&file);
            }
        }
        if let Some(source) = self.source() {
            self.renderer = navigator_renderer(source, &self.navigator);
        }
    }

    /// Build the export context for the chrome menu, if a file is loaded.
    fn export_context(&self) -> Option<(Vec<walrus_fmt::export::ExportFormat>, String, String)> {
        let player = self.player.as_ref()?;
        let (name, mime) = document_name_and_mime(player.manifest());
        let offered = walrus_fmt::export::offered_formats(&mime);
        Some((offered, mime, name))
    }
}

/// The renderer for the navigator's active file, chosen through the seam.
///
/// Falls back to [`Renderer::Text`] when there is no active selection.
fn navigator_renderer(source: &dyn HistorySource, navigator: &NavigatorState) -> Renderer {
    match navigator.active() {
        Some(active) => view::present::select_renderer_for(source, active),
        None => Renderer::Text,
    }
}

/// The document name and MIME for display and renderer selection.
///
/// Document metadata is per-file now; the single-document model keeps it on the
/// first tracked file. Falls back to sensible defaults for an empty manifest.
fn document_name_and_mime(manifest: &walrus_fmt::Manifest) -> (String, String) {
    match manifest.files.first() {
        Some(file) => (file.name.clone(), file.mime.clone()),
        None => (String::new(), "application/octet-stream".to_string()),
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

        // Left navigator: the two-pane file/import browser (FR-K4). Rendered
        // only when a source is open; selecting a file rebinds the viewer
        // through the seam.
        if self.source.is_some() {
            let mut activated = None;
            let mut import_outcome = None;
            let mut import_error = None;
            let picker = RfdPicker;
            egui::Panel::left("navigator").show(ui, |ui| {
                let source = self.source.as_ref().expect("source is Some") as &dyn HistorySource;
                let action =
                    navigator::panels(ui, source, &picker, &mut self.import, &mut self.navigator);
                activated = action.activated;
                import_outcome = action.import;
                import_error = action.import_error;
            });
            if let Some(message) = import_error {
                self.status = message;
            }
            if let Some(outcome) = import_outcome {
                // FR-K5: a resolved import applies through the concrete source
                // (the write path) and refreshes the list/selection.
                self.apply_import(outcome);
            }
            if activated.is_some() {
                // The active file changed: rebind the viewer's renderer through
                // the seam. The central viewer reconstructs the active file at
                // the player's current index via `view::render`, so selection
                // binds the viewer to that file's version timeline (AC2). A live
                // per-file `Player` rebind is deferred app-shell wiring: `Player`
                // owns a `WalrusFile` the seam does not expose, so FR-K4 binds
                // through the trait (model-level AC2, proven headlessly).
                if let Some(source) = self.source() {
                    self.renderer = navigator_renderer(source, &self.navigator);
                }
            }
        }

        // Bottom status bar.
        let variant = self.variant_str();
        let renderer_label = renderer_label(&self.renderer);
        let status = self.status.clone();
        egui::Panel::bottom("status").show(ui, |ui| {
            let info = self.player.as_ref().and_then(|player| {
                // Document metadata is per-file; the single-document model keeps
                // it on the first tracked file.
                let file = player.manifest().files.first()?;
                Some(StatusInfo {
                    file_name: &file.name,
                    variant,
                    index: player.index(),
                    total: player.len(),
                    renderer: renderer_label,
                    mime: &file.mime,
                })
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
                let version = VersionId::Index(self.player.as_ref().map_or(0, Player::index));
                let active = self.navigator.active().cloned();
                egui::ScrollArea::both().show(ui, |ui| {
                    match (self.source(), active.as_ref()) {
                        // Drive the viewer through the seam for the selected
                        // file: reconstruct + paint via one byte path (AC2).
                        (Some(source), Some(active)) => {
                            view::render(ui, source, active, &version);
                        }
                        // No source/selection yet: the single-document fallback.
                        _ => view::paint(ui, &self.renderer, &self.current_bytes),
                    }
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
