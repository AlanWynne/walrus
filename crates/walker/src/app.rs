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
/// The player holds only a position, not the opened archive, so the variant
/// (`wal`/`wbl`) is tracked here alongside the boxed read source. Automerge has
/// no `.wal`/`.wbl` variant, so it is its own arm.
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

    /// Rebuild the position-only player from the CURRENTLY ACTIVE file's version
    /// count after a mutate changed it, seeking to index 0 (current). Unlike
    /// `bind_player` (which keys off the first tracked file at open time), this
    /// keys off the active selection so an import/update/condense that altered
    /// the viewed file's version range leaves the player's far-index correct. A
    /// no-op when nothing is open or no file is active.
    fn rebind_player_to_active(&mut self) {
        let len = match (self.source(), self.navigator.active().cloned()) {
            (Some(source), Some(active)) => source.versions(&active).len().saturating_sub(1),
            _ => return,
        };
        let mut player = Player::new(len);
        player.seek(0);
        transport::set_speed_multiplier(&mut player, self.speed);
        self.player = Some(player);
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
    /// (the player holds only a position), so export works identically for both
    /// backends.
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
                // holds position-only state the seam does not expose, so FR-K4
                // binds through the trait (model-level AC2, proven headlessly).
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
        // the ACTIVE SOURCE through the trait (the player holds only a
        // position); the index/total come from the position-only player.
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

    // === Real multi-file `.wal` fixtures for driving the shell ==========
    //
    // These build REAL compressed containers (ustar tar + zstd) from the
    // library's public manifest types, so a `WalkerApp` opens them through the
    // same `WalrusSource::open` path the GUI uses -- no mirror of the open logic.
    // Modelled verbatim on the `single_forward_container`/`append` helpers in
    // `src/source/walrus.rs`'s test module (that module's helpers are private to
    // it, so a small local copy is unavoidable; it is test-only, excluded from
    // the NFR-1 cap).
    use walrus_fmt::{ChangeRecord, Direction, Manifest, TrackedFile, WchgHeader};

    /// One tracked file for a fixture: its archive id, display name, base bytes,
    /// and the forward splice records that build its newer versions.
    struct FixtureFile<'a> {
        id: &'a str,
        name: &'a str,
        base: &'a [u8],
        records: &'a [ChangeRecord],
    }

    /// Append a ustar member to the tar builder (mode 0o644, mtime 0).
    fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) {
        let mut header = tar::Header::new_ustar();
        header.set_path(path).expect("path");
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder.append(&header, data).expect("append");
    }

    /// Assemble a forward (`.wal`) container holding `files`, each with its own
    /// base + `.wchg` log, zstd-packed -- the exact shape `WalrusSource::open`
    /// reads.
    fn multi_file_wal(files: &[FixtureFile<'_>]) -> Vec<u8> {
        let mut manifest = Manifest::new(Variant::Wal);
        for f in files {
            let count = f.records.iter().filter(|r| r.is_splice()).count();
            manifest.files.push(TrackedFile::new_with_direction(
                f.id,
                f.name,
                Direction::Forward,
                count,
            ));
        }

        let mut builder = tar::Builder::new(Vec::new());
        append(
            &mut builder,
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest).expect("manifest"),
        );
        for f in files {
            let count = f.records.iter().filter(|r| r.is_splice()).count();
            let header = WchgHeader::new(Direction::Forward, 0, f.name, "utf-8", count);
            let mut wchg = serde_json::to_string(&header).expect("header");
            wchg.push('\n');
            for record in f.records {
                wchg.push_str(&serde_json::to_string(record).expect("record"));
                wchg.push('\n');
            }
            append(&mut builder, &format!("files/{}/base", f.id), f.base);
            append(
                &mut builder,
                &format!("files/{}/{}.wchg", f.id, f.name),
                wchg.as_bytes(),
            );
        }
        let tar_bytes = builder.into_inner().expect("tar");
        zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd")
    }

    /// A single base-only file with zero splices: a valid archive whose only
    /// version is the base (`count == 0`), used for the empty/zero-version
    /// render pass (point 4).
    fn zero_version_wal() -> Vec<u8> {
        multi_file_wal(&[FixtureFile {
            id: "z0",
            name: "empty.txt",
            base: b"",
            records: &[],
        }])
    }

    /// The canonical multi-file fixture: file 0 `notes.txt` has base "cat" plus
    /// two forward splices (so its version `count == 2`), file 1 `data.csv` is
    /// base-only. Returns the fixture bytes.
    ///
    /// Forward index semantics (FORMAT.md section 5): index 0 is CURRENT, the
    /// far index == splice count is the ORIGINAL. The two splices build newer
    /// versions on top of "cat", so index 0 differs from index 2.
    fn canonical_multi_file_wal() -> Vec<u8> {
        // base "cat"; splice @0 del 0 ins "the " -> "the cat"; splice @7 del 0
        // ins "!" -> "the cat!". Oldest step first in log order.
        let notes_records = [
            ChangeRecord::splice(0, 0, "the "),
            ChangeRecord::splice(7, 0, "!"),
        ];
        multi_file_wal(&[
            FixtureFile {
                id: "f0",
                name: "notes.txt",
                base: b"cat",
                records: &notes_records,
            },
            FixtureFile {
                id: "f1",
                name: "data.csv",
                base: b"a,b\n1,2\n",
                records: &[],
            },
        ])
    }

    /// Write fixture `bytes` named `name` under a fresh temp dir; keep the dir
    /// alive for the test's duration.
    fn write_temp_wal(bytes: &[u8], name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        temp_file_with(bytes, name)
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

    // === Driving the REAL WalkerApp shell (audit section 3.1 gap) =======
    //
    // These tests construct a real `WalkerApp` and drive it through its OWN
    // `open_file`/`open_walrus`/`refresh_bytes` + render path over a real
    // multi-file `.wal`, asserting on the shell's private model state. They
    // replace reliance on mirror-closure harnesses for the shell open path: a
    // divergence between the shell and a hand-written mirror would now be caught.
    //
    // MANUAL (NOT tested here, cannot be driven headlessly): `prompt_open`
    // (OS-native `rfd` open dialog) and the `rfd` SAVE inside the FR-K9 history
    // panel (`show_history_panel` -> `app_mutate::save_history_report`). A
    // headless accesskit harness cannot drive an OS-native file dialog, so both
    // stay manual by design.

    /// Task points 1 and 2: opening a real multi-file `.wal` through the shell
    /// wires the active selection, sizes the player to the active file's version
    /// count, records the Walrus variant and a success status, and caches
    /// `current_bytes` equal to a direct reconstruct at index 0.
    #[test]
    fn opening_a_multi_file_wal_through_the_shell_wires_selection_player_and_bytes() {
        let (_dir, path) = write_temp_wal(&canonical_multi_file_wal(), "notes.wal");
        let mut app = WalkerApp::default();
        app.open_file(path);

        // Point 1: the first tracked file is the active selection.
        let active = app
            .navigator
            .active()
            .cloned()
            .expect("open must select the first tracked file");
        assert_eq!(
            active.id(),
            0,
            "the FIRST tracked file is active after open"
        );
        assert_eq!(active.name(), "notes.txt");

        // Point 1: the player exists and is sized to the active file's version
        // count (far index == splice count == 2 for notes.txt).
        let source = app.source().expect("open must install a read source");
        let far_index = source.versions(&active).len().saturating_sub(1);
        assert_eq!(
            far_index, 2,
            "notes.txt has two forward splices (count == 2)"
        );
        let player = app.player.as_ref().expect("open must build a player");
        assert_eq!(
            player.len(),
            far_index,
            "the player is sized to the active file's far version index"
        );

        // Point 1: the source_kind is the Walrus forward (.wal) variant.
        assert_eq!(
            app.variant_str(),
            "wal",
            "a .wal archive records the Wal variant"
        );

        // Point 1: status reflects a successful open, not a failure.
        assert!(
            !app.status.contains("Failed to open"),
            "a real archive must open without a failure status, got: {}",
            app.status
        );
        assert!(
            app.status.contains("Opened"),
            "a successful open reports 'Opened ...', got: {}",
            app.status
        );

        // Point 2: current_bytes equals a direct reconstruct at the current
        // version (index 0) through the same source.
        let expected = app
            .source()
            .expect("source")
            .reconstruct(&active, &crate::source::VersionId::Index(0))
            .expect("reconstruct index 0");
        assert_eq!(
            app.current_bytes, expected,
            "current_bytes caches the reconstruct at index 0 (current version)"
        );
        // Sanity: the current version is the newest, built from the two splices.
        assert_eq!(app.current_bytes, b"the cat!");
    }

    /// Task point 3: seeking to an older index and refreshing (the way the shell
    /// wires `refresh_bytes`) updates `current_bytes` to the reconstruct at that
    /// index, which differs from the current (index 0) version.
    #[test]
    fn seeking_older_through_the_shell_refreshes_cached_bytes_to_that_version() {
        let (_dir, path) = write_temp_wal(&canonical_multi_file_wal(), "notes.wal");
        let mut app = WalkerApp::default();
        app.open_file(path);
        let active = app.navigator.active().cloned().expect("active file");

        let current = app.current_bytes.clone();

        // Drive the seek the way the shell does: move the player's index, then
        // call the real `refresh_bytes` (the method the audit flags as untested).
        let older = 2usize; // the far index == original base "cat".
        app.player.as_mut().expect("player").seek(older);
        app.refresh_bytes();

        let expected = app
            .source()
            .expect("source")
            .reconstruct(&active, &crate::source::VersionId::Index(older))
            .expect("reconstruct older index");
        assert_eq!(
            app.current_bytes, expected,
            "after seek + refresh, current_bytes is the reconstruct at the older index"
        );
        assert_eq!(
            app.current_bytes, b"cat",
            "the far index is the original base"
        );
        assert_ne!(
            app.current_bytes, current,
            "the older version genuinely differs from the current version"
        );
    }

    /// Task point 6 (feasible without extra wiring): driving the private
    /// `open_walrus` directly (bypassing the magic-byte sniff) wires the SAME
    /// selection/player/variant/current_bytes invariants as `open_file`. Pins
    /// `open_walrus` -- the highest-risk untested path the audit names --
    /// independently of the sniff routing.
    #[test]
    fn open_walrus_directly_wires_the_same_invariants_as_open_file() {
        let (_dir, path) = write_temp_wal(&canonical_multi_file_wal(), "notes.wal");
        let mut app = WalkerApp::default();
        app.open_walrus(path);

        let active = app.navigator.active().cloned().expect("active file");
        assert_eq!(active.id(), 0);
        assert_eq!(app.variant_str(), "wal");
        assert!(app.status.contains("Opened"), "got: {}", app.status);
        let player = app.player.as_ref().expect("player");
        assert_eq!(player.len(), 2);
        let expected = app
            .source()
            .expect("source")
            .reconstruct(&active, &crate::source::VersionId::Index(0))
            .expect("reconstruct index 0");
        assert_eq!(app.current_bytes, expected);
    }

    /// Task point 4: the full render path does not panic on a normal open AND on
    /// an empty/zero-version archive. Each case builds the REAL `WalkerApp` as
    /// the eframe harness state and runs a FIXED, bounded number of frames (the
    /// builder's default `max_steps`), so the real `WalkerApp::logic` +
    /// `WalkerApp::ui` execute through a kittest-provided `eframe::Frame`.
    /// Reaching the line after `run()` IS the no-panic assertion.
    #[test]
    fn rendering_the_real_app_shell_does_not_panic_on_open_or_empty() {
        // Normal open: a real multi-file archive.
        let (_dir, path) = write_temp_wal(&canonical_multi_file_wal(), "notes.wal");
        let mut harness = egui_kittest::Harness::builder().build_eframe(
            |cc: &mut eframe::CreationContext<'_>| {
                let mut app = WalkerApp::new(&*cc);
                app.open_file(path);
                app
            },
        );
        harness.run();
        assert!(
            harness.state().player.is_some(),
            "the shell renders a loaded archive with a bound player"
        );

        // Empty / zero-version archive: a single base-only file (count == 0).
        // The central panel's `player` is Some over a zero-length timeline and
        // the navigator/status panels paint a single base-only file.
        let (_zdir, zpath) = write_temp_wal(&zero_version_wal(), "empty.wal");
        let mut zero = egui_kittest::Harness::builder().build_eframe(
            |cc: &mut eframe::CreationContext<'_>| {
                let mut app = WalkerApp::new(&*cc);
                app.open_file(zpath);
                app
            },
        );
        zero.run();
        let zplayer = zero.state().player.as_ref().expect("zero-version player");
        assert_eq!(
            zplayer.len(),
            0,
            "a base-only file has a zero-length timeline"
        );

        // A freshly-constructed shell with NOTHING open must also render (the
        // "No file loaded." central-panel fallback arm).
        let mut empty = egui_kittest::Harness::builder().build_eframe(|cc| WalkerApp::new(&*cc));
        empty.run();
        assert!(
            empty.state().player.is_none(),
            "an unopened shell has no player and still renders"
        );
    }

    // === Driving the REAL shell MUTATE/IMPORT path (audit section 3 gap) =
    //
    // Task point 4: these drive `WalkerApp::apply_import` and
    // `WalkerApp::handle_file_action` -- the SHELL mutate/import methods --
    // against a real archive on a `TempDir`, asserting BOTH the transactional
    // on-disk change AND the shell's in-memory refresh (read source, selection,
    // `current_bytes`). This closes a gap distinct from `tests/import.rs`: that
    // suite drives `WalrusSource::import` (the underlying write source) directly
    // but NEVER the `WalkerApp::apply_import`/`handle_file_action` wiring that
    // calls it and then re-syncs the read source + selection. A divergence
    // between that shell wiring and the write source would slip past the
    // existing tests; it is caught here.
    //
    // No `rfd` dialog is involved: `apply_import` takes an already-resolved
    // `ImportOutcome` (the OS pick happens upstream in the navigator pane, which
    // `tests/import.rs` drives with the `FakePicker`), so building an
    // `ImportOutcome` directly is the exact seam the shell consumes.

    use crate::import::{ImportOutcome, ImportResolution};
    use crate::navigator::controls::FileAction;
    use crate::source::VersionId;

    /// Open the canonical multi-file `.wal` into a shell and keep its temp dir
    /// alive. The write source + path are set (a real walrus open), so the
    /// mutate/import methods have a target.
    fn open_shell_over_canonical() -> (tempfile::TempDir, WalkerApp) {
        let (dir, path) = write_temp_wal(&canonical_multi_file_wal(), "notes.wal");
        let mut app = WalkerApp::default();
        app.open_file(path);
        (dir, app)
    }

    /// Validates: app-shell import coverage gap (audit section 3) -- the SHELL
    /// `apply_import` ADD path, not the write source directly.
    ///
    /// Task point 4: applying an `Add` import through `WalkerApp::apply_import`
    /// adds the file to the archive ON DISK (transactionally), rebuilds the read
    /// source so the navigator lists the new file, and selects it.
    #[test]
    fn apply_import_add_through_the_shell_adds_a_file_on_disk_and_refreshes() {
        let (dir, mut app) = open_shell_over_canonical();
        let path = dir.path().join("notes.wal");

        let outcome = ImportOutcome {
            path: dir.path().join("extra.txt"),
            bytes: b"brand new".to_vec(),
            resolution: ImportResolution::Add {
                name: "extra.txt".to_string(),
            },
        };
        app.apply_import(outcome);

        // The shell reported success and re-selected the imported file.
        assert!(
            app.status.contains("Imported extra.txt"),
            "apply_import reports the imported name, got: {}",
            app.status
        );
        let active = app
            .navigator
            .active()
            .cloned()
            .expect("active after import");
        assert_eq!(
            active.name(),
            "extra.txt",
            "the shell selects the just-imported file"
        );

        // The read source was rebuilt from disk and lists the new file.
        let shell_names: Vec<String> = app
            .source()
            .expect("read source after import")
            .files()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        assert!(
            shell_names.contains(&"extra.txt".to_string()),
            "the refreshed read source lists the imported file, got: {shell_names:?}"
        );

        // The change hit disk transactionally: a fresh open sees it too.
        let reopened = crate::source::WalrusSource::open(&path).expect("re-open");
        let disk = reopened
            .files()
            .into_iter()
            .find(|f| f.name() == "extra.txt")
            .expect("imported file is tracked on disk");
        assert_eq!(
            reopened
                .reconstruct(&disk, &VersionId::Index(0))
                .expect("reconstruct current"),
            b"brand new",
            "the imported bytes are the current version on disk"
        );
    }

    /// Validates: app-shell import coverage gap (audit section 3) -- the SHELL
    /// `apply_import` UPDATE (append) path.
    ///
    /// Task point 4: applying a confirmed `UpdatePrompt` import of an EXISTING
    /// name through the shell appends exactly one version on disk and refreshes
    /// `current_bytes` to the new current version.
    #[test]
    fn apply_import_update_through_the_shell_appends_a_version_and_refreshes_bytes() {
        let (dir, mut app) = open_shell_over_canonical();
        let path = dir.path().join("notes.wal");

        // Version count of notes.txt before: indices [0, 2] inclusive == 3.
        let before = app
            .source()
            .expect("source")
            .file_info(&app.navigator.active().cloned().expect("active"))
            .index_size;

        let outcome = ImportOutcome {
            path: dir.path().join("notes.txt"),
            bytes: b"a wholly new revision".to_vec(),
            // An existing name confirmed to append (the FR-K5 UpdatePrompt the
            // navigator surfaces, here already resolved to the update intent).
            resolution: ImportResolution::UpdatePrompt {
                name: "notes.txt".to_string(),
            },
        };
        app.apply_import(outcome);

        assert!(
            app.status.contains("Imported notes.txt"),
            "apply_import reports the updated name, got: {}",
            app.status
        );

        // Exactly one version was appended on disk.
        let reopened = crate::source::WalrusSource::open(&path).expect("re-open");
        let file = reopened
            .files()
            .into_iter()
            .find(|f| f.name() == "notes.txt")
            .expect("notes.txt still tracked");
        assert_eq!(
            reopened.file_info(&file).index_size,
            before + 1,
            "a confirmed update appends exactly one version on disk"
        );
        assert_eq!(
            reopened
                .reconstruct(&file, &VersionId::Index(0))
                .expect("reconstruct current"),
            b"a wholly new revision",
            "index 0 on disk is the newly imported current version"
        );

        // The shell refreshed its cached current bytes to the new version.
        assert_eq!(
            app.current_bytes, b"a wholly new revision",
            "the shell refreshed current_bytes to the appended current version"
        );
    }

    /// Validates: app-shell mutate coverage gap (audit section 3) -- the SHELL
    /// `handle_file_action` REMOVE path.
    ///
    /// Task point 4: a `FileAction::Remove` through the shell removes the file
    /// from the archive on disk (transactionally) and refreshes the read source
    /// so the navigator no longer lists it.
    #[test]
    fn handle_file_action_remove_through_the_shell_drops_a_file_on_disk_and_refreshes() {
        let (dir, mut app) = open_shell_over_canonical();
        let path = dir.path().join("notes.wal");

        // Precondition: both files are tracked (data.csv is id 1).
        let before: Vec<String> = app
            .source()
            .expect("source")
            .files()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        assert!(
            before.contains(&"data.csv".to_string()),
            "precondition: data.csv is tracked, got: {before:?}"
        );

        app.handle_file_action(FileAction::Remove { id: 1 });

        assert!(
            app.status.contains("Removed file 1"),
            "handle_file_action reports the removal, got: {}",
            app.status
        );

        // The refreshed read source no longer lists the removed file.
        let after: Vec<String> = app
            .source()
            .expect("read source after remove")
            .files()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        assert!(
            !after.contains(&"data.csv".to_string()),
            "the refreshed read source dropped the removed file, got: {after:?}"
        );

        // The change hit disk transactionally: a fresh open agrees.
        let reopened = crate::source::WalrusSource::open(&path).expect("re-open");
        let disk_names: Vec<String> = reopened
            .files()
            .iter()
            .map(|f| f.name().to_string())
            .collect();
        assert!(
            !disk_names.contains(&"data.csv".to_string()),
            "the removed file is gone on disk, got: {disk_names:?}"
        );
        assert!(
            disk_names.contains(&"notes.txt".to_string()),
            "the untouched file survives on disk, got: {disk_names:?}"
        );
    }

    /// Validates: app-shell mutate coverage gap (audit section 3) -- the SHELL
    /// `handle_file_action` SET-GENERATIONS path.
    ///
    /// Task point 4: a `FileAction::SetGenerations` through the shell persists
    /// the generation cap to the archive on disk (transactionally) and the
    /// refreshed read source reports the new cap.
    #[test]
    fn handle_file_action_set_generations_through_the_shell_persists_the_cap() {
        let (dir, mut app) = open_shell_over_canonical();
        let path = dir.path().join("notes.wal");

        // Precondition: notes.txt (id 0) has no cap (unlimited -> None).
        let file0 = app.navigator.active().cloned().expect("active");
        assert_eq!(
            app.source().expect("source").file_info(&file0).generations,
            None,
            "precondition: no generation cap is set"
        );

        app.handle_file_action(FileAction::SetGenerations { id: 0, n: Some(1) });

        assert!(
            app.status.contains("Set generation cap on file 0"),
            "handle_file_action reports the cap change, got: {}",
            app.status
        );

        // The refreshed read source reports the new cap ...
        let shell_file = app
            .source()
            .expect("read source after set-generations")
            .files()
            .into_iter()
            .find(|f| f.name() == "notes.txt")
            .expect("notes.txt still tracked");
        assert_eq!(
            app.source()
                .expect("source")
                .file_info(&shell_file)
                .generations,
            Some(1),
            "the refreshed read source reports the new generation cap"
        );

        // ... and it persisted to disk transactionally.
        let reopened = crate::source::WalrusSource::open(&path).expect("re-open");
        let disk_file = reopened
            .files()
            .into_iter()
            .find(|f| f.name() == "notes.txt")
            .expect("notes.txt on disk");
        assert_eq!(
            reopened.file_info(&disk_file).generations,
            Some(1),
            "the generation cap persisted to disk"
        );
    }
}
