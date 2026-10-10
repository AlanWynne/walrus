//! The pure, deterministic re-base / trim core (FR-W5, FORMAT.md sections 3,
//! 4.3, 5).
//!
//! A re-base drops the OLDEST version of a tracked file by folding one change
//! record into the base, leaving every surviving index byte-identical to before
//! the trim (INV-3). It is the single primitive both the `generations`
//! rolling-window cap and the one-shot `condense` share.
//!
//! Everything here is pure: no clock, no I/O. The `last` audit timestamp is
//! taken as a parameter (`Option<String>`); the pure core passes `None` so
//! tests stay deterministic. Reconstruction and splice application are REUSED
//! from [`crate::reconstruct`] (INV-4) rather than re-implemented.
//
// This module is the PURE core (FEAT-002). Its `pub(crate)` entry points are
// consumed by the Archive-level `condense` / `generations` wiring that lands in
// FEAT-003 (`mutate.rs`); until then the only non-test caller is absent, so the
// surface is allowed to read as dead code without failing `-D warnings`.
#![allow(dead_code)]

use crate::record::ChangeRecord;
use crate::wchg::Direction;
use crate::{reconstruct, Result};

// === fold_oldest ========================================================

/// Fold the OLDEST version of a file into its base, dropping exactly one
/// change record. This is the single re-base primitive; `count` decreases by
/// one and every surviving index stays byte-identical (INV-3).
///
/// - FORWARD file: the base is the original (index `count`); the oldest change
///   is the FIRST splice record. Apply it to the base to produce the new base
///   (the former index `count - 1`), then drop that record.
/// - REVERSE file: the base is the current (index 0) and stays put; the oldest
///   version is the far end, reached by the LAST splice record. Drop that
///   record; the base is unchanged.
///
/// A file with no splice records has no oldest version to drop: this is a
/// benign NO-OP success returning the inputs unchanged (D7). The splice apply
/// is REUSED from [`crate::reconstruct`] (INV-4).
///
/// # Errors
/// Returns [`crate::WalrusError::SpliceOutOfBounds`] if the folded forward
/// record is inconsistent with the base it applies to.
pub(crate) fn fold_oldest(
    base: &[u8],
    records: &[ChangeRecord],
    direction: Direction,
) -> Result<(Vec<u8>, Vec<ChangeRecord>)> {
    match direction {
        Direction::Forward => fold_forward(base, records),
        Direction::Reverse => fold_reverse(base, records),
    }
}

/// Forward fold: apply the first splice record to the base, drop it.
fn fold_forward(base: &[u8], records: &[ChangeRecord]) -> Result<(Vec<u8>, Vec<ChangeRecord>)> {
    match first_splice_index(records) {
        None => Ok((base.to_vec(), records.to_vec())),
        Some(pos) => {
            let mut new_base = base.to_vec();
            reconstruct::apply_record(&mut new_base, &records[pos])?;
            let mut new_records = records.to_vec();
            new_records.remove(pos);
            Ok((new_base, new_records))
        }
    }
}

/// Reverse fold: drop the last splice record, leave the base unchanged.
fn fold_reverse(base: &[u8], records: &[ChangeRecord]) -> Result<(Vec<u8>, Vec<ChangeRecord>)> {
    match last_splice_index(records) {
        None => Ok((base.to_vec(), records.to_vec())),
        Some(pos) => {
            let mut new_records = records.to_vec();
            new_records.remove(pos);
            Ok((base.to_vec(), new_records))
        }
    }
}

// === trim_to ============================================================

/// Trim a file down to `keep` retained VERSIONS by re-basing the oldest version
/// repeatedly until the splice `count` equals `keep`. This is the shared engine
/// both `generations` and `condense` call.
///
/// Returns the new base, the trimmed records, and the number of versions
/// dropped (`u32`). `keep >= count` is a benign NO-OP success returning the
/// inputs unchanged with `0` dropped (D7). Every surviving index `0..=keep`
/// reconstructs byte-identically to before the trim (INV-3).
///
/// # Errors
/// Returns [`crate::WalrusError::SpliceOutOfBounds`] if a folded forward record
/// is inconsistent with its base. A `keep` greater than the available `count`
/// is NOT an error here (it is the no-op case); the Archive-level callers raise
/// [`crate::WalrusError::Rebase`] for a genuinely impossible request.
pub(crate) fn trim_to(
    base: &[u8],
    records: &[ChangeRecord],
    direction: Direction,
    keep: usize,
) -> Result<(Vec<u8>, Vec<ChangeRecord>, u32)> {
    let mut cur_base = base.to_vec();
    let mut cur_records = records.to_vec();
    let mut dropped: u32 = 0;

    while splice_count(&cur_records) > keep {
        let before = splice_count(&cur_records);
        let (next_base, next_records) = fold_oldest(&cur_base, &cur_records, direction)?;
        // A no-op fold (no splice records left) cannot make progress; stop to
        // avoid looping. In practice the while-guard already prevents this.
        if splice_count(&next_records) == before {
            break;
        }
        cur_base = next_base;
        cur_records = next_records;
        dropped += 1;
    }

    Ok((cur_base, cur_records, dropped))
}

/// Trim a file down to `versions` RETAINED versions (indices `[0, versions-1]`).
/// This is the version-count-facing entry both `generations = N` and
/// `condense --keep N` map onto: `versions` retained means `keep = versions - 1`
/// splice records survive.
///
/// Retaining ZERO versions is impossible -- the current version (index 0)
/// always exists -- so it is a typed error. Retaining more versions than the
/// file has is a benign NO-OP success (D7).
///
/// # Errors
/// Returns [`crate::WalrusError::Rebase`] when `versions == 0`, and propagates
/// [`crate::WalrusError::SpliceOutOfBounds`] from the fold.
pub(crate) fn trim_to_versions(
    base: &[u8],
    records: &[ChangeRecord],
    direction: Direction,
    versions: usize,
) -> Result<(Vec<u8>, Vec<ChangeRecord>, u32)> {
    if versions == 0 {
        return Err(crate::WalrusError::Rebase {
            reason: "cannot retain zero versions: the current version always exists".to_string(),
        });
    }
    trim_to(base, records, direction, versions - 1)
}

// === checkpoint pruning =================================================

/// Prune a file's checkpoint list after a trim to `new_count` (D5). A trim
/// removes versions only from the oldest/far end and index 0 stays current, so
/// the `[0, new_count]` indices are stable: DROP any checkpoint whose index now
/// exceeds `new_count` (it named a version that no longer exists) and RETAIN the
/// rest UNCHANGED (no re-indexing -- a surviving checkpoint still names the same
/// version). Order is preserved.
pub(crate) fn prune_checkpoints(
    checkpoints: &[crate::Checkpoint],
    new_count: usize,
) -> Vec<crate::Checkpoint> {
    checkpoints
        .iter()
        .filter(|c| c.index <= new_count)
        .cloned()
        .collect()
}

// === audit metadata (RebaseInfo) ========================================

/// Return the per-file [`RebaseInfo`] after one re-base OPERATION that dropped
/// `dropped` versions. A single `condense`/`generations` trim is ONE operation
/// regardless of how many versions it folds away: `count += 1`, `dropped +=
/// dropped` (D4). `last` is taken as a parameter and the pure core passes
/// `None`, so this helper never reads the clock.
pub(crate) fn updated_rebase_info(
    existing: Option<crate::RebaseInfo>,
    dropped: u32,
    last: Option<String>,
) -> crate::RebaseInfo {
    let mut info = existing.unwrap_or_default();
    info.count += 1;
    info.dropped += dropped;
    info.last = last;
    info
}

/// Count the SPLICE records (checkpoints do not advance the version count).
fn splice_count(records: &[ChangeRecord]) -> usize {
    records.iter().filter(|r| r.is_splice()).count()
}

/// Index of the first SPLICE record (the oldest change for a forward file).
fn first_splice_index(records: &[ChangeRecord]) -> Option<usize> {
    records.iter().position(ChangeRecord::is_splice)
}

/// Index of the last SPLICE record (the farthest change for a reverse file).
fn last_splice_index(records: &[ChangeRecord]) -> Option<usize> {
    records.iter().rposition(ChangeRecord::is_splice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::ChangeRecord;
    use crate::wchg::Direction;

    // === fixtures =======================================================

    /// A forward history: original base "cat"; forward records append toward the
    /// current "the cats". count = 2 (index 0 = current, index 2 = original).
    fn forward_history() -> (Vec<u8>, Vec<ChangeRecord>) {
        (
            b"cat".to_vec(),
            vec![
                ChangeRecord::splice(3, 0, "s"),    // original -> "cats"
                ChangeRecord::splice(0, 0, "the "), // "cats" -> "the cats"
            ],
        )
    }

    /// The equivalent reverse history for the same document: base = current
    /// "the cats"; reverse records undo toward the original "cat".
    fn reverse_history() -> (Vec<u8>, Vec<ChangeRecord>) {
        (
            b"the cats".to_vec(),
            vec![
                ChangeRecord::splice(0, 4, ""), // "the cats" -> "cats"
                ChangeRecord::splice(3, 1, ""), // "cats" -> "cat"
            ],
        )
    }

    fn splice_count(records: &[ChangeRecord]) -> usize {
        records.iter().filter(|r| r.is_splice()).count()
    }

    // === fold_oldest ====================================================

    #[test]
    fn fold_oldest_on_a_forward_file_advances_the_base_and_drops_one_record() {
        // Forward: fold the first/oldest record into the base. The new base is
        // the former index count-1 ("cats"), and the log loses that record.
        let (base, records) = forward_history();
        let (new_base, new_records) =
            fold_oldest(&base, &records, Direction::Forward).expect("forward fold");
        assert_eq!(new_base, b"cats", "new base is the former index count-1");
        assert_eq!(new_records.len(), 1, "exactly one record dropped");
        assert_eq!(new_records[0], records[1], "the surviving record is kept");
    }

    #[test]
    fn fold_oldest_on_a_reverse_file_drops_the_farthest_record_and_keeps_the_base() {
        // Reverse: the base (current) is unchanged; the farthest/last reverse
        // record (the one reaching the oldest version) is dropped.
        let (base, records) = reverse_history();
        let (new_base, new_records) =
            fold_oldest(&base, &records, Direction::Reverse).expect("reverse fold");
        assert_eq!(new_base, base, "reverse base (current) is unchanged");
        assert_eq!(new_records.len(), 1, "exactly one record dropped");
        assert_eq!(new_records[0], records[0], "the farthest record is dropped");
    }

    #[test]
    fn fold_oldest_decreases_count_by_exactly_one() {
        for direction in [Direction::Forward, Direction::Reverse] {
            let (base, records) = match direction {
                Direction::Forward => forward_history(),
                Direction::Reverse => reverse_history(),
            };
            let before = splice_count(&records);
            let (_b, new_records) =
                fold_oldest(&base, &records, direction).expect("fold decreases count");
            assert_eq!(
                splice_count(&new_records),
                before - 1,
                "fold drops exactly one version for {direction:?}"
            );
        }
    }

    #[test]
    fn fold_oldest_on_a_zero_record_file_is_a_noop_or_typed_error() {
        // D7: a zero-record file has no oldest version to drop. Folding it is a
        // no-op SUCCESS returning the inputs unchanged (the benign case).
        let base = b"only current".to_vec();
        for direction in [Direction::Forward, Direction::Reverse] {
            let (new_base, new_records) =
                fold_oldest(&base, &[], direction).expect("zero-record fold is a no-op success");
            assert_eq!(new_base, base);
            assert!(new_records.is_empty());
        }
    }

    // === byte-identical invariant across a trim (INV-3) =================

    /// A richer forward history: insertions and a deletion at several offsets.
    /// original "abcdef"; count = 3; current = "XYcdZZZfGH".
    fn rich_forward_history() -> (Vec<u8>, Vec<ChangeRecord>) {
        (
            b"abcdef".to_vec(),
            vec![
                ChangeRecord::splice(6, 0, "GH"),  // "abcdef" -> "abcdefGH"
                ChangeRecord::splice(0, 2, "XY"),  // -> "XYcdefGH"
                ChangeRecord::splice(4, 1, "ZZZ"), // -> "XYcdZZZfGH"
            ],
        )
    }

    /// Invert a forward history into its reverse-log equivalent (base = current,
    /// records apply current -> original), mirroring the archive.rs fixture.
    fn invert_forward_to_reverse(
        original: &[u8],
        forward: &[ChangeRecord],
    ) -> (Vec<u8>, Vec<ChangeRecord>) {
        let mut version = original.to_vec();
        let mut inverses = Vec::new();
        for rec in forward {
            if let crate::record::Op::Splice {
                at,
                delete_len,
                insert,
            } = &rec.op
            {
                let deleted = version[*at..*at + *delete_len].to_vec();
                let deleted_str = String::from_utf8(deleted).expect("utf-8 deleted bytes");
                inverses.push(ChangeRecord::splice(*at, insert.len(), deleted_str));
                let mut next = version.clone();
                next.splice(*at..*at + *delete_len, insert.bytes());
                version = next;
            }
        }
        inverses.reverse();
        (version, inverses)
    }

    /// Reconstruct every index `0..=count` of a (base, records, direction) via
    /// the shared directional engine. Returns the vector indexed by version.
    fn reconstruct_all(
        base: &[u8],
        records: &[ChangeRecord],
        direction: Direction,
    ) -> Vec<Vec<u8>> {
        let count = splice_count(records);
        (0..=count)
            .map(|k| {
                reconstruct::reconstruct_directional(base, records, direction, k, count)
                    .expect("reconstruct index")
            })
            .collect()
    }

    #[test]
    fn forward_rebase_leaves_every_surviving_index_byte_identical_to_before_the_trim() {
        let (base, records) = rich_forward_history();
        let count = splice_count(&records);
        let before = reconstruct_all(&base, &records, Direction::Forward);

        // Trim away the two oldest versions (keep indices 0..=1).
        let keep = 1usize;
        let (new_base, new_records, dropped) =
            trim_to(&base, &records, Direction::Forward, keep).expect("forward trim");
        assert_eq!(
            dropped,
            (count - keep) as u32,
            "dropped count is count-keep"
        );

        let after = reconstruct_all(&new_base, &new_records, Direction::Forward);
        assert_eq!(after.len(), keep + 1, "surviving range is 0..=keep");
        for k in 0..=keep {
            assert_eq!(after[k], before[k], "surviving index {k} must be identical");
        }
        assert_eq!(after[0], before[0], "index 0 stays the current version");
    }

    #[test]
    fn forward_generations_window_keeps_exactly_n_versions_and_index_zero_stays_current() {
        // generations = N keeps indices [0, N-1], i.e. trim to keep = N - 1.
        let (base, records) = rich_forward_history(); // count = 3
        let before = reconstruct_all(&base, &records, Direction::Forward);
        let n = 2usize; // keep 2 versions: indices 0 and 1
        let keep = n - 1;
        let (new_base, new_records, dropped) =
            trim_to(&base, &records, Direction::Forward, keep).expect("forward generations");
        assert_eq!(
            splice_count(&new_records),
            keep,
            "exactly N-1 records remain"
        );
        assert_eq!(dropped, 2, "two oldest versions dropped");
        let after = reconstruct_all(&new_base, &new_records, Direction::Forward);
        assert_eq!(after.len(), n, "exactly N versions remain");
        assert_eq!(after[0], before[0], "index 0 is still the current version");
        assert_eq!(after[1], before[1], "index 1 is byte-identical");
    }

    #[test]
    fn reverse_generations_window_keeps_exactly_n_versions_and_index_zero_stays_current() {
        let (original, forward) = rich_forward_history();
        let (base, records) = invert_forward_to_reverse(&original, &forward);
        let before = reconstruct_all(&base, &records, Direction::Reverse);
        let n = 2usize;
        let keep = n - 1;
        let (new_base, new_records, dropped) =
            trim_to(&base, &records, Direction::Reverse, keep).expect("reverse generations");
        assert_eq!(splice_count(&new_records), keep);
        assert_eq!(dropped, 2);
        assert_eq!(new_base, base, "reverse base (current) never moves");
        let after = reconstruct_all(&new_base, &new_records, Direction::Reverse);
        assert_eq!(after.len(), n);
        assert_eq!(after[0], before[0], "index 0 is still the current version");
        assert_eq!(after[1], before[1], "index 1 is byte-identical");
    }

    #[test]
    fn mixed_archive_rebase_leaves_each_files_surviving_indices_byte_identical() {
        // One forward file and its inverted reverse twin over the SAME history,
        // both trimmed to the same keep: each file's surviving indices are
        // byte-identical to before, and the two directions agree index-by-index.
        let (original, forward) = rich_forward_history();
        let (rev_base, rev_records) = invert_forward_to_reverse(&original, &forward);

        let fwd_before = reconstruct_all(&original, &forward, Direction::Forward);
        let rev_before = reconstruct_all(&rev_base, &rev_records, Direction::Reverse);

        let keep = 1usize;
        let (fwd_nb, fwd_nr, fwd_dropped) =
            trim_to(&original, &forward, Direction::Forward, keep).expect("forward trim");
        let (rev_nb, rev_nr, rev_dropped) =
            trim_to(&rev_base, &rev_records, Direction::Reverse, keep).expect("reverse trim");
        assert_eq!(fwd_dropped, rev_dropped, "both files drop the same count");

        let fwd_after = reconstruct_all(&fwd_nb, &fwd_nr, Direction::Forward);
        let rev_after = reconstruct_all(&rev_nb, &rev_nr, Direction::Reverse);
        for k in 0..=keep {
            assert_eq!(fwd_after[k], fwd_before[k], "forward index {k} identical");
            assert_eq!(rev_after[k], rev_before[k], "reverse index {k} identical");
            assert_eq!(
                fwd_after[k], rev_after[k],
                "forward and reverse agree at {k}"
            );
        }
    }

    #[test]
    fn reverse_rebase_leaves_every_surviving_index_byte_identical_to_before_the_trim() {
        let (original, forward) = rich_forward_history();
        let (base, records) = invert_forward_to_reverse(&original, &forward);
        let count = splice_count(&records);
        let before = reconstruct_all(&base, &records, Direction::Reverse);

        let keep = 1usize;
        let (new_base, new_records, dropped) =
            trim_to(&base, &records, Direction::Reverse, keep).expect("reverse trim");
        assert_eq!(dropped, (count - keep) as u32);
        assert_eq!(new_base, base, "reverse base (current) never moves");

        let after = reconstruct_all(&new_base, &new_records, Direction::Reverse);
        assert_eq!(after.len(), keep + 1);
        for k in 0..=keep {
            assert_eq!(after[k], before[k], "surviving index {k} must be identical");
        }
        assert_eq!(after[0], before[0], "index 0 stays the current version");
    }

    // === audit metadata (RebaseInfo) and error cases ====================

    #[test]
    fn a_rebase_increments_rebase_count_by_one_per_operation() {
        // One trim that drops K versions is ONE re-base operation: count += 1.
        let updated = updated_rebase_info(None, 3, None);
        assert_eq!(updated.count, 1, "one operation increments count by one");
    }

    #[test]
    fn a_rebase_adds_the_number_of_dropped_versions_to_rebase_dropped() {
        let first = updated_rebase_info(None, 3, None);
        assert_eq!(first.dropped, 3, "dropped accumulates the versions removed");
        // A second operation dropping 2 more: count 1 -> 2, dropped 3 -> 5.
        let second = updated_rebase_info(Some(first), 2, None);
        assert_eq!(second.count, 2);
        assert_eq!(second.dropped, 5);
    }

    #[test]
    fn the_pure_rebase_core_does_not_set_last_so_tests_stay_deterministic() {
        // The pure core passes `last = None`; the audit object leaves it unset.
        let updated = updated_rebase_info(None, 1, None);
        assert_eq!(updated.last, None, "pure core never reads the clock");
    }

    #[test]
    fn trimming_below_what_exists_is_a_typed_error() {
        // Retaining ZERO versions is impossible: index 0 (current) always
        // exists. Distinct from the benign `keep >= count` no-op (see below).
        let (base, records) = rich_forward_history();
        let err = trim_to_versions(&base, &records, Direction::Forward, 0)
            .expect_err("retaining zero versions must be a typed error");
        assert!(matches!(err, crate::WalrusError::Rebase { .. }));
    }

    #[test]
    fn trim_to_versions_keep_greater_than_or_equal_to_count_is_a_noop_success() {
        // The benign counterpart: asking to retain more versions than exist is
        // a no-op success, NOT the typed error above.
        let (base, records) = rich_forward_history(); // count = 3 -> 4 versions
        let (nb, nr, dropped) = trim_to_versions(&base, &records, Direction::Forward, 99)
            .expect("retaining more than exists is a no-op success");
        assert_eq!(dropped, 0);
        assert_eq!(nb, base);
        assert_eq!(nr, records);
    }

    // === checkpoint pruning (D5) ========================================

    fn cp(index: usize, label: &str) -> crate::Checkpoint {
        crate::Checkpoint {
            index,
            label: label.to_string(),
        }
    }

    #[test]
    fn a_trim_drops_checkpoints_whose_index_falls_off_the_retained_window() {
        // Count was 3 (indices 0..=3); after a trim to count' = 1 any checkpoint
        // naming index 2 or 3 is dropped.
        let checkpoints = vec![
            cp(0, "current"),
            cp(1, "keep"),
            cp(2, "gone"),
            cp(3, "gone2"),
        ];
        let pruned = prune_checkpoints(&checkpoints, 1);
        assert_eq!(pruned.len(), 2, "only in-window checkpoints survive");
        assert!(
            pruned.iter().all(|c| c.index <= 1),
            "no index exceeds count'"
        );
    }

    #[test]
    fn a_trim_keeps_surviving_checkpoints_with_their_index_and_label_unchanged() {
        // Surviving checkpoints are retained UNCHANGED: no re-indexing.
        let checkpoints = vec![cp(0, "current"), cp(2, "milestone"), cp(5, "far")];
        let pruned = prune_checkpoints(&checkpoints, 2);
        assert_eq!(pruned, vec![cp(0, "current"), cp(2, "milestone")]);
    }

    #[test]
    fn a_trimmed_manifest_still_passes_validate() {
        use crate::manifest::{Manifest, TrackedFile, Variant, Versions};

        // Build a forward file with checkpoints, trim it to a smaller count',
        // prune checkpoints, re-derive versions, and confirm validate() is Ok
        // and no checkpoint index exceeds count'.
        let (base, records) = rich_forward_history(); // count = 3
        let keep = 1usize;
        let (_nb, new_records, _dropped) =
            trim_to(&base, &records, Direction::Forward, keep).expect("trim");
        let new_count = new_records.iter().filter(|r| r.is_splice()).count();

        let mut tracked = TrackedFile::new_with_direction("f", "doc.txt", Direction::Forward, 3);
        tracked.checkpoints = vec![cp(0, "current"), cp(1, "keep"), cp(3, "gone")];

        tracked.checkpoints = prune_checkpoints(&tracked.checkpoints, new_count);
        tracked.versions = Versions::for_direction(Direction::Forward, new_count);

        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(tracked);
        manifest
            .validate()
            .expect("a trimmed manifest still validates");
        assert!(manifest.files[0]
            .checkpoints
            .iter()
            .all(|c| c.index <= new_count));
    }
}
