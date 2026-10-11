//! The FR-K9 `history()` mapping for [`WalrusSource`].
//!
//! Split out of `walrus.rs` to keep that file under the 400 non-test-line
//! standard (NFR-1). It is a CHILD of `walrus` (via `#[path]`), so it stays
//! inside the one `walrus_fmt`-aware adapter and may name `walrus_fmt` types and
//! touch `WalrusSource` privates without crossing the FR-K1 AC2 boundary.

use walrus_fmt::Op;

// `walrus_history` is a `#[path]` CHILD of `walrus`, so `super` is the `walrus`
// module (the helpers and `WalrusSource` live there) and `super::super` is the
// `source` module (where the seam types live).
use super::super::{FileRef, HistoryEntry, SpliceSummary};
use super::{nth_splice, parse_iso_epoch, WalrusSource};

impl WalrusSource {
    /// The change-log history of `f` in version-index order (FR-K9).
    ///
    /// Each splice record becomes one [`HistoryEntry`]: the version index it
    /// produced (0 = current, higher = older), the record's timestamp (parsed
    /// from `ts`, `None` when absent or malformed) and note (`None` when
    /// absent), and the [`SpliceSummary`] read straight off `Op::Splice`
    /// (`inserted_len` is the UTF-8 byte length of the inserted string).
    ///
    /// Entries are listed index 0 FIRST then increasing (current -> older),
    /// matching `versions`. Checkpoint records are SKIPPED (`nth_splice` filters
    /// to splices). The far index `count` is the ORIGINAL base, produced by no
    /// record, so it is NOT an entry: indices span `0..count`. A stale or
    /// foreign `f` yields an EMPTY `Vec`.
    pub(super) fn history_entries(&self, f: &FileRef) -> Vec<HistoryEntry> {
        let Some(id) = self.file_id_for(f) else {
            return Vec::new();
        };
        let Some(count) = self.splice_count_for(f) else {
            return Vec::new();
        };
        let Some(records) = self.records_for(&id) else {
            return Vec::new();
        };
        // Version `index` was produced by splice ordinal `count - index`
        // (FORMAT.md 4.1: the i-th splice, 1-based oldest-first, yields the
        // version with canonical index `count - i`). Iterate current -> older.
        (0..count)
            .filter_map(|index| {
                let ordinal = count - index;
                let record = nth_splice(records, ordinal)?;
                let Op::Splice {
                    at,
                    delete_len,
                    insert,
                } = &record.op
                else {
                    // nth_splice already filters to splices; a non-splice here
                    // would be a logic error, so skip defensively.
                    return None;
                };
                Some(HistoryEntry {
                    index,
                    timestamp: record.ts.as_deref().and_then(parse_iso_epoch),
                    note: record.note.clone(),
                    splice: SpliceSummary {
                        at: *at,
                        deleted_len: *delete_len,
                        inserted_len: insert.len(),
                    },
                })
            })
            .collect()
    }
}
