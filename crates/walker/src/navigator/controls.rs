//! The pure control model for the FR-K6 per-file and batch controls.
//!
//! This module holds the egui-free, `walrus_fmt`-free model the navigator
//! control panes drive: the [`FileAction`] intents the panes emit for the app
//! to apply, and the [`PendingBatchRemove`] confirmation state that gates a
//! batch remove of two or more files (the GUI analogue of the CLI `--force`
//! multi-match guard). Keeping this logic pure means it is unit-tested directly
//! and the FR-K1 AC2 boundary (no archive types outside `source/walrus.rs`)
//! holds -- the panes pass only plain `&str` / `usize` / `Vec<usize>` values.

// === FileAction =========================================================

/// A per-file or batch control intent the navigator panes emit for the app to
/// apply through the concrete `WalrusSource` (FR-K6).
///
/// Each variant carries only plain values (file ids, a generation cap, a keep
/// count), never a `walrus_fmt` type, so the navigator stays on the
/// format-agnostic side of the FR-K1 AC2 boundary. The app resolves an id to a
/// `FileRef` and calls the matching `WalrusSource` mutate method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileAction {
    /// Set (or clear) the generation retention cap for one file. `n = None`
    /// means unlimited (0 in the 0.1 format).
    SetGenerations {
        /// The 0-based manifest id of the target file.
        id: usize,
        /// The new cap, or `None` for unlimited.
        n: Option<u32>,
    },
    /// Condense one file to `keep` versions beyond the current, discarding
    /// everything older.
    Condense {
        /// The 0-based manifest id of the target file.
        id: usize,
        /// The oldest surviving version index to retain.
        keep: usize,
    },
    /// Remove one file (a single-file remove needs no confirmation).
    Remove {
        /// The 0-based manifest id of the target file.
        id: usize,
    },
    /// Export one file's current version (reuses the existing export path).
    ExportFile {
        /// The 0-based manifest id of the target file.
        id: usize,
    },
    /// Condense several selected files to the same `keep` count.
    BatchCondense {
        /// The 0-based manifest ids of the selected files.
        ids: Vec<usize>,
        /// The oldest surviving version index to retain for each file.
        keep: usize,
    },
    /// Export several selected files' current versions.
    BatchExport {
        /// The 0-based manifest ids of the selected files.
        ids: Vec<usize>,
    },
    /// Remove several selected files. ONLY emitted after an explicit
    /// confirmation (AC2).
    BatchRemove {
        /// The 0-based manifest ids of the selected files.
        ids: Vec<usize>,
    },
}

// === PendingBatchRemove =================================================

/// The AC2 confirmation gate for a batch remove of two or more files.
///
/// A batch remove does NOT act immediately: the bar calls [`request`] with the
/// selected ids, which parks them here as pending. The confirmation row then
/// renders; [`confirm`] returns the parked ids (and clears), [`cancel`] clears
/// without returning. A single-file remove never routes through this gate.
///
/// [`request`]: PendingBatchRemove::request
/// [`confirm`]: PendingBatchRemove::confirm
/// [`cancel`]: PendingBatchRemove::cancel
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PendingBatchRemove {
    ids: Option<Vec<usize>>,
}

impl PendingBatchRemove {
    /// Park `ids` as awaiting an explicit confirmation. Replaces any previously
    /// parked set.
    pub fn request(&mut self, ids: Vec<usize>) {
        self.ids = Some(ids);
    }

    /// Whether a batch remove is parked awaiting confirmation.
    pub fn is_pending(&self) -> bool {
        self.ids.is_some()
    }

    /// The parked ids awaiting confirmation, or an empty slice when none.
    pub fn ids(&self) -> &[usize] {
        self.ids.as_deref().unwrap_or(&[])
    }

    /// Confirm the parked remove: return the parked ids and clear the gate.
    /// Returns an empty `Vec` when nothing was pending.
    pub fn confirm(&mut self) -> Vec<usize> {
        self.ids.take().unwrap_or_default()
    }

    /// Cancel the parked remove without returning its ids.
    pub fn cancel(&mut self) {
        self.ids = None;
    }
}

// === gen_cap parsing ====================================================

/// Parse a generation-cap input field into an [`Option<u32>`] cap.
///
/// A blank field, the word `unlimited` (any case), or `0` map to `None`
/// (unlimited retention, which is 0 in the 0.1 format); a positive integer maps
/// to `Some(n)`. Any other (non-numeric) input also maps to `None` so a typo
/// never silently sets a surprising cap.
pub fn gen_cap_from_input(input: &str) -> Option<u32> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unlimited") {
        return None;
    }
    match trimmed.parse::<u32>() {
        Ok(0) => None,
        Ok(n) => Some(n),
        Err(_) => None,
    }
}

// === Tests ==============================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_remove_request_is_pending_until_confirmed() {
        let mut pending = PendingBatchRemove::default();
        assert!(!pending.is_pending(), "a fresh gate has nothing pending");
        pending.request(vec![1, 3]);
        assert!(pending.is_pending(), "a request parks the ids as pending");
        assert_eq!(pending.ids(), &[1, 3]);
    }

    #[test]
    fn confirm_returns_the_exact_ids_and_clears_the_gate() {
        let mut pending = PendingBatchRemove::default();
        pending.request(vec![2, 5, 7]);
        let confirmed = pending.confirm();
        assert_eq!(confirmed, vec![2, 5, 7], "confirm returns the parked ids");
        assert!(!pending.is_pending(), "confirm clears the gate");
    }

    #[test]
    fn cancel_clears_the_gate_without_returning_ids() {
        let mut pending = PendingBatchRemove::default();
        pending.request(vec![4, 6]);
        pending.cancel();
        assert!(!pending.is_pending(), "cancel clears the gate");
        assert!(pending.ids().is_empty(), "no ids remain after cancel");
    }

    #[test]
    fn confirm_on_an_empty_gate_returns_an_empty_vec() {
        let mut pending = PendingBatchRemove::default();
        assert!(pending.confirm().is_empty(), "nothing pending yields none");
    }

    #[test]
    fn gen_cap_from_input_maps_blank_and_zero_and_unlimited_to_none() {
        assert_eq!(gen_cap_from_input(""), None);
        assert_eq!(gen_cap_from_input("   "), None);
        assert_eq!(gen_cap_from_input("0"), None);
        assert_eq!(gen_cap_from_input("unlimited"), None);
        assert_eq!(gen_cap_from_input("Unlimited"), None);
    }

    #[test]
    fn gen_cap_from_input_maps_a_positive_integer_to_some() {
        assert_eq!(gen_cap_from_input("5"), Some(5));
        assert_eq!(gen_cap_from_input(" 12 "), Some(12));
    }

    #[test]
    fn gen_cap_from_input_maps_non_numeric_input_to_none() {
        assert_eq!(gen_cap_from_input("abc"), None);
        assert_eq!(gen_cap_from_input("-1"), None);
    }
}
