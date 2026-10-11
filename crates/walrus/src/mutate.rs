//! Archive-level re-base entry points (FR-W5, FORMAT.md sections 3, 4.3, 5).
//!
//! These wire the pure re-base/trim core ([`crate::rebase`]) to a real
//! [`Archive`]: [`Archive::condense`] (the one-shot manual trim) and
//! [`Archive::enforce_generations`] (the rolling-window cap). Each selects the
//! matched file(s), calls [`crate::rebase::trim_to`], then updates that file's
//! `RebaseInfo`, prunes its off-window checkpoints, re-derives its `versions`
//! and `.wchg` header `count`, and leaves NO marker record in the log (the
//! audit trail is only the manifest `rebase` object, FORMAT.md section 4.3).
//!
//! They live in this sibling module (not `archive.rs`) so `archive.rs` stays
//! under the 400 non-test-line cap (D6/NFR-1). Mutation reaches the private
//! per-file contents through the `pub(crate)` `files_mut`/`manifest_mut` seam.
//! Persisting a mutated archive is a separate, TRANSACTIONAL step via
//! [`Archive::write`], which routes through `atomic::atomic_write` so a failed
//! write leaves any on-disk archive byte-for-byte unchanged (FORMAT.md
//! section 6).

use crate::archive::Archive;
use crate::manifest::Versions;
use crate::wchg::Direction;
use crate::{rebase, Result, WalrusError};

impl Archive {
    /// Condense the file(s) matched by `mask` (an id or an original name) to
    /// `keep` retained VERSIONS beyond the current, discarding everything
    /// older. `keep` names the oldest surviving index: `keep = 0` collapses a
    /// file to current-only (an empty log); `keep >= count` is a no-op success.
    ///
    /// This is the manual, one-shot form of the `generations` cap and shares the
    /// same re-base trimming logic. It mutates the in-memory archive only;
    /// persist the result with the transactional [`Archive::write`].
    ///
    /// # Errors
    /// Returns [`WalrusError::Rebase`] when `mask` matches no tracked file, and
    /// propagates [`WalrusError::SpliceOutOfBounds`] from the fold.
    pub fn condense(&mut self, mask: &str, keep: usize) -> Result<()> {
        let ids = self.ids_matching(mask);
        if ids.is_empty() {
            return Err(WalrusError::Rebase {
                reason: format!("condense target '{mask}' matches no tracked file"),
            });
        }
        for id in ids {
            self.rebase_file(&id, keep)?;
        }
        Ok(())
    }

    /// Enforce the `generations` rolling-window cap for the file selected by
    /// `id`. When the file's `generations` N is `>= 1` and its retained count
    /// exceeds N, re-base the oldest version(s) until exactly N versions remain
    /// (valid index range `[0, N-1]`). `generations = 0` is unlimited (no trim).
    ///
    /// # Errors
    /// Returns [`WalrusError::UnknownFile`] when no tracked file has `id`, and
    /// propagates [`WalrusError::SpliceOutOfBounds`] from the fold.
    pub fn enforce_generations(&mut self, id: &str) -> Result<()> {
        let generations = self
            .manifest()
            .file(id)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?
            .generations;
        // generations = 0 means unlimited retention: nothing to enforce.
        if generations == 0 {
            return Ok(());
        }
        // generations = N keeps N versions (indices [0, N-1]), i.e. keep = N-1
        // surviving splice records. rebase_file's keep >= count is a no-op.
        self.rebase_file(id, (generations as usize).saturating_sub(1))
    }

    // === internal re-base engine ========================================

    /// The id of every tracked file whose id or original name equals `mask`.
    fn ids_matching(&self, mask: &str) -> Vec<String> {
        self.manifest()
            .files
            .iter()
            .filter(|f| f.id == mask || f.name == mask)
            .map(|f| f.id.clone())
            .collect()
    }

    /// Re-base one tracked file (by id) down to `keep` retained splice records,
    /// updating its in-memory base/records, `.wchg` header `count`, manifest
    /// `versions`, pruned checkpoints, and `rebase` audit metadata. Leaves NO
    /// marker record in the log (the audit trail is only the `rebase` object).
    /// A trim that drops nothing records no re-base and leaves the file as-is.
    fn rebase_file(&mut self, id: &str, keep: usize) -> Result<()> {
        let direction = self.file_direction(id)?;

        // Trim the per-file base/records in place via the pure core.
        let (new_base, new_records, dropped) = {
            let file = self.archive_file_mut(id)?;
            rebase::trim_to(&file.base, &file.records, direction, keep)?
        };
        if dropped == 0 {
            // A no-op trim: do not record a re-base operation (D7).
            return Ok(());
        }
        let new_count = new_records.iter().filter(|r| r.is_splice()).count();
        {
            let file = self.archive_file_mut(id)?;
            file.base = new_base;
            file.records = new_records;
            file.header.count = new_count;
        }

        // Update the manifest entry: versions, pruned checkpoints, audit.
        let tracked = self
            .manifest_mut()
            .files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?;
        tracked.versions = Versions::for_direction(direction, new_count);
        tracked.checkpoints = rebase::prune_checkpoints(&tracked.checkpoints, new_count);
        // The pure core sets `last = None`; a clock-reading caller may fill it.
        tracked.rebase = Some(rebase::updated_rebase_info(
            tracked.rebase.clone(),
            dropped,
            None,
        ));
        Ok(())
    }

    /// This file's own replay direction (from its manifest entry).
    fn file_direction(&self, id: &str) -> Result<Direction> {
        self.manifest()
            .file(id)
            .map(|f| f.direction)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })
    }

    /// Mutable access to one parsed per-file entry by id. Shared with the
    /// sibling `mutate_add` module's save path, so it is `pub(crate)`.
    pub(crate) fn archive_file_mut(
        &mut self,
        id: &str,
    ) -> Result<&mut crate::archive::ArchiveFile> {
        self.files_mut()
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })
    }
}

// These Archive-level tests live in-crate (not in `tests/`) for the same reason
// the FR-W3 reconstruct tests do: the fixture seam `Archive::from_parts_for_test`
// is `#[cfg(test)] pub(crate)`, so an external integration crate cannot build an
// archive without widening the production API purely for tests -- which the task
// forbids. They assert the cross-file byte-identical INV-3 property over the
// `condense`/`generations` surface, plus the transactional guarantee.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Checkpoint, Manifest, TrackedFile, Variant};
    use crate::record::ChangeRecord;
    use crate::wchg::{Direction, WchgHeader};
    use crate::{FileId, Version};

    // === fixtures =======================================================

    /// A richer forward history: original "abcdef"; count = 3; current
    /// "XYcdZZZfGH" (insertions and a deletion at several offsets).
    fn rich_forward() -> (Vec<u8>, Vec<ChangeRecord>) {
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
    /// records apply current -> original), via the single production inversion
    /// helper ([`crate::invert::invert_forward_splices`]) so there is ONE
    /// inversion implementation, test and production alike.
    fn invert_forward_to_reverse(
        original: &[u8],
        forward: &[ChangeRecord],
    ) -> (Vec<u8>, Vec<ChangeRecord>) {
        crate::invert::invert_forward_splices(original, forward).expect("invert forward splices")
    }

    fn splice_count(records: &[ChangeRecord]) -> usize {
        records.iter().filter(|r| r.is_splice()).count()
    }

    /// Build a single-file archive from a per-file base + records + direction,
    /// deriving the manifest entry (count = splice count) and `.wchg` header.
    fn single_file_archive(
        id: &str,
        direction: Direction,
        base: &[u8],
        records: Vec<ChangeRecord>,
    ) -> Archive {
        let count = splice_count(&records);
        let tracked = TrackedFile::new_with_direction(id, "doc.txt", direction, count);
        let header = WchgHeader::new(direction, 0, "doc.txt", "utf-8", count);
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(tracked);
        Archive::from_parts_for_test(
            manifest,
            vec![(id.to_string(), base.to_vec(), header, records)],
        )
    }

    /// Set a tracked file's `generations` cap via the crate-visible manifest
    /// seam (a test helper; production sets this when a file is added).
    fn set_generations(archive: &mut Archive, id: &str, generations: u32) {
        let file = archive
            .manifest_mut()
            .files
            .iter_mut()
            .find(|f| f.id == id)
            .expect("file");
        file.generations = generations;
    }

    /// Reconstruct every index `0..=count` of one tracked file in an archive.
    fn reconstruct_all(archive: &Archive, id: &str) -> Vec<Vec<u8>> {
        let count = archive.file(id).expect("file").versions.count;
        (0..=count)
            .map(|k| {
                archive
                    .reconstruct(&FileId::from(id), Version::Index(k))
                    .expect("reconstruct index")
            })
            .collect()
    }

    // === condense semantics =============================================

    #[test]
    fn condense_keep_n_retains_indices_zero_through_n_and_discards_older() {
        let (base, records) = rich_forward(); // count = 3
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        let before = reconstruct_all(&archive, "f");

        archive.condense("f", 1).expect("condense keep 1");

        let file = archive.file("f").expect("file");
        assert_eq!(file.versions.count, 1, "count' == keep");
        let after = reconstruct_all(&archive, "f");
        assert_eq!(after.len(), 2, "surviving range is 0..=1");
        assert_eq!(after[0], before[0], "index 0 unchanged");
        assert_eq!(after[1], before[1], "index 1 unchanged");
    }

    #[test]
    fn condense_keep_zero_collapses_the_file_to_current_only_with_an_empty_log() {
        let (base, records) = rich_forward();
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        let current_before = archive
            .reconstruct(&FileId::from("f"), Version::Index(0))
            .expect("current");

        archive.condense("f", 0).expect("condense keep 0");

        let file = archive.file("f").expect("file");
        assert_eq!(file.versions.count, 0, "collapsed to current only");
        assert!(
            archive.records("f").expect("records").is_empty(),
            "empty log"
        );
        let current_after = archive
            .reconstruct(&FileId::from("f"), Version::Index(0))
            .expect("current after");
        assert_eq!(current_after, current_before, "current is unchanged");
    }

    #[test]
    fn condense_keep_greater_than_or_equal_to_count_is_a_noop_success() {
        let (base, records) = rich_forward(); // count = 3
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        let before = reconstruct_all(&archive, "f");

        archive
            .condense("f", 99)
            .expect("keep >= count is a no-op success");

        let file = archive.file("f").expect("file");
        assert_eq!(file.versions.count, 3, "nothing trimmed");
        assert!(file.rebase.is_none(), "a no-op records no re-base");
        let after = reconstruct_all(&archive, "f");
        assert_eq!(after, before, "every index unchanged");
    }

    #[test]
    fn condense_on_a_mask_matching_no_file_is_a_typed_error() {
        let (base, records) = rich_forward();
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        let err = archive
            .condense("nope", 0)
            .expect_err("a mask matching no file must error");
        assert!(matches!(err, WalrusError::Rebase { .. }));
    }

    // === INV-3 byte-identical across a condense (forward/reverse/mixed) ==

    #[test]
    fn surviving_indices_reconstruct_identically_after_a_generations_trim_forward() {
        let (base, records) = rich_forward(); // count = 3
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        // generations = 2 -> keep indices [0, 1].
        set_generations(&mut archive, "f", 2);
        let before = reconstruct_all(&archive, "f");

        archive
            .enforce_generations("f")
            .expect("enforce generations");

        let after = reconstruct_all(&archive, "f");
        assert_eq!(after.len(), 2, "exactly N=2 versions remain");
        for k in 0..2 {
            assert_eq!(after[k], before[k], "forward index {k} byte-identical");
        }
    }

    #[test]
    fn surviving_indices_reconstruct_identically_after_a_generations_trim_reverse() {
        let (original, forward) = rich_forward();
        let (rbase, rrecords) = invert_forward_to_reverse(&original, &forward);
        let mut archive = single_file_archive("r", Direction::Reverse, &rbase, rrecords);
        set_generations(&mut archive, "r", 2);
        let before = reconstruct_all(&archive, "r");

        archive
            .enforce_generations("r")
            .expect("enforce generations");

        let after = reconstruct_all(&archive, "r");
        assert_eq!(after.len(), 2);
        for k in 0..2 {
            assert_eq!(after[k], before[k], "reverse index {k} byte-identical");
        }
    }

    #[test]
    fn a_wal_and_its_converted_wbl_reconstruct_identically_after_the_same_trim() {
        // AC3: a forward file and its inverted reverse twin over the SAME
        // history, condensed to the SAME keep, agree index-by-index after.
        let (original, forward) = rich_forward();
        let (rbase, rrecords) = invert_forward_to_reverse(&original, &forward);
        let mut fwd = single_file_archive("f", Direction::Forward, &original, forward);
        let mut rev = single_file_archive("r", Direction::Reverse, &rbase, rrecords);

        fwd.condense("f", 1).expect("forward condense");
        rev.condense("r", 1).expect("reverse condense");

        let fwd_after = reconstruct_all(&fwd, "f");
        let rev_after = reconstruct_all(&rev, "r");
        assert_eq!(fwd_after.len(), rev_after.len(), "same surviving range");
        for k in 0..fwd_after.len() {
            assert_eq!(fwd_after[k], rev_after[k], "wal/wbl agree at index {k}");
        }
    }

    // === generations enforcement ========================================

    #[test]
    fn adding_past_generations_n_drops_the_oldest_via_rebase_and_decreases_count() {
        let (base, records) = rich_forward(); // count = 3
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        set_generations(&mut archive, "f", 2); // keep 2 versions -> count' = 1

        archive.enforce_generations("f").expect("enforce");

        let file = archive.file("f").expect("file");
        assert_eq!(file.versions.count, 1, "count decreased to N-1");
        let rebase = file.rebase.as_ref().expect("re-base recorded");
        assert_eq!(rebase.count, 1, "one re-base operation");
        assert_eq!(rebase.dropped, 2, "two oldest versions dropped");
    }

    #[test]
    fn after_a_generations_trim_index_zero_is_still_the_current_version() {
        let (base, records) = rich_forward();
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        let current_before = archive
            .reconstruct(&FileId::from("f"), Version::Index(0))
            .expect("current");
        set_generations(&mut archive, "f", 2);

        archive.enforce_generations("f").expect("enforce");

        let current_after = archive
            .reconstruct(&FileId::from("f"), Version::Index(0))
            .expect("current after");
        assert_eq!(current_after, current_before, "index 0 stays current");
    }

    #[test]
    fn generations_zero_is_unlimited_and_never_trims() {
        let (base, records) = rich_forward();
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        // generations stays 0 (the default): unlimited.
        archive.enforce_generations("f").expect("enforce");
        let file = archive.file("f").expect("file");
        assert_eq!(file.versions.count, 3, "unlimited retention never trims");
        assert!(file.rebase.is_none(), "no re-base recorded");
    }

    // === checkpoints, no marker, validate ===============================

    #[test]
    fn a_condense_prunes_off_window_checkpoints_and_leaves_no_marker_record() {
        let (base, records) = rich_forward(); // count = 3
        let mut archive = single_file_archive("f", Direction::Forward, &base, records);
        {
            let file = archive
                .manifest_mut()
                .files
                .iter_mut()
                .find(|f| f.id == "f")
                .expect("file");
            file.checkpoints = vec![
                Checkpoint {
                    index: 0,
                    label: "current".into(),
                },
                Checkpoint {
                    index: 1,
                    label: "keep".into(),
                },
                Checkpoint {
                    index: 3,
                    label: "gone".into(),
                },
            ];
        }

        archive.condense("f", 1).expect("condense");

        let file = archive.file("f").expect("file");
        assert_eq!(file.checkpoints.len(), 2, "off-window checkpoint dropped");
        assert!(
            file.checkpoints.iter().all(|c| c.index <= 1),
            "no checkpoint exceeds count'"
        );
        // No marker record: every surviving record is a splice (no pseudo-op).
        assert!(
            archive
                .records("f")
                .expect("records")
                .iter()
                .all(|r| r.is_splice()),
            "re-base leaves no marker record in the log"
        );
        archive
            .manifest()
            .validate()
            .expect("trimmed manifest validates");
    }

    // === transactional guarantee ========================================

    #[test]
    fn a_failed_condense_leaves_the_archive_bytes_unchanged() {
        // Seed a real archive on disk, capture its exact bytes, then run a
        // TRANSACTIONAL write (the step that persists a condense) whose fill
        // fails PART-WAY. The rename never runs, so the on-disk archive must be
        // byte-for-byte identical to the capture. This exercises the same
        // atomic::write_atomically seam Archive::write routes through.
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("archive.wal");

        let (base, records) = rich_forward();
        let archive = single_file_archive("f", Direction::Forward, &base, records);
        archive.write(&dest).expect("seed archive on disk");
        let captured = std::fs::read(&dest).expect("read seeded archive");

        // Mutate in memory (a condense), then attempt a transactional write
        // that fails mid-fill after staging partial bytes to the temp file.
        let mut mutated =
            single_file_archive("f", Direction::Forward, &rich_forward().0, rich_forward().1);
        mutated.condense("f", 0).expect("condense in memory");
        let result = crate::atomic::write_atomically(&dest, |file| {
            use std::io::Write;
            file.write_all(b"PARTIAL").map_err(WalrusError::Io)?;
            Err(WalrusError::Rebase {
                reason: "injected mid-write failure".into(),
            })
        });

        assert!(result.is_err(), "the injected failure must surface");
        assert_eq!(
            std::fs::read(&dest).expect("re-read archive"),
            captured,
            "a failed write leaves the on-disk archive byte-for-byte unchanged"
        );
    }
}
