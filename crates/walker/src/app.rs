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
use walrus_fmt::Variant;

use crate::chrome::{self, ExportContext, StatusInfo};
use crate::import::{ImportState, RfdPicker};
use crate::navigator::{self, NavigatorState};
use crate::player::Player;
use crate::source::{sniff_backend, Backend, HistorySource, WalrusSource};
use crate::theme::{self, ThemeMode};
use crate::transport;
use crate::view::{self, Renderer};

/// Which backend the open read source is, for the status-bar variant label.
///
/// The player no longer owns a `WalrusFile`, so the variant (`wal`/`wbl`) can no
/// longer be read from it. The app tracks this tag alongside the boxed read
/// source instead. Automerge has no `.wal`/`.wbl` variant, so it is its own arm.
enum SourceKind {
    /// A `.wal`/`.wbl` archive, carrying its forward/reverse variant.
    Walrus(Variant),
    /// An Automerge document (read-only; no `.wal`/`.wbl` variant).
    ///
    /// Constructed only by the feature-ON Automerge open path; with the feature
    /// off the variant is never built (but `variant_str` still matches it), so
    /// the dead-code allow is scoped to the feature-off build.
    #[cfg_attr(not(feature = "automerge"), allow(dead_code))]
    Automerge,
}

// The FR-K6 per-file + batch control handlers (apply a `FileAction` through the
// concrete `WalrusSource` and refresh), plus the FR-K5 import helpers and the
// FR-K9 history-report save, live in a sibling module split out so `app.rs`
// stays under the 400 non-test-line standard (NFR-1). `app` is a file module,
// so the child is declared with an explicit `#[path]`.
#[path = "app_mutate.rs"]
mod app_mutate;

// The pure app-shell view helpers (renderer selection for the active file,
// per-file document metadata, the status-bar renderer label) live in a sibling
// module split out so `app.rs` stays under the 400 non-test-line standard
// (NFR-1). Re-exported into this module's scope so both this shell and
// `app_mutate` (via `super::navigator_renderer`) resolve them unchanged.
#[path = "app_view.rs"]
mod app_view;
use app_view::{navigator_renderer, renderer_label};

/// The Walker application state.
pub struct WalkerApp {
    player: Option<Player>,
    /// The boxed, read-only history source for the open document, if any.
    ///
    /// ALL reads (navigator listing, viewer reconstruct, status bar, export) go
    /// through this format-agnostic [`HistorySource`] via the `source()`
    /// accessor, so no read path learns the concrete backend. A `.wal`/`.wbl`
    /// open populates this with a boxed `WalrusSource`; a future Automerge open
    /// populates it with a boxed `AutomergeSource` and leaves `write_source`
    /// `None`.
    read_source: Option<Box<dyn HistorySource>>,
    /// The CONCRETE `WalrusSource` for the FR-K5/FR-K6 WRITE path
    /// (import/mutate), if the open document is a writable `.wal`/`.wbl` archive.
    ///
    /// The read-only trait deliberately does not carry a write path, so the
    /// concrete handle is kept separately. An Automerge document is read-only and
    /// leaves this `None` (its write controls are inert, which is correct).
    write_source: Option<WalrusSource>,
    /// Which backend the open read source is, for the status-bar variant label.
    source_kind: Option<SourceKind>,
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
    /// The file whose FR-K9 history view is currently open, if any.
    history_file: Option<crate::source::FileRef>,
    /// The path the open archive was loaded from, if any. The FR-K6 mutate
    /// controls persist back to this path transactionally.
    path: Option<std::path::PathBuf>,
}

impl Default for WalkerApp {
    fn default() -> Self {
        Self {
            player: None,
            read_source: None,
            write_source: None,
            source_kind: None,
            navigator: NavigatorState::default(),
            import: ImportState::default(),
            current_bytes: Vec::new(),
            renderer: Renderer::Text,
            status: "Open a .wal or .wbl file to begin.".to_string(),
            last_frame: Instant::now(),
            speed: 1.0,
            theme_mode: ThemeMode::default(),
            theme_applied: false,
            history_file: None,
            path: None,
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
    ///
    /// Builds BOTH the boxed read source (for the viewer/player/status-bar/
    /// export seam) and the concrete `WalrusSource` write handle (for the
    /// FR-K5/FR-K6 mutate path) from the same path. The player is built from the
    /// active file's version count through the trait, so playback flows through
    /// one byte path regardless of backend.
    fn open_file(&mut self, path: std::path::PathBuf) {
        // The leading bytes -- NOT the extension -- decide the backend
        // (FR-K11.1, FORMAT.md section 2). Read a short head and sniff it; a
        // read failure surfaces the same way a failed open does.
        let head = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.status = format!("Failed to open {}: {e}", path.display());
                return;
            }
        };
        match sniff_backend(&head) {
            Backend::Automerge => self.open_automerge(path),
            Backend::Walrus => self.open_walrus(path),
        }
    }

    /// Open an Automerge document sniffed from the magic bytes.
    ///
    /// Gated on the `automerge` feature: with the feature ON this builds an
    /// `AutomergeSource` into the boxed read source (read-only; the concrete
    /// `WalrusSource` write handle stays `None`). With the feature OFF a
    /// recognised Automerge file is NOT mis-opened as a walrus container; the
    /// status reports that support is not built in and nothing is loaded.
    #[cfg(feature = "automerge")]
    fn open_automerge(&mut self, path: std::path::PathBuf) {
        use crate::source::AutomergeSource;

        // The read-only format-agnostic source. Automerge has no `.wal`/`.wbl`
        // write path, so the concrete `WalrusSource` write handle stays `None`
        // (the FR-K5/FR-K6 mutate controls are inert, which is correct).
        let read = match AutomergeSource::open(&path) {
            Ok(source) => source,
            Err(e) => {
                self.status = format!("Failed to open {}: {e}", path.display());
                return;
            }
        };
        let (player, len) = self.bind_player(&read);
        self.status = format!("Opened {} ({} versions)", path.display(), len);
        self.write_source = None;
        self.source_kind = Some(SourceKind::Automerge);
        self.read_source = Some(Box::new(read));
        self.player = Some(player);
        self.path = Some(path);
        self.refresh_bytes();
    }

    /// Feature-OFF stub for an Automerge-sniffed file: report that Automerge
    /// support is not built into this Walker and load nothing. The leading
    /// bytes already identified the file as Automerge, so it must NOT fall
    /// through to the walrus reader.
    #[cfg(not(feature = "automerge"))]
    fn open_automerge(&mut self, _path: std::path::PathBuf) {
        self.status =
            "Automerge support is not built into this Walker (rebuild with --features automerge)."
                .to_string();
    }

    /// Open a `.wal`/`.wbl` archive (the Phase-1 path, unchanged).
    ///
    /// Builds BOTH the boxed read source (for the viewer/player/status-bar/
    /// export seam) and the concrete `WalrusSource` write handle (for the
    /// FR-K5/FR-K6 mutate path) from the same path.
    fn open_walrus(&mut self, path: std::path::PathBuf) {
        // The format-agnostic read source is the single source of truth for the
        // navigator listing, the viewer, the status bar, and export. This is one
        // of only two app-shell sites that names `WalrusSource` (the other is the
        // write handle below); every read path sees `&dyn HistorySource`.
        //
        // Opening it is also the fail-fast point: a precise message surfaces
        // here if the archive cannot be read. The forward/reverse variant for
        // the status bar is read from this same open source via
        // `WalrusSource::variant`, NOT by re-opening the file with the legacy
        // single-file reader (which cannot read a multi-file archive).
        let read = match WalrusSource::open(&path) {
            Ok(source) => source,
            Err(e) => {
                self.status = format!("Failed to open {}: {e}", path.display());
                return;
            }
        };
        let variant = read.variant();
        let (player, len) = self.bind_player(&read);
        self.status = format!("Opened {} ({} versions)", path.display(), len);

        // The concrete write handle for the FR-K5/FR-K6 mutate path, reopened
        // from the same path. A walrus open populates both; the SourceKind stays
        // Walrus.
        self.write_source = WalrusSource::open(&path).ok();
        self.source_kind = Some(SourceKind::Walrus(variant));
        self.read_source = Some(Box::new(read));
        self.player = Some(player);
        // Remember the path so the FR-K6 mutate controls can persist back to it
        // transactionally.
        self.path = Some(path);
        // Refresh the cached bytes through the one source-driven path.
        self.refresh_bytes();
    }

    /// Default the active selection to the first tracked file, bind the
    /// viewer's renderer through the seam, and build the position-only player
    /// from that file's version count (0 = current, far index = original).
    /// Returns the player and the far index `len`. One path for both backends.
    fn bind_player(&mut self, read: &dyn HistorySource) -> (Player, usize) {
        let active = read.files().first().cloned();
        if let Some(first) = active.as_ref() {
            self.navigator.select(first);
            self.renderer = navigator_renderer(read, &self.navigator);
        }
        let len = active
            .as_ref()
            .map(|f| read.versions(f).len().saturating_sub(1))
            .unwrap_or(0);
        let mut player = Player::new(len);
        player.seek(0);
        transport::set_speed_multiplier(&mut player, self.speed);
        (player, len)
    }

    /// Refresh the cached current-version bytes from the ACTIVE SOURCE at the
    /// player's current index.
    ///
    /// One path for both backends: it reads the active source at
    /// `source.version_at(active, player.index())` then `reconstruct`. This
    /// replaces the three former `player.current_bytes()` call sites, so an
    /// Automerge document and a walrus document refresh through identical code.
    fn refresh_bytes(&mut self) {
        let fetched = match (
            self.source(),
            self.navigator.active().cloned(),
            self.player.as_ref(),
        ) {
            (Some(source), Some(active), Some(player)) => {
                let version = source.version_at(&active, player.index());
                source.reconstruct(&active, &version)
            }
            _ => Ok(Vec::new()),
        };
        self.current_bytes = match fetched {
            Ok(bytes) => bytes,
            Err(e) => format!("<reconstruction error: {e}>").into_bytes(),
        };
    }

    /// The variant string of the loaded source, if any.
    fn variant_str(&self) -> &'static str {
        match self.source_kind.as_ref() {
            Some(SourceKind::Walrus(Variant::Wal)) => "wal",
            Some(SourceKind::Walrus(Variant::Wbl)) => "wbl",
            Some(SourceKind::Automerge) => "amrg",
            None => "-",
        }
    }

    /// Open the native file dialog and load the chosen file.
    ///
    /// The dialog's extension filter is a HINT only; the magic-byte sniff in
    /// `open_file` decides the backend. The `automerge`/`amrg` hint extensions
    /// are offered only when the `automerge` feature is built in.
    fn prompt_open(&mut self) {
        #[cfg(feature = "automerge")]
        let extensions: &[&str] = &["wal", "wbl", "automerge", "amrg"];
        #[cfg(not(feature = "automerge"))]
        let extensions: &[&str] = &["wal", "wbl"];
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Walrus", extensions)
            .pick_file()
        {
            self.open_file(path);
        }
    }

    /// The open read source as a read-only [`HistorySource`], if any.
    ///
    /// Every read path (navigator, viewer, status bar, export) borrows the boxed
    /// read source through this accessor, so none of them learns the concrete
    /// backend.
    fn source(&self) -> Option<&dyn HistorySource> {
        self.read_source.as_deref()
    }

    /// Render the FR-K9 history view for the open file, if any, and run the
    /// `rfd` save when the panel yields an export intent.
    ///
    /// The panel reads through the format-agnostic seam; only the save dialog
    /// touches the OS. A no-op when no history file is open or no source is
    /// loaded.
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
            self.status = app_mutate::save_history_report(&file, &markdown);
        }
    }

    /// Build the export context for the chrome menu, if a file is loaded.
    ///
    /// The document name/mime come from the ACTIVE SOURCE via `content_hint`
    /// (the player no longer owns a `WalrusFile`), so export works identically
    /// for both backends.
    fn export_context(&self) -> Option<(Vec<walrus_fmt::export::ExportFormat>, String, String)> {
        let source = self.source()?;
        let active = self.navigator.active()?;
        let (mime, name) = source.content_hint(active);
        let offered = walrus_fmt::export::offered_formats(&mime);
        Some((offered, mime, name))
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
            self.refresh_bytes();
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
                // FR-K9: open the per-file history view for the clicked file.
                self.history_file = Some(file);
            }
            if let Some(message) = import_error {
                self.status = message;
            }
            if let Some(outcome) = import_outcome {
                // FR-K5: a resolved import applies through the concrete source
                // (the write path) and refreshes the list/selection.
                self.apply_import(outcome);
            }
            if let Some(fa) = file_action {
                // FR-K6: a per-file or batch control applies through the
                // concrete source's transactional mutate path and refreshes.
                self.handle_file_action(fa);
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

        // FR-K9 history view: a right panel listing the open file's change log
        // with a Markdown-report export. Rendered only when a file's history was
        // requested and a source is loaded.
        self.show_history_panel(ui);

        // Bottom status bar. The file name, mime, and structured note come from
        // the ACTIVE SOURCE through the trait (the player no longer owns a
        // `WalrusFile`); the index/total come from the position-only player.
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

        // Central content: transport strip + the content-aware viewer.
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
                // The per-frame VersionId ALWAYS comes from the source's
                // `version_at(active, index)`: `Index(index)` for walrus/
                // in-memory sources (behaviour-identical to Phase 1), the `Heads`
                // for that linear point for an Automerge source.
                let version = active.as_ref().zip(self.source()).map(|(file, source)| {
                    source.version_at(file, self.player.as_ref().map_or(0, Player::index))
                });
                egui::ScrollArea::both().show(ui, |ui| {
                    match (self.source(), active.as_ref(), version.as_ref()) {
                        // Drive the viewer through the seam for the selected
                        // file: reconstruct + paint via one byte path (AC2).
                        (Some(source), Some(active), Some(version)) => {
                            view::render(ui, source, active, version);
                        }
                        // No source/selection yet: the single-document fallback.
                        _ => view::paint(ui, &self.renderer, &self.current_bytes),
                    }
                });
            }
        });
    }
}

// === Tests ==============================================================
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write `bytes` to a uniquely named file under a fresh temp dir and return
    /// both so the dir lives for the test's duration.
    fn temp_file_with(bytes: &[u8], name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("make temp dir");
        let path = dir.path().join(name);
        let mut f = std::fs::File::create(&path).expect("create fixture file");
        f.write_all(bytes).expect("write fixture bytes");
        (dir, path)
    }

    /// A file whose leading bytes are the Automerge magic. The remaining bytes
    /// are junk: a feature-OFF open never decodes it (it is rejected at the
    /// sniff), and the feature-ON arm is proven by the headless harness with a
    /// REAL document, so junk is enough to exercise the sniff routing here.
    ///
    /// Gated to feature-OFF: its only caller is the
    /// `#[cfg(not(feature = "automerge"))]` not-built-in test below, so without
    /// this gate the helper is dead code (and a `-D warnings` failure) in the
    /// feature-ON build.
    #[cfg(not(feature = "automerge"))]
    fn automerge_magic_file() -> (tempfile::TempDir, std::path::PathBuf) {
        temp_file_with(&[0x85, 0x6F, 0x4A, 0x83, 0x00, 0x01, 0x02], "doc.amrg")
    }

    /// FR-K11 AC1 (shell half): a file whose leading bytes are the Automerge
    /// magic routes to the Automerge arm, NOT the walrus arm, regardless of its
    /// `.amrg` extension being unknown to the walrus reader.
    ///
    /// Feature OFF: the Automerge arm reports "not built in" and loads nothing
    /// (no source, no player) rather than mis-opening it as a walrus file.
    #[cfg(not(feature = "automerge"))]
    #[test]
    fn an_automerge_sniffed_file_reports_not_built_in_when_the_feature_is_off() {
        let (_dir, path) = automerge_magic_file();
        let mut app = WalkerApp::default();
        app.open_file(path);
        assert!(
            app.status.contains("Automerge support is not built"),
            "feature-off open of an Automerge-sniffed file must report it is \
             not built in, got: {}",
            app.status
        );
        assert!(
            app.read_source.is_none(),
            "feature-off Automerge open must load no source"
        );
        assert!(
            app.player.is_none(),
            "feature-off Automerge open must build no player"
        );
    }

    /// FR-K11 AC2 (feature off, Phase-1 unchanged): a non-Automerge file still
    /// routes to the walrus arm. A junk (non-container) file surfaces the
    /// walrus reader's precise open error, NOT the Automerge "not built in"
    /// message, proving the sniff did not misroute it.
    #[test]
    fn a_non_automerge_file_routes_to_the_walrus_arm() {
        let (_dir, path) = temp_file_with(b"not a real walrus container", "junk.wal");
        let mut app = WalkerApp::default();
        app.open_file(path);
        assert!(
            !app.status.contains("Automerge support is not built"),
            "a non-Automerge file must not hit the Automerge arm, got: {}",
            app.status
        );
        assert!(
            app.status.contains("Failed to open"),
            "a junk .wal file routes to the walrus arm and surfaces its open \
             error, got: {}",
            app.status
        );
    }
}
