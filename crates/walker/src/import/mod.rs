//! Import = add/update, resolved automatically (FR-K5).
//!
//! An import takes a file the user picked off the filesystem and folds it into
//! the open archive. The rule (FR-K5): a NEW name is ADDED with no prompt; an
//! EXISTING name defaults to an UPDATE (append a new version) but SURFACES a
//! prompt so the user can confirm the update or add the bytes under a different
//! name instead.
//!
//! This module holds the headless-testable parts of that flow:
//!   - the [`FilePicker`] seam (a real `RfdPicker` + a test fake) so the only
//!     MANUAL carve-out is the OS-native dialog,
//!   - the pure [`ImportResolution`] model + [`resolve_import`] decision and the
//!     confirm / add-as transitions,
//!   - [`ImportState`], the pending-import model the navigator pane renders and
//!     the app shell acts on.
//!
//! Nothing here names a `walrus_fmt` type: the import flow is coded to plain
//! walker-side values (`&str` names, `&[u8]` bytes, `PathBuf`), so the FR-K1
//! AC2 boundary (archive types only in `source/walrus.rs`) holds. The actual
//! byte-mutation lives behind the `WalrusSource` mutate methods in
//! `source/walrus.rs`.

use std::path::PathBuf;

// === The file-picker seam (the one MANUAL carve-out) ====================

/// Produces the file the user chose to import.
///
/// The real implementation ([`RfdPicker`]) opens an OS-native dialog; tests
/// inject a fake so the whole import flow after the pick is headless. The seam
/// returns only the chosen PATH: reading the bytes is a separate pure step so
/// byte-reading stays testable without `rfd`.
pub trait FilePicker {
    /// Return the chosen path, or `None` if the user cancelled.
    fn pick(&self) -> Option<PathBuf>;
}

/// The production [`FilePicker`]: an OS-native open dialog via `rfd`.
///
/// MANUAL carve-out (FR-K5 AC2): `rfd::FileDialog` opens an OS-native dialog
/// that a headless `accesskit`/`egui_kittest` harness cannot drive, exactly as
/// `app.rs`'s `prompt_open` and the FR-K4 external-pane scaffold note. Every
/// step AFTER the pick (read bytes, resolve, mutate, refresh) is headless and
/// is tested with a `FakePicker` instead.
#[derive(Debug, Default)]
pub struct RfdPicker;

impl FilePicker for RfdPicker {
    fn pick(&self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_file()
    }
}

// === The resolution model (pure, headless) ==============================

/// How an import of a candidate `name` resolves against the archive's current
/// file list.
///
/// Pure model state: a test drives it with no dialog and no archive. The
/// variant picks the downstream library call -- `Add` / `AddAs` create (or
/// append under a new name), `UpdatePrompt` once confirmed appends a version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportResolution {
    /// `name` is NOT tracked: add a new file, with no prompt (AC1).
    Add {
        /// The name to add the imported bytes under.
        name: String,
    },
    /// `name` IS tracked: default to appending a version, but SURFACE a prompt
    /// so the user can confirm-update or choose add-under-a-different-name.
    UpdatePrompt {
        /// The existing tracked name the import collides with.
        name: String,
    },
    /// The user chose to add the (existing-name) import under a new name.
    AddAs {
        /// The original candidate name that collided.
        original: String,
        /// The new, distinct name to add the bytes under.
        new_name: String,
    },
}

impl ImportResolution {
    /// Resolve a candidate `name` against the archive's current `existing`
    /// names: [`ImportResolution::Add`] when untracked, otherwise
    /// [`ImportResolution::UpdatePrompt`].
    pub fn resolve(name: &str, existing: &[&str]) -> ImportResolution {
        if existing.contains(&name) {
            ImportResolution::UpdatePrompt {
                name: name.to_string(),
            }
        } else {
            ImportResolution::Add {
                name: name.to_string(),
            }
        }
    }

    /// Whether this resolution is a surfaced prompt awaiting the user's choice.
    pub fn is_prompt(&self) -> bool {
        matches!(self, ImportResolution::UpdatePrompt { .. })
    }

    /// The name the imported bytes will ultimately be stored under.
    pub fn target_name(&self) -> &str {
        match self {
            ImportResolution::Add { name } => name,
            ImportResolution::UpdatePrompt { name } => name,
            ImportResolution::AddAs { new_name, .. } => new_name,
        }
    }

    /// Confirm an [`ImportResolution::UpdatePrompt`]: keep the update intent
    /// (append a new version to the existing name). A non-prompt resolution is
    /// returned unchanged, so calling this is always safe.
    pub fn confirm_update(self) -> ImportResolution {
        self
    }

    /// Turn an [`ImportResolution::UpdatePrompt`] into an
    /// [`ImportResolution::AddAs`] under `new_name` (the "add under a different
    /// name" alternative). A non-prompt resolution is returned unchanged.
    pub fn add_as(self, new_name: &str) -> ImportResolution {
        match self {
            ImportResolution::UpdatePrompt { name } => ImportResolution::AddAs {
                original: name,
                new_name: new_name.to_string(),
            },
            other => other,
        }
    }
}

/// The prompt text surfaced when an existing name collides (FR-K5 wording).
///
/// Uses ASCII `--` to match the requirement and the strict-ASCII source rule.
pub fn update_prompt_message(name: &str) -> String {
    format!("{name} exists -- import as a new version?")
}

// === The pending-import model the pane renders ==========================

/// A pending import: the bytes read off disk and the current resolution.
///
/// The navigator pane sets this when the user picks a file; it renders the
/// prompt while the resolution [`is_prompt`](ImportResolution::is_prompt), and
/// the app shell drains it into an [`ImportOutcome`] once resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingImport {
    /// The source path the bytes were read from (for status messages).
    pub path: PathBuf,
    /// The bytes read off disk, stored verbatim.
    pub bytes: Vec<u8>,
    /// How the import currently resolves.
    pub resolution: ImportResolution,
    /// Whether the user still has an open decision to make.
    ///
    /// `true` only while an [`ImportResolution::UpdatePrompt`] is surfaced and
    /// unanswered. A fresh `Add` is never awaiting; confirming the update or
    /// choosing add-as clears it, which is what makes the import drainable.
    awaiting_decision: bool,
}

/// The import model threaded through the navigator and acted on by the app.
///
/// Holds at most one pending import. `None` means no import is in flight (the
/// steady state and the post-mutate cleared state).
#[derive(Debug, Default)]
pub struct ImportState {
    pending: Option<PendingImport>,
}

impl ImportState {
    /// Begin an import of `bytes` picked from `path`, resolved against the
    /// archive's current `existing` names. A new name resolves to `Add` (no
    /// prompt); an existing name resolves to `UpdatePrompt` (a prompt renders).
    pub fn begin(&mut self, path: PathBuf, bytes: Vec<u8>, existing: &[&str]) {
        let name = candidate_name(&path);
        let resolution = ImportResolution::resolve(&name, existing);
        let awaiting_decision = resolution.is_prompt();
        self.pending = Some(PendingImport {
            path,
            bytes,
            resolution,
            awaiting_decision,
        });
    }

    /// The pending import, if any.
    pub fn pending(&self) -> Option<&PendingImport> {
        self.pending.as_ref()
    }

    /// Whether a prompt is currently surfaced AND unanswered (an existing-name
    /// collision the user has not yet confirmed or redirected to add-as).
    pub fn has_prompt(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| p.awaiting_decision)
    }

    /// Whether the pending import is resolved and ready for the app to apply
    /// (either a fresh `Add` or a prompt the user has answered).
    pub fn is_ready(&self) -> bool {
        self.pending.as_ref().is_some_and(|p| !p.awaiting_decision)
    }

    /// Confirm a surfaced update prompt (append a new version), clearing the
    /// open decision so the import becomes ready. No-op when nothing is pending.
    pub fn confirm_update(&mut self) {
        if let Some(pending) = self.pending.as_mut() {
            pending.resolution = pending.resolution.clone().confirm_update();
            pending.awaiting_decision = false;
        }
    }

    /// Resolve a surfaced update prompt into an add-under-a-different-name,
    /// clearing the open decision. No-op when nothing is pending.
    pub fn add_as(&mut self, new_name: &str) {
        if let Some(pending) = self.pending.as_mut() {
            pending.resolution = pending.resolution.clone().add_as(new_name);
            pending.awaiting_decision = false;
        }
    }

    /// Drain the pending import into an [`ImportOutcome`] the app acts on, and
    /// clear the pending slot. Returns `None` when nothing is pending.
    pub fn take_outcome(&mut self) -> Option<ImportOutcome> {
        self.pending.take().map(|p| ImportOutcome {
            path: p.path,
            bytes: p.bytes,
            resolution: p.resolution,
        })
    }

    /// Clear any pending import (the user cancelled the prompt).
    pub fn clear(&mut self) {
        self.pending = None;
    }
}

/// A resolved import handed to the app shell to apply through the source.
///
/// Carries plain walker-side values only (no `walrus_fmt` type), so the app and
/// navigator stay on the format-agnostic side of the FR-K1 boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOutcome {
    /// The source path the bytes came from.
    pub path: PathBuf,
    /// The bytes to import.
    pub bytes: Vec<u8>,
    /// How the import resolved (add / confirmed-update / add-as).
    pub resolution: ImportResolution,
}

/// The candidate tracked-name for a picked path: its file name, or the whole
/// path's string form if it somehow has no final component.
pub fn candidate_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

// === Tests ==========================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving_a_new_name_yields_add_with_no_prompt() {
        let resolution = ImportResolution::resolve("new.txt", &["a.txt"]);
        assert_eq!(
            resolution,
            ImportResolution::Add {
                name: "new.txt".to_string()
            }
        );
        assert!(!resolution.is_prompt(), "a new name surfaces no prompt");
    }

    #[test]
    fn resolving_an_existing_name_yields_update_prompt() {
        let resolution = ImportResolution::resolve("a.txt", &["a.txt", "b.txt"]);
        assert_eq!(
            resolution,
            ImportResolution::UpdatePrompt {
                name: "a.txt".to_string()
            }
        );
        assert!(resolution.is_prompt(), "an existing name surfaces a prompt");
    }

    #[test]
    fn confirming_an_update_prompt_keeps_the_update_intent() {
        let resolution = ImportResolution::UpdatePrompt {
            name: "a.txt".to_string(),
        }
        .confirm_update();
        assert_eq!(
            resolution,
            ImportResolution::UpdatePrompt {
                name: "a.txt".to_string()
            },
            "confirming keeps the append-a-version intent, not an Add"
        );
    }

    #[test]
    fn choosing_add_as_turns_an_update_prompt_into_add_as() {
        let resolution = ImportResolution::UpdatePrompt {
            name: "a.txt".to_string(),
        }
        .add_as("a-copy.txt");
        assert_eq!(
            resolution,
            ImportResolution::AddAs {
                original: "a.txt".to_string(),
                new_name: "a-copy.txt".to_string(),
            }
        );
    }

    #[test]
    fn the_update_prompt_message_names_the_existing_file() {
        let message = update_prompt_message("a.txt");
        assert_eq!(message, "a.txt exists -- import as a new version?");
    }

    #[test]
    fn target_name_follows_the_resolution_choice() {
        assert_eq!(
            ImportResolution::Add {
                name: "n.txt".to_string()
            }
            .target_name(),
            "n.txt"
        );
        assert_eq!(
            ImportResolution::AddAs {
                original: "a.txt".to_string(),
                new_name: "a-copy.txt".to_string(),
            }
            .target_name(),
            "a-copy.txt"
        );
    }

    #[test]
    fn beginning_an_import_of_a_new_name_sets_a_pending_add_without_a_prompt() {
        let mut state = ImportState::default();
        state.begin(PathBuf::from("/tmp/new.txt"), b"hello".to_vec(), &["a.txt"]);
        let pending = state.pending().expect("a pending import");
        assert_eq!(
            pending.resolution,
            ImportResolution::Add {
                name: "new.txt".to_string()
            }
        );
        assert!(!state.has_prompt(), "a new name does not surface a prompt");
    }

    #[test]
    fn beginning_an_import_of_an_existing_name_surfaces_the_prompt() {
        let mut state = ImportState::default();
        state.begin(PathBuf::from("/tmp/a.txt"), b"v2".to_vec(), &["a.txt"]);
        assert!(state.has_prompt(), "an existing name surfaces the prompt");
        assert_eq!(
            state.pending().expect("pending").resolution,
            ImportResolution::UpdatePrompt {
                name: "a.txt".to_string()
            }
        );
    }

    #[test]
    fn confirming_the_prompt_clears_it_from_the_has_prompt_view() {
        // Confirming keeps the UpdatePrompt resolution (append intent) but the
        // app then drains it via take_outcome; here we assert the confirm step
        // leaves a drainable outcome carrying the update intent.
        let mut state = ImportState::default();
        state.begin(PathBuf::from("/tmp/a.txt"), b"v2".to_vec(), &["a.txt"]);
        state.confirm_update();
        let outcome = state.take_outcome().expect("a drained outcome");
        assert_eq!(
            outcome.resolution,
            ImportResolution::UpdatePrompt {
                name: "a.txt".to_string()
            }
        );
        assert!(
            state.pending().is_none(),
            "taking the outcome clears pending"
        );
    }

    #[test]
    fn add_as_on_a_pending_prompt_switches_it_to_add_as() {
        let mut state = ImportState::default();
        state.begin(PathBuf::from("/tmp/a.txt"), b"v2".to_vec(), &["a.txt"]);
        state.add_as("a-copy.txt");
        assert!(!state.has_prompt(), "add-as is no longer a prompt");
        assert_eq!(
            state.take_outcome().expect("outcome").resolution,
            ImportResolution::AddAs {
                original: "a.txt".to_string(),
                new_name: "a-copy.txt".to_string(),
            }
        );
    }

    #[test]
    fn clearing_a_pending_import_cancels_it() {
        let mut state = ImportState::default();
        state.begin(PathBuf::from("/tmp/a.txt"), b"v2".to_vec(), &["a.txt"]);
        state.clear();
        assert!(state.pending().is_none());
        assert!(!state.has_prompt());
    }

    #[test]
    fn candidate_name_is_the_final_path_component() {
        assert_eq!(candidate_name(std::path::Path::new("/a/b/c.txt")), "c.txt");
    }
}
