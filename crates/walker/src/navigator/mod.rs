//! The two-pane navigator (FR-K4).
//!
//! The LEFT side of the app is a navigator with two stacked panes:
//!   - the INTERNAL pane (`internal`) lists the archive's tracked files -- the
//!     GUI form of `walrus query -l` -- driven entirely through the
//!     format-agnostic [`HistorySource`] seam (`files()` + `file_info()`),
//!   - the EXTERNAL pane (`external`) is a filesystem-browser SCAFFOLD for the
//!     FR-K5 import flow (layout only; the `rfd` dialog is left to FR-K5).
//!
//! Selection binds the player/viewer to one file's version timeline (AC2) and a
//! multi-select set drives batch-action affordances (AC3). The selection model
//! ([`NavigatorState`]) is pure logic with no egui dependency, so it is unit
//! tested directly; the panes render it and return a [`NavigatorAction`] the app
//! acts on, mirroring the `chrome::ChromeAction` pattern so `app.rs` stays thin.
//!
//! Nothing here names a `walrus_fmt` type: the navigator is coded to the seam,
//! preserving the FR-K1 AC2 boundary.

use std::collections::BTreeSet;

use eframe::egui;

use crate::import::{FilePicker, ImportOutcome, ImportState};
use crate::source::{FileRef, HistorySource};

pub mod bloat;
mod external;
mod internal;

use self::bloat::BloatThresholds;

/// The navigator's selection model: the active file (AC2) and the multi-select
/// set (AC3).
///
/// `active` is the file whose timeline the player/viewer is bound to. `selected`
/// is the set of file ids chosen for a batch action; batch affordances enable
/// only once two or more are selected.
#[derive(Debug, Default)]
pub struct NavigatorState {
    /// The file whose timeline the player/viewer is bound to (AC2).
    active: Option<FileRef>,
    /// Multi-select set for batch affordances (AC3): ids into `source.files()`.
    selected: BTreeSet<usize>,
    /// The overridable FR-K7 bloat thresholds the internal pane reads instead
    /// of inline literals. Defaults to the FR-K7 starting values (2.0/5.0/50).
    bloat_thresholds: BloatThresholds,
}

impl NavigatorState {
    /// The file whose timeline is currently bound, if any.
    pub fn active(&self) -> Option<&FileRef> {
        self.active.as_ref()
    }

    /// Single-click select: make `f` active AND reset the multi-select set to
    /// just this file. This is the primary-click model (a plain click both
    /// binds the timeline and starts a fresh selection).
    pub fn select(&mut self, f: &FileRef) {
        self.active = Some(f.clone());
        self.selected.clear();
        self.selected.insert(f.id());
    }

    /// Toggle a file's membership in the multi-select set (the ctrl/shift-click
    /// model) without changing the active binding.
    pub fn toggle(&mut self, id: usize) {
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
    }

    /// Whether `id` is in the multi-select set.
    pub fn is_selected(&self, id: usize) -> bool {
        self.selected.contains(&id)
    }

    /// How many files are in the multi-select set.
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// AC3: batch actions are enabled only with two or more files selected.
    pub fn batch_enabled(&self) -> bool {
        self.selected_count() >= 2
    }

    /// The FR-K7 bloat thresholds the internal pane colours rows against.
    pub fn bloat_thresholds(&self) -> &BloatThresholds {
        &self.bloat_thresholds
    }
}

/// What the navigator asked the app to do this frame.
///
/// Mirrors `chrome::ChromeAction`: the panes mutate [`NavigatorState`] directly
/// for selection bookkeeping and report higher-level intent here so the app
/// shell can rebind the player/viewer without reaching into the panes.
#[derive(Debug, Default)]
pub struct NavigatorAction {
    /// Set to the newly activated file when a row was clicked this frame, so the
    /// app can rebind the player/viewer to that file's timeline (AC2).
    pub activated: Option<FileRef>,
    /// A resolved import (FR-K5) the app should apply through the source and
    /// then refresh the file list, when one became ready this frame.
    pub import: Option<ImportOutcome>,
    /// A human-readable error from reading the picked import file, if the pick
    /// failed before resolution.
    pub import_error: Option<String>,
    /// The file whose history the app should show this frame (FR-K9), set when
    /// a row's "History" button was clicked. Purely additive, mirrors
    /// `activated`.
    pub history_for: Option<FileRef>,
}

/// Lay out both navigator panes: the internal file list (top) and the external
/// import affordance (bottom). Returns the [`NavigatorAction`] for this frame.
///
/// Driven entirely through `source` (`files()` + `file_info()`), so the
/// navigator stays coded to the format-agnostic seam. `picker` is injected (not
/// constructed in the pane) so tests pass a fake and never touch the OS dialog;
/// `import` carries the FR-K5 pending-import model across frames.
pub fn panels(
    ui: &mut egui::Ui,
    source: &dyn HistorySource,
    picker: &dyn FilePicker,
    import: &mut ImportState,
    state: &mut NavigatorState,
) -> NavigatorAction {
    let mut action = internal::internal_pane(ui, source, state);
    ui.separator();
    external::external_pane(ui, source, picker, import, &mut action);
    action
}

// === Tests ==========================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: usize, name: &str) -> FileRef {
        FileRef::new(id, name)
    }

    #[test]
    fn selecting_a_file_sets_it_active_and_resets_the_multi_select_set() {
        let mut state = NavigatorState::default();
        state.toggle(7); // pre-existing stray selection
        state.select(&file(2, "data.csv"));
        assert_eq!(state.active().map(FileRef::id), Some(2));
        assert_eq!(state.selected_count(), 1);
        assert!(state.is_selected(2));
        assert!(!state.is_selected(7), "select resets the multi-select set");
    }

    #[test]
    fn toggling_two_files_enables_batch_actions() {
        let mut state = NavigatorState::default();
        state.toggle(0);
        state.toggle(1);
        assert_eq!(state.selected_count(), 2);
        assert!(state.batch_enabled());
    }

    #[test]
    fn a_single_selected_file_does_not_enable_batch_actions() {
        let mut state = NavigatorState::default();
        state.select(&file(0, "README.md"));
        assert_eq!(state.selected_count(), 1);
        assert!(!state.batch_enabled());
    }

    #[test]
    fn selecting_a_different_file_changes_the_active_ref() {
        let mut state = NavigatorState::default();
        state.select(&file(0, "README.md"));
        assert_eq!(state.active().map(FileRef::id), Some(0));
        state.select(&file(3, "data.json"));
        assert_eq!(state.active().map(FileRef::id), Some(3));
    }

    #[test]
    fn default_navigator_state_carries_the_default_bloat_thresholds() {
        let state = NavigatorState::default();
        let defaults = BloatThresholds::default();
        assert_eq!(state.bloat_thresholds().amber_at, defaults.amber_at);
        assert_eq!(state.bloat_thresholds().red_at, defaults.red_at);
        assert_eq!(
            state.bloat_thresholds().floor_index_count,
            defaults.floor_index_count
        );
    }

    #[test]
    fn toggling_the_same_file_twice_clears_it() {
        let mut state = NavigatorState::default();
        state.toggle(4);
        assert!(state.is_selected(4));
        state.toggle(4);
        assert!(!state.is_selected(4));
        assert_eq!(state.selected_count(), 0);
    }
}
