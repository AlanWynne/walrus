//! The FR-W7 add / update mutate surface (FORMAT.md sections 3, 4.4, 5).
//!
//! This module wires the published `add`, `update_with_splices` (always
//! available), gated `update(bytes)`, and `set_generations` entry points onto
//! the existing primitives: the single splice primitive, the one apply path
//! ([`crate::reconstruct::apply_record`], INV-4), the inversion helper
//! ([`crate::invert::invert_forward_splices`]), and the FR-W5 generations cap
//! ([`crate::Archive::enforce_generations`]).
//!
//! Every save stores EXACTLY ONE net splice (FR-W7, FORMAT.md 4.4): the incoming
//! forward splice list is folded onto the pre-save current and coalesced to the
//! single net splice spanning the minimal changed byte range before storing. The
//! stored shape is direction-aware (decision b): a FORWARD file appends the net
//! forward splice with its base fixed; a REVERSE file inverts the net splice and
//! advances its base to the new current.

use crate::archive::{Archive, ArchiveFile};
use crate::manifest::{Manifest, TrackedFile, Versions};
use crate::reconstruct::apply_record;
use crate::record::{ChangeRecord, Op};
use crate::wchg::{Direction, WchgHeader};
use crate::{invert, Result, WalrusError};

impl Archive {
    /// Add `name` with `bytes` as a new save.
    ///
    /// If `name` is not yet tracked this CREATES the file: its first version is
    /// both base and current with `count == 0` and only index 0 (decision a).
    /// `dir` picks the new file's direction (default: the archive variant's).
    /// If `name` IS tracked this records a new save via the direction-aware
    /// storage path, exactly as `update_with_splices` would.
    ///
    /// `generations` sets the retention cap at create time (and is re-applied on
    /// every save when `Some`).
    ///
    /// # Errors
    /// Returns [`WalrusError::BinaryInput`] when `bytes` is not valid UTF-8.
    pub fn add(
        &mut self,
        name: &str,
        bytes: &[u8],
        generations: Option<u32>,
        dir: Option<Direction>,
    ) -> Result<()> {
        let text = utf8_or_binary(bytes)?;

        if let Some(id) = self.id_for_selector(name) {
            self.store_save_from_text(&id, text, None)?;
            if let Some(n) = generations {
                self.set_generations_by_id(&id, n)?;
            }
            return Ok(());
        }

        // Create path: mint id, seed base = bytes at count 0, only index 0.
        let direction = dir.unwrap_or(self.manifest().variant.direction());
        let id = self.mint_id(name);
        let gens = generations.unwrap_or(0);
        let mut tracked = TrackedFile::new_with_direction(&id, name, direction, 0);
        tracked.generations = gens;
        let header = WchgHeader::new(direction, gens, name, "utf-8", 0);
        let af = ArchiveFile {
            id: id.clone(),
            base: bytes.to_vec(),
            header,
            records: Vec::new(),
        };
        let (manifest, files) = self.directories_mut();
        push_tracked(manifest, files, tracked, af);
        Ok(())
    }

    /// Record a save for the tracked file `name` from a caller-supplied list of
    /// FORWARD-sense splices (old -> new), coalesced to one net splice.
    ///
    /// Each splice's `at` and `at + delete_len` MUST land on a UTF-8 character
    /// boundary of the version it applies to; a mid-character offset or an
    /// out-of-range offset leaves the file UNMUTATED (scratch-swap-on-success).
    ///
    /// # Errors
    /// Returns [`WalrusError::NotTracked`] when `name` matches no tracked file,
    /// [`WalrusError::SpliceNotCharBoundary`] for an in-range offset that splits
    /// a character, and [`WalrusError::SpliceOutOfBounds`] for an offset past the
    /// end of the version.
    pub fn update_with_splices(&mut self, name: &str, splices: &[ChangeRecord]) -> Result<()> {
        let id = self
            .id_for_selector(name)
            .ok_or_else(|| WalrusError::NotTracked {
                name: name.to_string(),
            })?;
        // Compute the new current on a SCRATCH copy, validating every splice
        // against the version it applies to before anything is stored.
        let current = self.current_bytes(&id)?;
        let new_current = fold_validated(&current, splices)?;
        self.store_save(&id, &current, &new_current, None)
    }

    /// Diff `bytes` against the tracked file `name`'s current version and record
    /// the resulting save through the identical net-splice storage path as
    /// [`Archive::update_with_splices`]. Gated behind the `diff` feature.
    ///
    /// # Errors
    /// Returns [`WalrusError::NotTracked`] when `name` is absent and
    /// [`WalrusError::BinaryInput`] when `bytes` is not valid UTF-8.
    #[cfg(feature = "diff")]
    pub fn update(&mut self, name: &str, bytes: &[u8], generations: Option<u32>) -> Result<()> {
        self.update_inner(name, bytes, generations, None)
    }

    /// Shared implementation of [`Archive::update`] / [`Archive::update_at`]:
    /// diff `bytes` against the current version and record the net save,
    /// stamping the recorded change with `ts` when supplied. `pub(crate)` so the
    /// sibling `mutate_stamp` module can route `update_at` through it.
    #[cfg(feature = "diff")]
    pub(crate) fn update_inner(
        &mut self,
        name: &str,
        bytes: &[u8],
        generations: Option<u32>,
        ts: Option<&str>,
    ) -> Result<()> {
        let id = self
            .id_for_selector(name)
            .ok_or_else(|| WalrusError::NotTracked {
                name: name.to_string(),
            })?;
        let text = utf8_or_binary(bytes)?;
        let current = self.current_bytes(&id)?;
        let old = utf8_or_binary(&current)?;
        let diffed = crate::diff::diff(old, text)?;
        let new_current = fold_validated(&current, &diffed.splices)?;
        self.store_save(&id, &current, &new_current, ts)?;
        if let Some(n) = generations {
            self.set_generations_by_id(&id, n)?;
        }
        Ok(())
    }

    /// Set the retention cap for the tracked file `file` and apply it now.
    ///
    /// # Errors
    /// Returns [`WalrusError::NotTracked`] when `file` is absent.
    pub fn set_generations(&mut self, file: &str, n: u32) -> Result<()> {
        let id = self
            .id_for_selector(file)
            .ok_or_else(|| WalrusError::NotTracked {
                name: file.to_string(),
            })?;
        self.set_generations_by_id(&id, n)
    }

    // === private helpers ================================================

    /// Resolve a selector (name first, then id) to a tracked-file id.
    /// `pub(crate)` so the sibling `mutate_stamp` module can reuse it.
    pub(crate) fn id_for_selector(&self, sel: &str) -> Option<String> {
        self.manifest()
            .files
            .iter()
            .find(|f| f.name == sel || f.id == sel)
            .map(|f| f.id.clone())
    }

    /// The current (index 0) bytes of the tracked file `id`. `pub(crate)` for
    /// the sibling `mutate_stamp` module.
    pub(crate) fn current_bytes(&self, id: &str) -> Result<Vec<u8>> {
        self.reconstruct(&crate::FileId::from(id), crate::Version::Index(0))
    }

    /// Store a save for tracked file `id`, given the pre-save `old` current and
    /// the computed `new` current. Derives the one net splice and stores it
    /// direction-dependently (decision b). `old == new` stores nothing. When
    /// `ts` is `Some`, every change record created for this save is stamped with
    /// it (FR-W11 AC8); the caller has already validated its shape.
    pub(crate) fn store_save(
        &mut self,
        id: &str,
        old: &[u8],
        new: &[u8],
        ts: Option<&str>,
    ) -> Result<()> {
        let net = net_splice(old, new);
        let Some(mut net) = net else {
            return Ok(()); // no-op save: count +0.
        };
        net.ts = ts.map(str::to_string);
        let direction = self
            .manifest()
            .file(id)
            .map(|f| f.direction)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?;

        {
            let file = self.archive_file_mut(id)?;
            match direction {
                Direction::Forward => {
                    // Base fixed; append the net forward splice to the END.
                    file.records.push(net);
                }
                Direction::Reverse => {
                    // Invert against the current and advance base to new current.
                    // Stamp the inverted record(s) representing this save too, so
                    // a reverse file is addressable by --at exactly as a forward
                    // one is.
                    let (new_base, inverse) = invert::invert_forward_splices(old, &[net])?;
                    for mut rec in inverse.into_iter().rev() {
                        rec.ts = ts.map(str::to_string);
                        file.records.insert(0, rec);
                    }
                    file.base = new_base;
                }
            }
            let count = file.records.iter().filter(|r| r.is_splice()).count();
            file.header.count = count;
        }
        self.enforce_generations(id)?;
        self.rederive_versions(id)
    }

    /// Store a save from a UTF-8 string by diffing the net range byte-wise,
    /// stamping the recorded change with `ts` when supplied. `pub(crate)` for
    /// the sibling `mutate_stamp` module.
    pub(crate) fn store_save_from_text(
        &mut self,
        id: &str,
        text: &str,
        ts: Option<&str>,
    ) -> Result<()> {
        let current = self.current_bytes(id)?;
        self.store_save(id, &current, text.as_bytes(), ts)
    }

    /// Re-derive the manifest `versions` and `.wchg` header `count` for `id`
    /// from the stored splice count, after a save or a generations trim. Index 0
    /// stays current for both directions.
    fn rederive_versions(&mut self, id: &str) -> Result<()> {
        let count = self
            .files_mut()
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.records.iter().filter(|r| r.is_splice()).count())
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?;
        let tracked = self
            .manifest_mut()
            .files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?;
        tracked.versions = Versions::for_direction(tracked.direction, count);
        Ok(())
    }

    /// Set the generations cap on the manifest + header for `id`, then enforce.
    /// `pub(crate)` for the sibling `mutate_stamp` module.
    pub(crate) fn set_generations_by_id(&mut self, id: &str, n: u32) -> Result<()> {
        {
            let tracked = self
                .manifest_mut()
                .files
                .iter_mut()
                .find(|f| f.id == id)
                .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?;
            tracked.generations = n;
        }
        {
            let file = self.archive_file_mut(id)?;
            file.header.generations = n;
        }
        self.enforce_generations(id)
    }

    /// Mint a deterministic, archive-unique, filesystem-safe id for `name`
    /// (M2): slugify, then append the smallest free `-N` (N >= 2) suffix.
    fn mint_id(&self, name: &str) -> String {
        let base = slugify(name);
        let taken = |candidate: &str| self.manifest().files.iter().any(|f| f.id == candidate);
        if !taken(&base) {
            return base;
        }
        let mut n = 2usize;
        loop {
            let candidate = format!("{base}-{n}");
            if !taken(&candidate) {
                return candidate;
            }
            n += 1;
        }
    }
}

// === free functions =====================================================

/// Push a newly-created file into BOTH directories in lockstep so
/// [`Archive::to_bytes`] never sees a half-added file (section 2a).
pub(crate) fn push_tracked(
    manifest: &mut Manifest,
    files: &mut Vec<ArchiveFile>,
    tracked: TrackedFile,
    af: ArchiveFile,
) {
    manifest.files.push(tracked);
    files.push(af);
}

/// Validate `bytes` as UTF-8, mapping failure to the non-UTF-8 `BinaryInput`
/// reason (distinct from the NUL-byte reason in `diff`). `pub(crate)` for the
/// sibling `mutate_stamp` module.
pub(crate) fn utf8_or_binary(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| WalrusError::BinaryInput {
        reason: "input is not valid UTF-8; declare a source encoding (i18n) for a non-UTF-8 file"
            .to_string(),
    })
}

/// Fold `splices` onto a SCRATCH copy of `current`, validating each splice's
/// `at`/`at + delete_len` against the evolving scratch bytes. Returns the folded
/// new current. The live file is never touched, so a bad splice leaves it as-is.
fn fold_validated(current: &[u8], splices: &[ChangeRecord]) -> Result<Vec<u8>> {
    let mut scratch = current.to_vec();
    for record in splices {
        if let Op::Splice { at, delete_len, .. } = &record.op {
            let end = at
                .checked_add(*delete_len)
                .ok_or(WalrusError::SpliceOutOfBounds {
                    at: *at,
                    len: scratch.len(),
                })?;
            if end > scratch.len() {
                return Err(WalrusError::SpliceOutOfBounds {
                    at: end,
                    len: scratch.len(),
                });
            }
            // Both edges must fall on char boundaries of the scratch &str view.
            let view =
                std::str::from_utf8(&scratch).map_err(|_| WalrusError::SpliceOutOfBounds {
                    at: *at,
                    len: scratch.len(),
                })?;
            if !view.is_char_boundary(*at) {
                return Err(WalrusError::SpliceNotCharBoundary { at: *at });
            }
            if !view.is_char_boundary(end) {
                return Err(WalrusError::SpliceNotCharBoundary { at: end });
            }
        }
        apply_record(&mut scratch, record)?;
    }
    Ok(scratch)
}

/// Compute the single NET splice between `old` and `new` (M3, byte-level), or
/// `None` when they are equal (a no-op save). `p` = common-prefix length, `s` =
/// common-suffix length bounded so `p + s <= min(len)`; the net splice is
/// `{ at: p, delete_len: old.len - p - s, insert: new[p .. new.len - s] }`.
fn net_splice(old: &[u8], new: &[u8]) -> Option<ChangeRecord> {
    if old == new {
        return None;
    }
    let min_len = old.len().min(new.len());
    let mut p = 0usize;
    while p < min_len && old[p] == new[p] {
        p += 1;
    }
    let mut s = 0usize;
    while s < min_len - p && old[old.len() - 1 - s] == new[new.len() - 1 - s] {
        s += 1;
    }
    let delete_len = old.len() - p - s;
    // `insert` must be valid UTF-8; `new` is a UTF-8 byte slice and both p and
    // new.len()-s are byte offsets, but the net range is derived from byte
    // equality, so the slice is a contiguous UTF-8 fragment of `new`.
    let insert = String::from_utf8_lossy(&new[p..new.len() - s]).into_owned();
    Some(ChangeRecord::splice(p, delete_len, insert))
}

/// Slugify `name` into a filesystem-safe id (M2): lowercase ASCII alnum kept,
/// every other char becomes `_`, repeats collapsed, leading/trailing `_`
/// trimmed, empty -> "file".
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_underscore = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.to_string()
    }
}

// These Archive-level tests live in-crate (not in `tests/`) because the fixture
// seam `Archive::from_parts_for_test` is `#[cfg(test)] pub(crate)`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, Variant};
    use crate::{FileId, Version};

    // === fixtures =======================================================

    /// An empty archive (zero tracked files) of the given variant.
    fn empty_archive(variant: Variant) -> Archive {
        Archive::from_parts_for_test(Manifest::new(variant), Vec::new())
    }

    /// Reconstruct one index of a tracked file.
    fn at(archive: &Archive, id: &str, index: usize) -> Vec<u8> {
        archive
            .reconstruct(&FileId::from(id), Version::Index(index))
            .expect("reconstruct index")
    }

    fn count_of(archive: &Archive, id: &str) -> usize {
        archive.file(id).expect("file").versions.count
    }

    // === AC1: add create + storage ======================================

    #[test]
    fn add_missing_file_creates_count_zero_single_version() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        assert_eq!(count_of(&a, &id), 0, "fresh add has count 0");
        assert_eq!(at(&a, &id, 0), b"hello", "index 0 is the added bytes");
    }

    #[test]
    fn second_add_appends_exactly_one_net_record_index1_is_old() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("first add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        a.add("doc.txt", b"hello world", None, None)
            .expect("second add");
        assert_eq!(count_of(&a, &id), 1, "exactly one net record appended");
        assert_eq!(at(&a, &id, 0), b"hello world", "index 0 is new");
        assert_eq!(at(&a, &id, 1), b"hello", "index 1 is old");
    }

    #[test]
    fn add_mints_deterministic_slug_id() {
        let mut a = empty_archive(Variant::Wal);
        a.add("src/Doc.TXT", b"x", None, None).expect("add");
        assert!(a.file("src_doc_txt").is_some(), "slug is deterministic");

        // A second file slugging to the same base gets `-2`.
        a.add("src\\doc!txt", b"y", None, None).expect("add 2");
        assert!(
            a.file("src_doc_txt-2").is_some(),
            "collision gets -2 suffix"
        );
    }

    #[test]
    fn add_empty_then_real_lands_empty_at_far_index_forward() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"", None, Some(Direction::Forward))
            .expect("empty add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        a.add("doc.txt", b"real", None, None).expect("real add");
        assert_eq!(at(&a, &id, 0), b"real", "real is current (index 0)");
        assert_eq!(at(&a, &id, 1), b"", "empty original is at the far index");
    }

    #[test]
    fn add_empty_then_real_lands_empty_at_far_index_reverse() {
        let mut a = empty_archive(Variant::Wbl);
        a.add("doc.txt", b"", None, Some(Direction::Reverse))
            .expect("empty add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        a.add("doc.txt", b"real", None, None).expect("real add");
        assert_eq!(at(&a, &id, 0), b"real", "real is current (index 0)");
        assert_eq!(at(&a, &id, 1), b"", "empty original is at the far index");
    }

    #[test]
    fn forward_file_appends_net_splice_base_unchanged() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"abc", None, Some(Direction::Forward))
            .expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        let base_before = a.base(&id).expect("base").to_vec();
        a.add("doc.txt", b"abcX", None, None).expect("save");
        assert_eq!(a.base(&id).expect("base"), &base_before[..], "base fixed");
        assert_eq!(at(&a, &id, 1), b"abc", "old at index 1");
        assert_eq!(at(&a, &id, 0), b"abcX", "new at index 0");
    }

    #[test]
    fn reverse_file_inverts_and_advances_base() {
        let mut a = empty_archive(Variant::Wbl);
        a.add("doc.txt", b"abc", None, Some(Direction::Reverse))
            .expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        a.add("doc.txt", b"abcX", None, None).expect("save");
        // Reverse base is the CURRENT, so it advances to the new current.
        assert_eq!(a.base(&id).expect("base"), b"abcX", "base advanced to new");
        assert_eq!(at(&a, &id, 0), b"abcX", "new at index 0");
        assert_eq!(at(&a, &id, 1), b"abc", "old at index 1");
    }

    #[test]
    fn net_splice_inserting_into_repeated_run_pins_at() {
        // old = "aaaa", new = "aaXaa": prefix wins, suffix clamped -> at == 2.
        let net = net_splice(b"aaaa", b"aaXaa").expect("a change");
        if let Op::Splice {
            at,
            delete_len,
            insert,
        } = net.op
        {
            assert_eq!(at, 2, "prefix wins the tie-break");
            assert_eq!(delete_len, 0);
            assert_eq!(insert, "X");
        } else {
            panic!("expected a splice");
        }
        // old == new stores nothing.
        assert!(
            net_splice(b"same", b"same").is_none(),
            "no-op stores nothing"
        );
    }

    #[test]
    fn no_op_save_does_not_advance_count() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"abc", None, None).expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        a.add("doc.txt", b"abc", None, None).expect("no-op save");
        assert_eq!(count_of(&a, &id), 0, "a no-op save stores nothing (+0)");
    }

    // === AC2 / AC5 / M-1 / N4 / H2: update_with_splices =================

    #[test]
    fn update_with_splices_on_untracked_returns_not_tracked() {
        let mut a = empty_archive(Variant::Wal);
        let err = a
            .update_with_splices("nope", &[ChangeRecord::splice(0, 0, "x")])
            .expect_err("untracked must error");
        assert!(matches!(err, WalrusError::NotTracked { .. }));
    }

    #[test]
    fn update_with_splices_forward_reconstructs_byte_identically() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, Some(Direction::Forward))
            .expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        // Two forward pieces for ONE save: "hello" -> "hello!" -> "Hello!".
        a.update_with_splices(
            "doc.txt",
            &[
                ChangeRecord::splice(5, 0, "!"),
                ChangeRecord::splice(0, 1, "H"),
            ],
        )
        .expect("update");
        assert_eq!(count_of(&a, &id), 1, "one save advances count by one");
        assert_eq!(at(&a, &id, 0), b"Hello!", "new current");
        assert_eq!(at(&a, &id, 1), b"hello", "pre-save current at index 1");
    }

    #[test]
    fn one_save_of_k_pieces_advances_count_by_one() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"", None, None).expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        a.update_with_splices(
            "doc.txt",
            &[
                ChangeRecord::splice(0, 0, "a"),
                ChangeRecord::splice(1, 0, "b"),
                ChangeRecord::splice(2, 0, "c"),
            ],
        )
        .expect("update");
        assert_eq!(count_of(&a, &id), 1, "K pieces coalesce to one save");
        assert_eq!(at(&a, &id, 0), b"abc");
    }

    #[test]
    fn update_with_splices_mid_char_splice_returns_splice_not_char_boundary() {
        let mut a = empty_archive(Variant::Wal);
        // A two-byte char so an at=1 offset splits it.
        let two_byte: String = ['\u{00E9}'].iter().collect(); // 'e'-acute, 2 bytes
        a.add("doc.txt", two_byte.as_bytes(), None, None)
            .expect("add");
        let err = a
            .update_with_splices("doc.txt", &[ChangeRecord::splice(1, 0, "x")])
            .expect_err("mid-char must error");
        assert!(matches!(err, WalrusError::SpliceNotCharBoundary { at: 1 }));
    }

    #[test]
    fn update_with_splices_out_of_range_returns_splice_out_of_bounds() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"abc", None, None).expect("add");
        let err = a
            .update_with_splices("doc.txt", &[ChangeRecord::splice(10, 0, "x")])
            .expect_err("out of range must error");
        assert!(matches!(err, WalrusError::SpliceOutOfBounds { .. }));
    }

    #[test]
    fn update_with_splices_bad_splice_leaves_file_unmutated() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"abc", None, None).expect("add");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        let base_before = a.base(&id).expect("base").to_vec();
        let count_before = count_of(&a, &id);
        let _ = a.update_with_splices("doc.txt", &[ChangeRecord::splice(99, 0, "x")]);
        assert_eq!(
            a.base(&id).expect("base"),
            &base_before[..],
            "base unchanged"
        );
        assert_eq!(count_of(&a, &id), count_before, "count unchanged");
        assert_eq!(at(&a, &id, 0), b"abc", "current unchanged");
    }

    // === AC2: set_generations ===========================================

    #[test]
    fn set_generations_on_untracked_returns_not_tracked() {
        let mut a = empty_archive(Variant::Wal);
        let err = a
            .set_generations("nope", 3)
            .expect_err("untracked must error");
        assert!(matches!(err, WalrusError::NotTracked { .. }));
    }

    #[test]
    fn set_generations_sets_cap_and_enforces() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"v0", None, None).expect("add");
        a.update_with_splices("doc.txt", &[ChangeRecord::splice(0, 2, "v1")])
            .expect("save 1");
        a.update_with_splices("doc.txt", &[ChangeRecord::splice(0, 2, "v2")])
            .expect("save 2");
        let id = a.file("doc.txt").expect("tracked").id.clone();
        assert_eq!(count_of(&a, &id), 2, "two saves");
        // Keep only 1 version (indices [0,0]) -> count 0.
        a.set_generations("doc.txt", 1).expect("set generations");
        assert_eq!(count_of(&a, &id), 0, "trimmed to 1 version");
        assert_eq!(a.file(&id).expect("file").generations, 1);
    }
}

// === gated update(bytes) tests ==========================================
#[cfg(all(test, feature = "diff"))]
mod diff_tests {
    use super::*;
    use crate::manifest::{Manifest, Variant};
    use crate::{FileId, Version};

    fn empty_archive() -> Archive {
        Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new())
    }

    #[test]
    fn update_on_untracked_returns_not_tracked() {
        let mut a = empty_archive();
        let err = a
            .update("nope", b"x", None)
            .expect_err("untracked must error");
        assert!(matches!(err, WalrusError::NotTracked { .. }));
    }

    #[test]
    fn update_bytes_equals_update_with_splices_of_diffed_splices() {
        // update(bytes) and update_with_splices(diff(old,new).splices) must land
        // the SAME current and the same count delta.
        let mut via_bytes = empty_archive();
        via_bytes
            .add("doc.txt", b"the cat", None, None)
            .expect("add");
        via_bytes
            .update("doc.txt", b"the black cat", None)
            .expect("update bytes");

        let mut via_splices = empty_archive();
        via_splices
            .add("doc.txt", b"the cat", None, None)
            .expect("add");
        let diffed = crate::diff::diff("the cat", "the black cat").expect("diff");
        via_splices
            .update_with_splices("doc.txt", &diffed.splices)
            .expect("update splices");

        let id = "doc_txt";
        let b0 = via_bytes
            .reconstruct(&FileId::from(id), Version::Index(0))
            .expect("current");
        let s0 = via_splices
            .reconstruct(&FileId::from(id), Version::Index(0))
            .expect("current");
        assert_eq!(b0, b"the black cat");
        assert_eq!(b0, s0, "both paths land the same current");
        assert_eq!(
            via_bytes.file(id).expect("f").versions.count,
            via_splices.file(id).expect("f").versions.count,
            "same count delta"
        );
    }
}
