//! The app-shell mutate handlers: FR-K6 per-file + batch controls, the FR-K5
//! import apply/refresh, and the FR-K9 history-report save.
//!
//! Split out of `app.rs` (as a `mod app_mutate;` child) so that file stays
//! under the 400 non-test-line standard (NFR-1). Everything here applies a
//! plain, format-agnostic intent through the CONCRETE `WalrusSource` write
//! path: the mutate methods persist transactionally (temp-file-then-rename), so
//! a failed write leaves the archive byte-for-byte unchanged. On any mutate
//! error the handler surfaces the status and RE-OPENS the source from the open
//! path (unchanged bytes) so the in-memory archive and disk stay in lockstep.
//!
//! The `rfd` save dialog an export runs, and the one in `save_history_report`,
//! are the only MANUAL carve-outs (a real OS dialog a headless harness cannot
//! drive); every step before the dialog is pure and tested.

use crate::navigator::controls::FileAction;
use crate::source::{FileRef, HistorySource, VersionId, WalrusSource};

use super::{navigator_renderer, WalkerApp};

impl WalkerApp {
    /// Apply a resolved FR-K5 import through the concrete source, then refresh
    /// the navigator list and the active selection so the new/updated file is
    /// shown. Sets a status string on success or failure.
    ///
    /// The byte-mutation + transactional write go through the `walrus_fmt`
    /// mutate API (`Archive::add`, FR-W7), merged on this branch.
    pub(super) fn apply_import(&mut self, outcome: crate::import::ImportOutcome) {
        if self.source.is_none() {
            return;
        }
        let target = outcome.resolution.target_name().to_string();
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

    /// Re-select a file by name so the navigator list and the viewer binding
    /// reflect it after a successful mutate (AC1 list refresh). Falls back to
    /// the first tracked file when the named file is gone (e.g. after a remove).
    pub(super) fn refresh_after_import(&mut self, name: &str) {
        if let Some(source) = self.source.as_ref() {
            let files = source.files();
            if let Some(file) = files.iter().find(|f| f.name() == name) {
                self.navigator.select(file);
            } else if let Some(first) = files.first() {
                self.navigator.select(first);
            }
        }
        if let Some(source) = self.source() {
            self.renderer = navigator_renderer(source, &self.navigator);
        }
    }

    /// Apply a FR-K6 per-file or batch control intent through the concrete
    /// `WalrusSource` transactional mutate path, set a status string, and
    /// refresh the navigator listing + active selection on success. On a mutate
    /// error the status is surfaced and the source is RE-OPENED from the open
    /// path (unchanged bytes) so memory and disk stay in lockstep. Export
    /// intents route to the reconstruct + save path instead of a mutate.
    pub(super) fn handle_file_action(&mut self, action: FileAction) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Some(source) = self.source.as_mut() else {
            return;
        };
        let result = match &action {
            FileAction::SetGenerations { id, n } => {
                let file = FileRef::new(*id, "");
                source
                    .set_generations(&file, *n, &path)
                    .map(|()| format!("Set generation cap on file {id}"))
            }
            FileAction::Condense { id, keep } => {
                let file = FileRef::new(*id, "");
                source
                    .condense_file(&file, *keep, &path)
                    .map(|()| format!("Condensed file {id} to keep {keep}"))
            }
            FileAction::Remove { id } => {
                let file = FileRef::new(*id, "");
                source
                    .remove_file(&file, &path)
                    .map(|()| format!("Removed file {id}"))
            }
            FileAction::BatchCondense { ids, keep } => source
                .condense_files(ids, *keep, &path)
                .map(|()| format!("Condensed {} files to keep {keep}", ids.len())),
            FileAction::BatchRemove { ids } => source
                .remove_files(ids, &path)
                .map(|()| format!("Removed {} files", ids.len())),
            // Export intents are not mutations: reconstruct + save (below).
            FileAction::ExportFile { .. } | FileAction::BatchExport { .. } => {
                self.handle_export_action(&action);
                return;
            }
        };
        match result {
            Ok(message) => {
                self.status = message;
                self.refresh_after_mutate();
            }
            Err(e) => {
                self.status = format!("Operation failed: {e}");
                // Re-open from the unchanged on-disk bytes so memory and disk
                // stay in lockstep after a mid-op failure.
                self.reopen_source(&path);
            }
        }
    }

    /// Re-derive the active selection and renderer after a successful mutate.
    /// Keeps the current active file selected when it still exists, else binds
    /// the first tracked file (handles the removed-active-file case).
    fn refresh_after_mutate(&mut self) {
        let active_name = self.navigator.active().map(|f| f.name().to_string());
        match active_name {
            Some(name) => self.refresh_after_import(&name),
            None => self.refresh_after_import(""),
        }
    }

    /// Re-open the source from `path` (unchanged bytes) after a failed mutate.
    fn reopen_source(&mut self, path: &std::path::Path) {
        match WalrusSource::open(path) {
            Ok(source) => self.source = Some(source),
            Err(e) => {
                self.status = format!("{} (and re-open failed: {e})", self.status);
                self.source = None;
            }
        }
    }

    /// Reconstruct the current version of the export target(s) and run the save
    /// path. MANUAL: the `rfd` save dialog is a real OS dialog a headless
    /// harness cannot drive; the reconstruct seam before it is pure.
    fn handle_export_action(&mut self, action: &FileAction) {
        let Some(source) = self.source() else {
            return;
        };
        let ids: Vec<usize> = match action {
            FileAction::ExportFile { id } => vec![*id],
            FileAction::BatchExport { ids } => ids.clone(),
            _ => return,
        };
        let files = source.files();
        let mut saved = 0usize;
        for id in ids {
            let Some(file) = files.iter().find(|f| f.id() == id) else {
                continue;
            };
            let bytes = match source.reconstruct(file, &VersionId::Index(0)) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.status = format!("Export of {} failed: {e}", file.name());
                    return;
                }
            };
            // MANUAL carve-out: the `rfd` save dialog.
            if save_bytes(file.name(), &bytes) {
                saved += 1;
            }
        }
        self.status = format!("Exported {saved} file(s)");
    }
}

/// Open the native save dialog with `suggested` as the default name and write
/// `bytes`, returning whether a file was written. MANUAL carve-out.
fn save_bytes(suggested: &str, bytes: &[u8]) -> bool {
    let Some(path) = rfd::FileDialog::new().set_file_name(suggested).save_file() else {
        return false;
    };
    std::fs::write(&path, bytes).is_ok()
}

/// Open the native save dialog and write the FR-K9 Markdown history report,
/// returning a status string (success/cancel/error) -- same pattern as
/// `export_ui::run_export`.
///
/// MANUAL: the `rfd` save dialog is a real OS dialog a headless harness cannot
/// drive; the report STRING is produced and tested by `history_view`.
pub(super) fn save_history_report(file: &FileRef, markdown: &str) -> String {
    let stem = file
        .name()
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(file.name());
    let suggested = format!("{stem}-history.md");
    let Some(path) = rfd::FileDialog::new().set_file_name(&suggested).save_file() else {
        return "History export cancelled.".to_string();
    };
    match std::fs::write(&path, markdown.as_bytes()) {
        Ok(()) => format!("Exported history report to {}", path.display()),
        Err(e) => format!("History export write failed: {e}"),
    }
}
