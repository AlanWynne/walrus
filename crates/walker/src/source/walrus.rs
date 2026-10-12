//! The `WalrusSource` adapter: a [`HistorySource`] over a multi-file
//! `walrus-fmt` [`Archive`] (FR-K2).
//!
//! This is the ONE module in `walker` allowed to name `walrus_fmt` archive
//! types. Everything else in the GUI depends only on the format-agnostic
//! [`HistorySource`] trait (`source/mod.rs`), so swapping or adding a backend
//! touches this file alone (FR-K1 AC2 boundary).
//!
//! Index semantics are shared end-to-end: walker's [`VersionId::Index`] and
//! `walrus_fmt`'s [`Version::Index`] both put the CURRENT version at index 0 and
//! the ORIGINAL at the file's splice `count`, for both the forward (`.wal`) and
//! reverse (`.wbl`) directions (FORMAT.md section 5). The index therefore passes
//! straight through without remapping.

use anyhow::{Context, Result};
use walrus_fmt::{Archive, FileId, Version};

use super::{FileInfo, FileRef, HistoryEntry, HistorySource, VersionId, VersionMeta};

// === WalrusSource =======================================================

/// A read-only [`HistorySource`] backed by an owned `walrus-fmt` [`Archive`].
///
/// Open one with [`WalrusSource::open`]. The archive's manifest `files` array is
/// the authoritative directory; this adapter maps each tracked file's 0-based
/// manifest position to the stable [`FileId`] the archive reconstructs against,
/// so a [`FileRef::id`] round-trips to the right file.
#[derive(Debug)]
pub struct WalrusSource {
    archive: Archive,
    /// Archive [`FileId`]s indexed by 0-based manifest position. A
    /// [`FileRef::id`] is an index into this vector; the value is the archive id
    /// used to look the file up for reconstruction and metadata.
    file_ids: Vec<FileId>,
}

impl WalrusSource {
    /// Open a `.wal` or `.wbl` archive from `path`.
    ///
    /// # Errors
    /// Returns an error (with context naming the path) if the archive cannot be
    /// read: an unknown codec, a missing or invalid manifest, or a malformed
    /// per-file base/`.wchg` member.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<WalrusSource> {
        let path = path.as_ref();
        let archive = Archive::open(path)
            .with_context(|| format!("opening Walrus archive {}", path.display()))?;
        let file_ids = archive
            .files()
            .map(|tracked| FileId::from(tracked.id.as_str()))
            .collect();
        Ok(WalrusSource { archive, file_ids })
    }

    /// The archive [`FileId`] for a [`FileRef`], or `None` if the ref's id is
    /// outside the manifest range (a stale or foreign ref).
    fn file_id(&self, f: &FileRef) -> Option<&FileId> {
        self.file_ids.get(f.id())
    }

    /// The splice count (far index = the original) for the tracked file at
    /// 0-based manifest position `index`, or `None` if out of range. The count
    /// is read from the tracked file's own version bookkeeping, which the reader
    /// derives from its `.wchg`.
    fn splice_count(&self, index: usize) -> Option<usize> {
        self.archive.files().nth(index).map(|t| t.versions.count)
    }

    /// The tracked file at the [`FileRef`]'s 0-based manifest position, or
    /// `None` for a stale or foreign ref.
    fn tracked(&self, f: &FileRef) -> Option<&walrus_fmt::TrackedFile> {
        self.archive.files().nth(f.id())
    }
}

// === Import (FR-K5): the first WRITE path ===============================

// The import (WRITE) orchestration -- the `MutateArchive` seam,
// `apply_resolution`, `WalrusSource::import`, and the gated real-archive impl --
// lives in a child module split out to keep this file under the 400 non-test-
// line standard (NFR-1). It is a CHILD of `walrus` (via `#[path]`), so it stays
// inside the one `walrus_fmt`-aware adapter and may touch `WalrusSource`'s
// private fields and name `walrus_fmt` types without crossing the FR-K1 AC2
// boundary.
#[path = "walrus_import.rs"]
mod walrus_import;

// The FR-K9 `history()` mapping lives in a child module split out to keep this
// file under the 400 non-test-line standard (NFR-1). Same `#[path]` child
// pattern as `walrus_import`: it stays inside the one `walrus_fmt`-aware adapter
// and may name `walrus_fmt` types (FR-K1 AC2).
#[path = "walrus_history.rs"]
mod walrus_history;

// The FR-K6 per-file + batch mutate orchestration (set-generations / condense /
// remove, each through the transactional `Archive::write`) lives in a child
// module split out to keep this file under the 400 non-test-line standard
// (NFR-1). Same `#[path]` child pattern as `walrus_import`: it stays inside the
// one `walrus_fmt`-aware adapter and may name `walrus_fmt` types + touch
// `WalrusSource` privates without crossing the FR-K1 AC2 boundary.
#[path = "walrus_mutate.rs"]
mod walrus_mutate;

// === HistorySource ======================================================

impl HistorySource for WalrusSource {
    fn files(&self) -> Vec<FileRef> {
        self.archive
            .files()
            .enumerate()
            .map(|(index, tracked)| FileRef::new(index, tracked.name.as_str()))
            .collect()
    }

    fn versions(&self, f: &FileRef) -> Vec<VersionId> {
        // Valid indices are [0, count] INCLUSIVE: 0 is current, count is the
        // original. An out-of-range ref yields no versions.
        match self.splice_count(f.id()) {
            Some(count) => (0..=count).map(VersionId::Index).collect(),
            None => Vec::new(),
        }
    }

    fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>> {
        let file_id = self
            .file_id(f)
            .ok_or_else(|| anyhow::anyhow!("no tracked file for id {}", f.id()))?;
        let VersionId::Index(index) = v;
        // Index semantics are identical across the two layers (0 = current,
        // count = original), so the index passes straight through.
        self.archive
            .reconstruct(file_id, Version::Index(*index))
            .with_context(|| format!("reconstructing {} at version index {}", f.name(), index))
    }

    fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta {
        let VersionId::Index(index) = v;
        metadata_for(&self.archive, f.id(), *index)
    }

    fn content_hint(&self, f: &FileRef) -> (String, String) {
        match self.archive.files().nth(f.id()) {
            Some(tracked) => (tracked.mime.clone(), tracked.name.clone()),
            None => (String::new(), String::new()),
        }
    }

    fn history(&self, f: &FileRef) -> Vec<HistoryEntry> {
        self.history_entries(f)
    }

    fn file_info(&self, f: &FileRef) -> FileInfo {
        let Some(tracked) = self.tracked(f) else {
            // A stale or foreign ref yields a zeroed listing rather than panics.
            return FileInfo {
                index_size: 0,
                generations: None,
                size_bytes: 0,
                log_bytes: 0,
                mime: String::new(),
            };
        };
        // Indices span [0, count] inclusive, so the version count is count + 1.
        let index_size = tracked.versions.count + 1;
        // generations == 0 means unlimited in the 0.1 format.
        let generations = (tracked.generations != 0).then_some(tracked.generations);
        let size_bytes = self
            .reconstruct(f, &VersionId::Index(0))
            .map(|bytes| bytes.len() as u64)
            .unwrap_or(0);
        let log_bytes = self.log_bytes(f);
        FileInfo {
            index_size,
            generations,
            size_bytes,
            log_bytes,
            mime: tracked.mime.clone(),
        }
    }
}

impl WalrusSource {
    /// The archive id string for `f`, or `None` for a stale/foreign ref.
    /// A small owned-`String` accessor the `walrus_history` child reuses so it
    /// never touches `file_ids` directly.
    pub(super) fn file_id_for(&self, f: &FileRef) -> Option<String> {
        self.file_id(f).map(|id| id.as_str().to_string())
    }

    /// The splice count (far index = original) for `f`, or `None` if foreign.
    pub(super) fn splice_count_for(&self, f: &FileRef) -> Option<usize> {
        self.splice_count(f.id())
    }

    /// The archive id string for the tracked file at 0-based manifest position
    /// `index`, or `None` when `index` is out of range (a stale/foreign id).
    /// The FR-K6 batch mutate path resolves each selected id this way.
    pub(super) fn file_id_by_index(&self, index: usize) -> Option<String> {
        self.file_ids.get(index).map(|id| id.as_str().to_string())
    }

    /// The change records for archive id `id`, or `None` when absent.
    pub(super) fn records_for(&self, id: &str) -> Option<&[walrus_fmt::ChangeRecord]> {
        self.archive.records(id)
    }

    /// The derived change-log byte figure for `f`: the deterministic sum of the
    /// serialized JSON byte lengths of each change record (there is no raw
    /// `log_bytes` accessor on the archive). Returns 0 when the file has no
    /// records or the ref is foreign.
    fn log_bytes(&self, f: &FileRef) -> u64 {
        let Some(id) = self.file_id(f) else {
            return 0;
        };
        match self.archive.records(id.as_str()) {
            Some(records) => records
                .iter()
                .map(|record| {
                    serde_json::to_vec(record)
                        .map(|v| v.len() as u64)
                        .unwrap_or(0)
                })
                .sum(),
            None => 0,
        }
    }
}

// === metadata mapping ===================================================

/// Build the [`VersionMeta`] for the tracked file at 0-based manifest position
/// `file_index` at version `index` (0 = current, count = original).
///
/// - `label`: the label of a checkpoint whose index equals `index`, else the
///   index rendered as a string (a sensible, stable default).
/// - `timestamp`: epoch seconds parsed from the `ts` of the splice record that
///   produced this version, when present and in the fixed ISO-8601 shape.
///   Index 0 (current) uses the last splice; the original end has no record and
///   so no timestamp.
/// - `author`: always `None` -- a change record carries no author field in the
///   0.1 format (`record.rs`), so there is nothing to surface.
fn metadata_for(archive: &Archive, file_index: usize, index: usize) -> VersionMeta {
    let Some(tracked) = archive.files().nth(file_index) else {
        return empty_meta(index);
    };
    let label = tracked
        .checkpoints
        .iter()
        .find(|c| c.index == index)
        .map(|c| c.label.clone())
        .unwrap_or_else(|| index.to_string());

    let timestamp = record_timestamp(archive, &tracked.id, tracked.versions.count, index);

    VersionMeta {
        label,
        timestamp,
        author: None,
    }
}

/// A default [`VersionMeta`] (index as label, no timestamp/author) for an
/// out-of-range file.
fn empty_meta(index: usize) -> VersionMeta {
    VersionMeta {
        label: index.to_string(),
        timestamp: None,
        author: None,
    }
}

/// The epoch-seconds timestamp for the splice that produced version `index`.
///
/// FORMAT.md 4.1: the `i`-th splice (1-based, oldest step first) yields the
/// forward version whose canonical index is `count - i`. So version `index` was
/// produced by splice ordinal `count - index`; ordinal 0 is the original base,
/// which no record produced, so it has no timestamp. Records that lack a `ts` or
/// carry one outside the fixed ISO-8601 shape yield `None`.
fn record_timestamp(archive: &Archive, id: &str, count: usize, index: usize) -> Option<i64> {
    if index >= count {
        // index == count is the original (no producing record); index > count is
        // out of range. Either way there is no record timestamp.
        return None;
    }
    let ordinal = count - index; // 1-based splice ordinal that produced `index`.
    let records = archive.records(id)?;
    let record = nth_splice(records, ordinal)?;
    record.ts.as_deref().and_then(parse_iso_epoch)
}

/// The `n`-th splice record (1-based) in log order, skipping checkpoints.
fn nth_splice(records: &[walrus_fmt::ChangeRecord], n: usize) -> Option<&walrus_fmt::ChangeRecord> {
    records
        .iter()
        .filter(|r| r.is_splice())
        .nth(n.checked_sub(1)?)
}

// === timestamp parsing ==================================================

/// Parse the fixed ISO-8601 UTC shape `YYYY-MM-DDThh:mm:ssZ` into epoch seconds.
///
/// This mirrors the strict, dependency-free parser the library uses internally
/// for `Timestamp` addressing (that parser is crate-private, so the adapter
/// re-implements the same fixed shape here rather than pulling in a date crate).
/// It accepts ONLY the exact 20-char shape the writer emits and returns `None`
/// for anything else.
fn parse_iso_epoch(ts: &str) -> Option<i64> {
    let bytes = ts.as_bytes();
    if bytes.len() != 20 {
        return None;
    }
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }
    let year = parse_digits(&bytes[0..4])?;
    let month = parse_digits(&bytes[5..7])?;
    let day = parse_digits(&bytes[8..10])?;
    let hour = parse_digits(&bytes[11..13])?;
    let minute = parse_digits(&bytes[14..16])?;
    let second = parse_digits(&bytes[17..19])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Parse an ASCII digit run into an `i64`; `None` if any byte is not a digit.
fn parse_digits(bytes: &[u8]) -> Option<i64> {
    let mut value: i64 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i64::from(b - b'0');
    }
    Some(value)
}

/// Days since the Unix epoch for a proleptic-Gregorian civil date
/// (Howard Hinnant's days_from_civil), kept branch-light.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// === Tests ==============================================================
#[cfg(test)]
mod tests {
    use super::*;
    use walrus_fmt::{ChangeRecord, Direction, Manifest, TrackedFile, Variant, WchgHeader};

    // The import ORCHESTRATION tests (the `MutateArchive` seam + a recording
    // `FakeArchive`) moved with the orchestration code into the `walrus_import`
    // child module. The on-disk real-archive tests (AC1 + the transactional
    // guarantee) live in `tests/import.rs`.

    /// Build the compressed container bytes for a single forward tracked file,
    /// so the unit tests can open a real `WalrusSource` without the integration
    /// fixture builder.
    fn single_forward_container(
        id: &str,
        name: &str,
        base: &[u8],
        records: &[ChangeRecord],
    ) -> Vec<u8> {
        let count = records.iter().filter(|r| r.is_splice()).count();
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(TrackedFile::new_with_direction(
            id,
            name,
            Direction::Forward,
            count,
        ));

        let header = WchgHeader::new(Direction::Forward, 0, name, "utf-8", count);
        let mut wchg = serde_json::to_string(&header).expect("header");
        wchg.push('\n');
        for record in records {
            wchg.push_str(&serde_json::to_string(record).expect("record"));
            wchg.push('\n');
        }

        let mut builder = tar::Builder::new(Vec::new());
        append(
            &mut builder,
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest).expect("manifest"),
        );
        append(&mut builder, &format!("files/{id}/base"), base);
        append(
            &mut builder,
            &format!("files/{id}/{name}.wchg"),
            wchg.as_bytes(),
        );
        let tar_bytes = builder.into_inner().expect("tar");
        zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd")
    }

    fn append(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) {
        let mut header = tar::Header::new_ustar();
        header.set_path(path).expect("path");
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        builder.append(&header, data).expect("append");
    }

    /// Open a `WalrusSource` over an in-memory container by writing it to a
    /// tempdir first (there is no in-memory `WalrusSource` constructor).
    fn source_from(bytes: &[u8], name: &str) -> (tempfile::TempDir, WalrusSource) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).expect("write");
        let source = WalrusSource::open(&path).expect("open");
        (dir, source)
    }

    #[test]
    fn file_ref_id_round_trips_to_the_archive_file_id() {
        // Two files: the FileRef id (0-based manifest position) must select the
        // matching archive file for reconstruction.
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(TrackedFile::new_with_direction(
            "a0",
            "a.txt",
            Direction::Forward,
            1,
        ));
        manifest.files.push(TrackedFile::new_with_direction(
            "b1",
            "b.txt",
            Direction::Forward,
            0,
        ));

        // Build the two-file container by hand (base + one splice for a0).
        let mut builder = tar::Builder::new(Vec::new());
        append(
            &mut builder,
            "manifest.json",
            &serde_json::to_vec_pretty(&manifest).expect("m"),
        );
        let ha = WchgHeader::new(Direction::Forward, 0, "a.txt", "utf-8", 1);
        let mut wa = serde_json::to_string(&ha).expect("ha");
        wa.push('\n');
        wa.push_str(&serde_json::to_string(&ChangeRecord::splice(1, 0, "X")).expect("rec"));
        wa.push('\n');
        append(&mut builder, "files/a0/base", b"a");
        append(&mut builder, "files/a0/a.txt.wchg", wa.as_bytes());
        let hb = WchgHeader::new(Direction::Forward, 0, "b.txt", "utf-8", 0);
        let mut wb = serde_json::to_string(&hb).expect("hb");
        wb.push('\n');
        append(&mut builder, "files/b1/base", b"b");
        append(&mut builder, "files/b1/b.txt.wchg", wb.as_bytes());
        let tar_bytes = builder.into_inner().expect("tar");
        let bytes = zstd::stream::encode_all(&tar_bytes[..], 19).expect("zstd");

        let (_dir, source) = source_from(&bytes, "two.wal");
        let files = source.files();
        assert_eq!(files[0].id(), 0);
        assert_eq!(files[1].id(), 1);
        // id 0 is "a.txt": current (index 0) is "aX", original (index 1) is "a".
        assert_eq!(
            source
                .reconstruct(&files[0], &VersionId::Index(0))
                .expect("cur"),
            b"aX"
        );
        assert_eq!(
            source
                .reconstruct(&files[0], &VersionId::Index(1))
                .expect("orig"),
            b"a"
        );
        // id 1 is "b.txt": a single version (base only).
        assert_eq!(
            source
                .reconstruct(&files[1], &VersionId::Index(0))
                .expect("b cur"),
            b"b"
        );
    }

    #[test]
    fn content_hint_returns_the_tracked_mime_and_name() {
        let bytes = single_forward_container("f0", "notes.txt", b"hi", &[]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let (mime, name) = source.content_hint(&source.files()[0]);
        assert_eq!(name, "notes.txt");
        // TrackedFile::new_with_direction defaults the mime to text/plain.
        assert_eq!(mime, "text/plain");
    }

    #[test]
    fn metadata_label_defaults_to_the_index_when_no_checkpoint_matches() {
        let records = [ChangeRecord::splice(2, 0, "!")];
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let meta = source.metadata(&source.files()[0], &VersionId::Index(1));
        assert_eq!(meta.label, "1");
        assert_eq!(meta.author, None);
    }

    #[test]
    fn metadata_timestamp_comes_from_the_producing_splice_record() {
        // One splice (count = 1) carrying a timestamp. Index 0 (current) is
        // produced by splice ordinal count - 0 = 1, so it carries that ts.
        let mut rec = ChangeRecord::splice(2, 0, "!");
        rec.ts = Some("1970-01-01T00:00:00Z".to_string());
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &[rec]);
        let (_dir, source) = source_from(&bytes, "x.wal");

        let current = source.metadata(&source.files()[0], &VersionId::Index(0));
        assert_eq!(current.timestamp, Some(0));

        // The original end (index == count) has no producing record, so none.
        let original = source.metadata(&source.files()[0], &VersionId::Index(1));
        assert_eq!(original.timestamp, None);
    }

    // === file_info (FR-K4, Option A) ====================================

    #[test]
    fn walrus_file_info_index_size_is_splice_count_plus_one() {
        // One splice -> count 1 -> indices [0, 1] inclusive -> index_size 2.
        let records = [ChangeRecord::splice(2, 0, "!")];
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let file = &source.files()[0];
        assert_eq!(source.file_info(file).index_size, 2);
        assert_eq!(
            source.file_info(file).index_size,
            source.versions(file).len()
        );
    }

    #[test]
    fn walrus_file_info_generations_zero_maps_to_none() {
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &[]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        // TrackedFile::new_with_direction defaults generations to 0 = unlimited.
        assert_eq!(source.file_info(&source.files()[0]).generations, None);
    }

    #[test]
    fn walrus_file_info_log_bytes_sums_serialized_record_lengths() {
        let records = [
            ChangeRecord::splice(0, 0, "a"),
            ChangeRecord::splice(1, 0, "bc"),
        ];
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let expected: u64 = records
            .iter()
            .map(|r| serde_json::to_vec(r).expect("ser").len() as u64)
            .sum();
        assert_eq!(source.file_info(&source.files()[0]).log_bytes, expected);
    }

    #[test]
    fn walrus_file_info_size_bytes_is_the_current_reconstruction_length() {
        // base "hi" + insert "!" at offset 2 -> current "hi!" (3 bytes).
        let records = [ChangeRecord::splice(2, 0, "!")];
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let file = &source.files()[0];
        let current_len = source
            .reconstruct(file, &VersionId::Index(0))
            .expect("cur")
            .len() as u64;
        assert_eq!(source.file_info(file).size_bytes, current_len);
        assert_eq!(source.file_info(file).size_bytes, 3);
    }

    #[test]
    fn walrus_file_info_mime_matches_the_tracked_mime() {
        let bytes = single_forward_container("f0", "notes.txt", b"hi", &[]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        assert_eq!(source.file_info(&source.files()[0]).mime, "text/plain");
    }

    #[test]
    fn walrus_file_info_for_an_unknown_ref_is_zeroed() {
        let bytes = single_forward_container("f0", "notes.txt", b"hi", &[]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let foreign = FileRef::new(99, "ghost.txt");
        let info = source.file_info(&foreign);
        assert_eq!(info.index_size, 0);
        assert_eq!(info.size_bytes, 0);
        assert_eq!(info.log_bytes, 0);
        assert_eq!(info.generations, None);
        assert!(info.mime.is_empty());
    }

    #[test]
    fn parse_iso_epoch_accepts_the_fixed_shape_and_rejects_others() {
        assert_eq!(parse_iso_epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso_epoch("2026-10-08T12:00:00Z"), Some(1_791_460_800));
        assert_eq!(parse_iso_epoch(""), None);
        assert_eq!(parse_iso_epoch("2026-10-08 12:00:00Z"), None);
        assert_eq!(parse_iso_epoch("2026-13-08T12:00:00Z"), None);
    }

    // === history (FR-K9) ================================================

    use super::super::SpliceSummary;

    #[test]
    fn history_lists_one_entry_per_splice_in_index_order() {
        // base + 2 splices -> entries for index 0 and 1 (NOT index 2 = original).
        let records = [
            ChangeRecord::splice(3, 0, "s"),
            ChangeRecord::splice(0, 0, "the "),
        ];
        let bytes = single_forward_container("f0", "notes.txt", b"cat", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let history = source.history(&source.files()[0]);
        let indices: Vec<usize> = history.iter().map(|e| e.index).collect();
        assert_eq!(indices, vec![0, 1]);
    }

    #[test]
    fn history_entry_splice_summary_matches_the_record_op() {
        // One splice splice(2, 1, "ab") -> index 0 carries @2 -1 +2 (byte len).
        let records = [ChangeRecord::splice(2, 1, "ab")];
        let bytes = single_forward_container("f0", "doc.txt", b"hiya", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let history = source.history(&source.files()[0]);
        assert_eq!(
            history[0].splice,
            SpliceSummary {
                at: 2,
                deleted_len: 1,
                inserted_len: 2,
            }
        );
    }

    #[test]
    fn history_entry_timestamp_comes_from_the_record_ts() {
        let mut with_ts = ChangeRecord::splice(0, 0, "x");
        with_ts.ts = Some("1970-01-01T00:00:00Z".to_string());
        let bytes = single_forward_container("f0", "doc.txt", b"", &[with_ts]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        assert_eq!(source.history(&source.files()[0])[0].timestamp, Some(0));

        // A record with no ts yields None.
        let no_ts = ChangeRecord::splice(0, 0, "x");
        let bytes = single_forward_container("f0", "doc.txt", b"", &[no_ts]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        assert_eq!(source.history(&source.files()[0])[0].timestamp, None);
    }

    #[test]
    fn history_entry_note_is_surfaced_when_present() {
        let mut with_note = ChangeRecord::splice(0, 0, "x");
        with_note.note = Some("tidy up".to_string());
        let bytes = single_forward_container("f0", "doc.txt", b"", &[with_note]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        assert_eq!(
            source.history(&source.files()[0])[0].note,
            Some("tidy up".to_string())
        );

        let no_note = ChangeRecord::splice(0, 0, "x");
        let bytes = single_forward_container("f0", "doc.txt", b"", &[no_note]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        assert_eq!(source.history(&source.files()[0])[0].note, None);
    }

    #[test]
    fn history_skips_checkpoint_records() {
        // A checkpoint among splices does not add an entry: 2 splices -> 2
        // entries regardless of the interleaved checkpoint line.
        let records = [
            ChangeRecord::splice(0, 0, "a"),
            ChangeRecord {
                op: walrus_fmt::Op::Checkpoint {
                    index: 1,
                    sha256: "0".repeat(64),
                },
                ts: None,
                note: None,
            },
            ChangeRecord::splice(1, 0, "b"),
        ];
        let bytes = single_forward_container("f0", "doc.txt", b"", &records);
        let (_dir, source) = source_from(&bytes, "x.wal");
        assert_eq!(source.history(&source.files()[0]).len(), 2);
    }

    #[test]
    fn history_for_an_unknown_ref_is_empty() {
        let bytes = single_forward_container("f0", "doc.txt", b"hi", &[]);
        let (_dir, source) = source_from(&bytes, "x.wal");
        let foreign = FileRef::new(99, "ghost.txt");
        assert!(source.history(&foreign).is_empty());
    }
}
