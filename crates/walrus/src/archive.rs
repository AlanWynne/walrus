//! The read-only multi-file [`Archive`] model (FORMAT.md section 2.1).
//!
//! An archive is read from its compressed container bytes into a [`Manifest`]
//! plus, per tracked file, its base bytes, parsed [`WchgHeader`], and the
//! ordered [`ChangeRecord`] list. This type is READ-ONLY: reconstruct, version
//! resolution, mutate, write, and convert are deliberately NOT implemented here
//! (they are FR-W3/W4/W5/W7). The manifest `files` array is the authoritative
//! directory of tracked files; the tar member set is never globbed for the
//! directory.

use std::collections::BTreeMap;
use std::path::Path;

use crate::manifest::{Manifest, TrackedFile};
use crate::record::ChangeRecord;
use crate::version::Version;
use crate::wchg::WchgHeader;
use crate::{container, reconstruct, version, Result, WalrusError};

// === FileId =============================================================

/// A stable, archive-unique identifier for a tracked file. It mirrors the
/// `files/<id>/` directory and the manifest `id` field; it is a filesystem-safe
/// slug, NOT the original filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileId(String);

impl FileId {
    /// Borrow the id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for FileId {
    fn from(value: &str) -> Self {
        FileId(value.to_string())
    }
}

impl From<String> for FileId {
    fn from(value: String) -> Self {
        FileId(value)
    }
}

// === Archive ============================================================

/// One tracked file's parsed container contents, matched to its manifest entry
/// by `id`. Private: callers reach a file's data through [`Archive`] accessors.
struct ArchiveFile {
    id: String,
    base: Vec<u8>,
    header: WchgHeader,
    records: Vec<ChangeRecord>,
}

/// A read-only, in-memory Walrus archive: the manifest plus every tracked
/// file's base, `.wchg` header, and change records.
///
/// Open with [`Archive::open`] (from a path) or [`Archive::read`] (from the
/// whole compressed container bytes). This type exposes read-only accessors
/// only; mutation, reconstruction, and conversion are later FRs.
#[derive(Debug)]
pub struct Archive {
    manifest: Manifest,
    files: Vec<ArchiveFile>,
}

impl std::fmt::Debug for ArchiveFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveFile")
            .field("id", &self.id)
            .field("base_len", &self.base.len())
            .field("header", &self.header)
            .field("records", &self.records.len())
            .finish()
    }
}

impl Archive {
    /// Read an archive from the whole compressed container bytes: sniff the
    /// outer codec, decompress, untar, parse `manifest.json`, validate it, and
    /// collect each tracked file's base and parsed `.wchg`.
    ///
    /// # Errors
    /// Returns a typed [`WalrusError`] for an unknown codec, a missing or
    /// invalid manifest, a missing per-file `base`/`.wchg` member, or a
    /// malformed `.wchg` stream.
    pub fn read(bytes: &[u8]) -> Result<Archive> {
        let tar_bytes = container::decompress(bytes)?;
        let members = read_members(&tar_bytes)?;

        let manifest_json = members
            .get("manifest.json")
            .ok_or(WalrusError::MissingMember("manifest.json"))?;
        let manifest: Manifest = serde_json::from_slice(manifest_json)?;
        manifest.validate()?;

        let mut files = Vec::with_capacity(manifest.files.len());
        for tracked in &manifest.files {
            files.push(read_tracked_file(tracked, &members)?);
        }
        Ok(Archive { manifest, files })
    }

    /// Open an archive from a path: read the file, then delegate to
    /// [`Archive::read`].
    ///
    /// # Errors
    /// Returns [`WalrusError::Io`] on a read failure, otherwise as
    /// [`Archive::read`].
    pub fn open(path: impl AsRef<Path>) -> Result<Archive> {
        let bytes = std::fs::read(path).map_err(WalrusError::Io)?;
        Self::read(&bytes[..])
    }

    /// The parsed manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Iterate the tracked files in manifest order.
    pub fn files(&self) -> impl Iterator<Item = &TrackedFile> {
        self.manifest.files.iter()
    }

    /// Look up a tracked file by id or by original name.
    pub fn file(&self, sel: &str) -> Option<&TrackedFile> {
        self.manifest
            .files
            .iter()
            .find(|f| f.id == sel || f.name == sel)
    }

    /// The base bytes of the tracked file selected by id or name.
    pub fn base(&self, sel: &str) -> Option<&[u8]> {
        self.lookup(sel).map(|f| f.base.as_slice())
    }

    /// The parsed `.wchg` header of the tracked file selected by id or name.
    pub fn header(&self, sel: &str) -> Option<&WchgHeader> {
        self.lookup(sel).map(|f| &f.header)
    }

    /// The change records of the tracked file selected by id or name.
    pub fn records(&self, sel: &str) -> Option<&[ChangeRecord]> {
        self.lookup(sel).map(|f| f.records.as_slice())
    }

    /// Reconstruct a tracked file's bytes at the requested version.
    ///
    /// The file is selected by [`FileId`] (matched against the manifest `id`).
    /// Reconstruction direction is taken from THAT file's own `.wchg`
    /// `direction`, never the archive variant (FORMAT.md section 5): index 0 is
    /// the current version and the file's splice `count` is the original, for
    /// both directions. The version selector resolves against this file's own
    /// `[0, count]` range.
    ///
    /// # Errors
    /// Returns [`WalrusError::UnknownFile`] when no tracked file has the given
    /// id, [`WalrusError::VersionRange`] for an out-of-range index,
    /// [`WalrusError::UnknownCheckpoint`] for an unknown label, and
    /// [`WalrusError::SpliceOutOfBounds`] if a record is inconsistent with the
    /// base it applies to.
    pub fn reconstruct(&self, file: &FileId, version: Version) -> Result<Vec<u8>> {
        let archive_file = self
            .files
            .iter()
            .find(|f| f.id == file.as_str())
            .ok_or_else(|| WalrusError::UnknownFile {
                id: file.as_str().to_string(),
            })?;
        let tracked = self
            .manifest
            .files
            .iter()
            .find(|t| t.id == archive_file.id)
            .ok_or_else(|| WalrusError::UnknownFile {
                id: archive_file.id.clone(),
            })?;

        // count is the number of SPLICE records (checkpoints do not advance it).
        let count = archive_file
            .records
            .iter()
            .filter(|r| r.is_splice())
            .count();
        let index = version::resolve_file(tracked, &archive_file.records, count, version)?;
        reconstruct::reconstruct_directional(
            &archive_file.base,
            &archive_file.records,
            archive_file.header.direction,
            index,
            count,
        )
    }

    /// Resolve a selector (id or name) to the parsed per-file contents.
    fn lookup(&self, sel: &str) -> Option<&ArchiveFile> {
        let tracked = self.file(sel)?;
        self.files.iter().find(|f| f.id == tracked.id)
    }

    /// Build an [`Archive`] directly from a manifest and per-file parts, for
    /// in-crate tests that must exercise reconstruction without the (not-yet
    /// written) multi-file writer. Not part of the public API.
    #[cfg(test)]
    pub(crate) fn from_parts_for_test(
        manifest: Manifest,
        files: Vec<(String, Vec<u8>, WchgHeader, Vec<ChangeRecord>)>,
    ) -> Archive {
        let files = files
            .into_iter()
            .map(|(id, base, header, records)| ArchiveFile {
                id,
                base,
                header,
                records,
            })
            .collect();
        Archive { manifest, files }
    }
}

// === container collection ==============================================

/// Collect every tar member into a map keyed by member path in a single pass.
/// Iterating the manifest (not this map) remains the authoritative directory;
/// the map just avoids relying on tar member ordering.
fn read_members(tar_bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    use std::io::Read;

    let mut archive = tar::Archive::new(tar_bytes);
    let mut members = BTreeMap::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        members.insert(path, buf);
    }
    Ok(members)
}

/// Collect one tracked file's base and parsed `.wchg` from the member map.
fn read_tracked_file(
    tracked: &TrackedFile,
    members: &BTreeMap<String, Vec<u8>>,
) -> Result<ArchiveFile> {
    let dir = format!("files/{}/", tracked.id);
    let base_path = format!("{dir}base");
    let base = members
        .get(&base_path)
        .ok_or_else(|| WalrusError::BaseMissing {
            id: tracked.id.clone(),
        })?;

    let wchg_bytes = find_wchg_member(&dir, members).ok_or_else(|| WalrusError::WchgMissing {
        id: tracked.id.clone(),
    })?;
    let (header, records) = container::parse_wchg(&tracked.id, wchg_bytes)?;

    Ok(ArchiveFile {
        id: tracked.id.clone(),
        base: base.clone(),
        header,
        records,
    })
}

/// Match the single `*.wchg` member inside a file's `files/<id>/` directory.
/// The manifest is authoritative for the name, so the reader matches by
/// directory + extension rather than reconstructing the exact `<name>.wchg`.
fn find_wchg_member<'a>(dir: &str, members: &'a BTreeMap<String, Vec<u8>>) -> Option<&'a Vec<u8>> {
    members
        .iter()
        .find(|(path, _)| path.starts_with(dir) && path.ends_with(".wchg"))
        .map(|(_, bytes)| bytes)
}

// === FR-W3 reconstruction tests (AC1/AC2/AC3) ==========================
//
// These exercise the PUBLIC `Archive::reconstruct` surface over hand-built
// in-memory fixtures. They live here, rather than in `tests/`, because the
// fixture seam `from_parts_for_test` is deliberately `#[cfg(test)] pub(crate)`
// (an integration-test crate could only reach a fully `pub` constructor, which
// would widen the production API just for tests -- forbidden by the task).
#[cfg(test)]
mod reconstruct_tests {
    use super::*;
    use crate::manifest::{Checkpoint, TrackedFile, Variant};
    use crate::record::ChangeRecord;
    use crate::wchg::Direction;

    /// Assemble a single-file archive from a per-file base + records + direction,
    /// deriving the manifest entry (count = splice count) and `.wchg` header so
    /// the public `reconstruct` path has everything it needs.
    fn single_file_archive(
        id: &str,
        direction: Direction,
        base: &[u8],
        records: Vec<ChangeRecord>,
    ) -> Archive {
        let count = records.iter().filter(|r| r.is_splice()).count();
        let tracked = TrackedFile::new_with_direction(id, "doc.txt", direction, count);
        let header = WchgHeader::new(direction, 0, "doc.txt", "utf-8", count);
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(tracked);
        Archive::from_parts_for_test(
            manifest,
            vec![(id.to_string(), base.to_vec(), header, records)],
        )
    }

    /// Forward fixture: original base "cat"; forward log appends toward the
    /// current "the cats". Splice count = 2 so index 0 = current, 2 = original.
    fn forward_fixture() -> Archive {
        single_file_archive(
            "fwd",
            Direction::Forward,
            b"cat",
            vec![
                ChangeRecord::splice(3, 0, "s"),    // "cat" -> "cats"
                ChangeRecord::splice(0, 0, "the "), // "cats" -> "the cats"
            ],
        )
    }

    /// Reverse fixture for the SAME history: base = current "the cats"; reverse
    /// records are the inverse forward splices applied current -> original.
    fn reverse_fixture() -> Archive {
        single_file_archive(
            "rev",
            Direction::Reverse,
            b"the cats",
            vec![
                ChangeRecord::splice(0, 4, ""), // "the cats" -> "cats"
                ChangeRecord::splice(3, 1, ""), // "cats" -> "cat"
            ],
        )
    }

    // --- AC1: index 0 is current; far end is original (both directions) ---

    #[test]
    fn reconstruct_forward_file_at_index_zero_returns_current_bytes() {
        let a = forward_fixture();
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Index(0))
                .expect("current"),
            b"the cats"
        );
    }

    #[test]
    fn reconstruct_reverse_file_at_index_zero_returns_current_bytes() {
        let a = reverse_fixture();
        assert_eq!(
            a.reconstruct(&FileId::from("rev"), Version::Index(0))
                .expect("current"),
            b"the cats"
        );
    }

    #[test]
    fn reconstruct_forward_file_at_far_index_returns_original_base() {
        let a = forward_fixture();
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Index(2))
                .expect("original"),
            b"cat"
        );
    }

    #[test]
    fn reconstruct_reverse_file_at_far_index_returns_original() {
        let a = reverse_fixture();
        assert_eq!(
            a.reconstruct(&FileId::from("rev"), Version::Index(2))
                .expect("original"),
            b"cat"
        );
    }

    #[test]
    fn reconstruct_in_a_mixed_archive_uses_each_files_own_direction() {
        // One archive (variant Wal) holds a forward AND a reverse file. Index(0)
        // is current for BOTH, proving the archive variant is never consulted --
        // only each file's own header direction.
        let fwd = TrackedFile::new_with_direction("fwd", "f.txt", Direction::Forward, 2);
        let rev = TrackedFile::new_with_direction("rev", "r.txt", Direction::Reverse, 2);
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(fwd);
        manifest.files.push(rev);

        let fwd_header = WchgHeader::new(Direction::Forward, 0, "f.txt", "utf-8", 2);
        let rev_header = WchgHeader::new(Direction::Reverse, 0, "r.txt", "utf-8", 2);
        let a = Archive::from_parts_for_test(
            manifest,
            vec![
                (
                    "fwd".to_string(),
                    b"cat".to_vec(),
                    fwd_header,
                    vec![
                        ChangeRecord::splice(3, 0, "s"),
                        ChangeRecord::splice(0, 0, "the "),
                    ],
                ),
                (
                    "rev".to_string(),
                    b"the cats".to_vec(),
                    rev_header,
                    vec![
                        ChangeRecord::splice(0, 4, ""),
                        ChangeRecord::splice(3, 1, ""),
                    ],
                ),
            ],
        );

        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Index(0))
                .expect("fwd current"),
            b"the cats"
        );
        assert_eq!(
            a.reconstruct(&FileId::from("rev"), Version::Index(0))
                .expect("rev current"),
            b"the cats"
        );
        // And each far end is the original for its own direction.
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Index(2))
                .expect("fwd original"),
            b"cat"
        );
        assert_eq!(
            a.reconstruct(&FileId::from("rev"), Version::Index(2))
                .expect("rev original"),
            b"cat"
        );
    }

    // --- AC2 (INV-3): byte-identical across directions over the WHOLE range ---

    /// Derive the equivalent reverse file from a forward file by inverting each
    /// splice against the version it applies to. For a forward splice
    /// `{at, delete_len, insert}` applied to forward-version `Vp` producing
    /// `Vq`, the inverse splice applied to `Vq` is
    /// `{at, delete_len: insert.len(), insert: <the bytes deleted from Vp>}`.
    /// We walk the forward history from the original forward, capturing the
    /// deleted bytes, then reverse the collected inverses so they apply
    /// current -> original (the reverse log order).
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
                // Bytes about to be deleted from the current version `Vp`.
                let deleted = version[*at..*at + *delete_len].to_vec();
                let deleted_str = String::from_utf8(deleted).expect("utf-8 deleted bytes");
                // The inverse of this forward step, applied to `Vq`.
                inverses.push(ChangeRecord::splice(*at, insert.len(), deleted_str));
                // Advance `version` by applying the forward splice to reach `Vq`.
                let mut next = version.clone();
                next.splice(*at..*at + *delete_len, insert.bytes());
                version = next;
            }
        }
        // `version` is now the current (fully forward-applied) bytes = reverse
        // base. The reverse log applies current -> original, i.e. the inverses
        // in REVERSE order (undo the last forward step first).
        inverses.reverse();
        (version, inverses)
    }

    #[test]
    fn forward_and_reverse_reconstruct_identically_for_every_index() {
        // A richer history than the "cat" fixture: insertions and a deletion so
        // the inversion logic is genuinely exercised at several offsets.
        let original = b"abcdef".to_vec();
        let forward = vec![
            ChangeRecord::splice(6, 0, "GH"),  // "abcdef" -> "abcdefGH"
            ChangeRecord::splice(0, 2, "XY"),  // "abcdefGH" -> "XYcdefGH"
            ChangeRecord::splice(4, 1, "ZZZ"), // "XYcdefGH" -> "XYcdZZZfGH"
        ];
        let count = forward.len();

        let fwd = single_file_archive("fwd", Direction::Forward, &original, forward.clone());
        let (current, reverse) = invert_forward_to_reverse(&original, &forward);
        let rev = single_file_archive("rev", Direction::Reverse, &current, reverse);

        // THOROUGH LOOP: every index must match byte-for-byte across directions.
        for k in 0..=count {
            let f = fwd
                .reconstruct(&FileId::from("fwd"), Version::Index(k))
                .expect("forward reconstruct");
            let r = rev
                .reconstruct(&FileId::from("rev"), Version::Index(k))
                .expect("reverse reconstruct");
            assert_eq!(
                f, r,
                "forward and reverse reconstruct differ at index {k}: {f:?} vs {r:?}"
            );
        }
    }

    #[test]
    fn every_index_round_trips_current_and_original_endpoints_match() {
        let original = b"abcdef".to_vec();
        let forward = vec![
            ChangeRecord::splice(6, 0, "GH"),
            ChangeRecord::splice(0, 2, "XY"),
            ChangeRecord::splice(4, 1, "ZZZ"),
        ];
        let count = forward.len();
        let fwd = single_file_archive("fwd", Direction::Forward, &original, forward.clone());
        let (current, reverse) = invert_forward_to_reverse(&original, &forward);
        let rev = single_file_archive("rev", Direction::Reverse, &current, reverse);

        // Current endpoint (index 0) matches across directions.
        assert_eq!(
            fwd.reconstruct(&FileId::from("fwd"), Version::Index(0))
                .expect("fwd current"),
            rev.reconstruct(&FileId::from("rev"), Version::Index(0))
                .expect("rev current"),
        );
        // Original endpoint (index count) matches across directions.
        assert_eq!(
            fwd.reconstruct(&FileId::from("fwd"), Version::Index(count))
                .expect("fwd original"),
            rev.reconstruct(&FileId::from("rev"), Version::Index(count))
                .expect("rev original"),
        );
    }

    // --- AC3: selectors resolve per-file and clamp; typed errors ---

    #[test]
    fn current_selector_resolves_to_index_zero_for_both_directions() {
        let fwd = forward_fixture();
        let rev = reverse_fixture();
        assert_eq!(
            fwd.reconstruct(&FileId::from("fwd"), Version::Current)
                .expect("fwd current"),
            b"the cats"
        );
        assert_eq!(
            rev.reconstruct(&FileId::from("rev"), Version::Current)
                .expect("rev current"),
            b"the cats"
        );
    }

    #[test]
    fn original_selector_resolves_to_count() {
        let fwd = forward_fixture();
        let rev = reverse_fixture();
        assert_eq!(
            fwd.reconstruct(&FileId::from("fwd"), Version::Original)
                .expect("fwd original"),
            b"cat"
        );
        assert_eq!(
            rev.reconstruct(&FileId::from("rev"), Version::Original)
                .expect("rev original"),
            b"cat"
        );
    }

    #[test]
    fn index_selector_out_of_range_is_a_typed_version_range_error() {
        let a = forward_fixture(); // count = 2
        let err = a
            .reconstruct(&FileId::from("fwd"), Version::Index(3))
            .expect_err("index past count must error");
        assert!(matches!(
            err,
            WalrusError::VersionRange {
                requested: 3,
                max: 2
            }
        ));
    }

    #[test]
    fn relative_zero_is_current_and_positive_moves_older() {
        let a = forward_fixture(); // count = 2, current "the cats", original "cat"
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Relative(0))
                .expect("relative 0 = current"),
            b"the cats"
        );
        // Relative(count) == Original.
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Relative(2))
                .expect("relative count = original"),
            b"cat"
        );
        // A mid positive value lands at the older index 1 ("cats").
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Relative(1))
                .expect("relative mid"),
            b"cats"
        );
    }

    #[test]
    fn relative_clamps_at_both_ends() {
        let a = forward_fixture(); // count = 2
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Relative(99))
                .expect("over clamps to original"),
            b"cat"
        );
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Relative(-5))
                .expect("under clamps to current"),
            b"the cats"
        );
    }

    #[test]
    fn label_resolves_to_its_checkpoint_index_in_the_files_own_space() {
        // Forward file with a checkpoint naming index 1 ("cats").
        let count = 2usize;
        let mut tracked =
            TrackedFile::new_with_direction("fwd", "doc.txt", Direction::Forward, count);
        tracked.checkpoints = vec![Checkpoint {
            index: 1,
            label: "milestone".to_string(),
        }];
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(tracked);
        let header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", count);
        let a = Archive::from_parts_for_test(
            manifest,
            vec![(
                "fwd".to_string(),
                b"cat".to_vec(),
                header,
                vec![
                    ChangeRecord::splice(3, 0, "s"),
                    ChangeRecord::splice(0, 0, "the "),
                ],
            )],
        );
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Label("milestone".into()))
                .expect("label resolves"),
            b"cats"
        );
    }

    #[test]
    fn unknown_label_is_a_typed_unknown_checkpoint_error() {
        let a = forward_fixture();
        let err = a
            .reconstruct(&FileId::from("fwd"), Version::Label("nope".into()))
            .expect_err("unknown label must error");
        assert!(matches!(err, WalrusError::UnknownCheckpoint(_)));
    }

    #[test]
    fn timestamp_before_earliest_record_resolves_to_current_index_zero() {
        // Forward file whose records carry ascending timestamps; a `t` before
        // all of them resolves to index 0 (current) per FORMAT.md section 5.
        let count = 2usize;
        let mut r1 = ChangeRecord::splice(3, 0, "s");
        r1.ts = Some("2026-01-01T00:00:05Z".to_string());
        let mut r2 = ChangeRecord::splice(0, 0, "the ");
        r2.ts = Some("2026-01-01T00:00:09Z".to_string());

        let tracked = TrackedFile::new_with_direction("fwd", "doc.txt", Direction::Forward, count);
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(tracked);
        let header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", count);
        let a = Archive::from_parts_for_test(
            manifest,
            vec![("fwd".to_string(), b"cat".to_vec(), header, vec![r1, r2])],
        );

        let before = crate::timestamp::parse_ts_epoch("2026-01-01T00:00:00Z").expect("parse");
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Timestamp(before))
                .expect("timestamp before earliest = current"),
            b"the cats"
        );
    }

    #[test]
    fn timestamp_matches_the_best_index_at_or_before_it() {
        // Forward file: record 1 (ts ...05Z) takes original "cat" -> "cats"
        // (forward-version 1), record 2 (ts ...09Z) takes "cats" -> "the cats"
        // (forward-version 2 = current). FORMAT.md 4.1: forward-version `i` has
        // canonical index `count - i`. A `t` at/just after record 1's ts (but
        // before record 2's) has `applied = 1` so canonical index = 2 - 1 = 1,
        // which reconstructs "cats".
        let count = 2usize;
        let mut r1 = ChangeRecord::splice(3, 0, "s");
        r1.ts = Some("2026-01-01T00:00:05Z".to_string());
        let mut r2 = ChangeRecord::splice(0, 0, "the ");
        r2.ts = Some("2026-01-01T00:00:09Z".to_string());

        let tracked = TrackedFile::new_with_direction("fwd", "doc.txt", Direction::Forward, count);
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.files.push(tracked);
        let header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", count);
        let a = Archive::from_parts_for_test(
            manifest,
            vec![("fwd".to_string(), b"cat".to_vec(), header, vec![r1, r2])],
        );

        let t = crate::timestamp::parse_ts_epoch("2026-01-01T00:00:07Z").expect("parse");
        assert_eq!(
            a.reconstruct(&FileId::from("fwd"), Version::Timestamp(t))
                .expect("timestamp matches index 1"),
            b"cats"
        );
    }

    #[test]
    fn unknown_file_id_is_a_typed_error() {
        let a = forward_fixture();
        let err = a
            .reconstruct(&FileId::from("absent"), Version::Current)
            .expect_err("absent file id must error");
        assert!(matches!(err, WalrusError::UnknownFile { .. }));
    }
}
