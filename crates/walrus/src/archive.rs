//! The read-only multi-file [`Archive`] model (FORMAT.md section 2.1).
//!
//! An archive is read from its compressed container bytes into a [`Manifest`]
//! plus, per tracked file, its base bytes, parsed [`WchgHeader`], and the
//! ordered [`ChangeRecord`] list. This type is READ-ONLY: reconstruct, version
//! resolution, mutate, write, and convert are deliberately NOT implemented here
//! (they are FR-W3/W4/W5/W7). The manifest `files` array is the authoritative
//! directory of tracked files; the tar member set is never globbed for the
//! directory.

use std::path::Path;
use std::{collections::BTreeMap, fmt};

#[path = "archive_groups.rs"]
mod archive_groups;
#[path = "archive_read.rs"]
mod archive_read;
#[cfg(test)]
use archive_read::bound_tail_bytes;
use archive_read::{read_blobs, read_members, read_tracked_file, wchg_member_name};

use crate::manifest::{Manifest, TrackedFile};
use crate::record::ChangeRecord;
use crate::version::Version;
use crate::wchg::WchgHeader;
use crate::{atomic, container, reconstruct, version, write, Result, WalrusError};

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
/// by `id`. Crate-visible so the sibling `mutate` module can re-base a file's
/// base/records in place; external callers reach a file's data through
/// [`Archive`] accessors only.
pub(crate) struct ArchiveFile {
    pub(crate) id: String,
    pub(crate) base: Vec<u8>,
    pub(crate) header: WchgHeader,
    pub(crate) records: Vec<ChangeRecord>,
}

/// A read-only, in-memory Walrus archive: the manifest plus every tracked
/// file's base, `.wchg` header, and change records.
///
/// Open with [`Archive::open`] (from a path) or [`Archive::read`] (from the
/// whole compressed container bytes). This type exposes read-only accessors
/// only; mutation, reconstruction, and conversion are later FRs.
pub struct Archive {
    manifest: Manifest,
    files: Vec<ArchiveFile>,
    blobs: BTreeMap<String, Vec<u8>>,
}

impl fmt::Debug for Archive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Archive")
            .field("manifest", &self.manifest)
            .field("files", &self.files)
            .field("blob_count", &self.blobs.len())
            .field(
                "blob_bytes",
                &self
                    .blobs
                    .values()
                    .fold(0usize, |total, blob| total.saturating_add(blob.len())),
            )
            .finish()
    }
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
    /// Seed a fresh, empty archive from a prepared manifest (zero tracked
    /// files). Use this when building an archive programmatically and you need
    /// to set archive-level manifest fields -- e.g. an example generator setting
    /// `created`/`generator` -- BEFORE adding files via [`Archive::add`]. The
    /// manifest's own `files` array is ignored here; the archive starts with no
    /// parsed file contents and files are added through the mutate API.
    pub fn new(manifest: Manifest) -> Archive {
        Archive {
            manifest,
            files: Vec::new(),
            blobs: BTreeMap::new(),
        }
    }

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
        let blobs = read_blobs(&members);

        let mut files = Vec::with_capacity(manifest.files.len());
        for tracked in &manifest.files {
            files.push(read_tracked_file(tracked, &members)?);
        }
        Ok(Archive {
            manifest,
            files,
            blobs,
        })
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

    /// Serialise this archive to its compressed `zstd(tar{...})` container
    /// bytes: `manifest.json` plus, per tracked file, `files/<id>/base` and
    /// `files/<id>/<name>.wchg` (header line first), in manifest order.
    ///
    /// This is the in-memory counterpart of [`Archive::write`]; it does no I/O.
    ///
    /// # Errors
    /// Returns [`WalrusError::Json`] on a serialisation fault and
    /// [`WalrusError::Io`] on a tar/zstd framing failure.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        // The `.wchg` member name is a filesystem-safe rendering of the tracked
        // file's authoritative manifest name (FORMAT.md section 2.1). The
        // manifest entry is the source of that name; fall back to the id so a
        // file missing from the manifest still gets a valid, unique member path.
        let wchg_names: Vec<String> = self
            .files
            .iter()
            .map(|f| {
                let name = self
                    .manifest
                    .files
                    .iter()
                    .find(|t| t.id == f.id)
                    .map(|t| t.name.as_str())
                    .unwrap_or(f.id.as_str());
                wchg_member_name(name)
            })
            .collect();
        let parts: Vec<write::FileParts<'_>> = self
            .files
            .iter()
            .zip(wchg_names.iter())
            .map(|(f, wchg_name)| write::FileParts {
                id: &f.id,
                wchg_name: wchg_name.as_str(),
                base: &f.base,
                header: &f.header,
                records: &f.records,
            })
            .collect();
        write::write_archive(&self.manifest, &parts, &self.blobs)
    }

    /// Write this archive to `path` TRANSACTIONALLY: serialise to the full
    /// container bytes, then replace `path` via a temp-file-then-atomic-rename
    /// (FORMAT.md section 6). A failed or interrupted write leaves any
    /// pre-existing archive at `path` byte-for-byte unchanged -- the rename is
    /// the only mutation of `path` and runs last, only after the complete bytes
    /// are staged in a sibling temp file.
    ///
    /// # Errors
    /// Returns [`WalrusError::Json`]/[`WalrusError::Io`] from serialisation (see
    /// [`Archive::to_bytes`]), or [`WalrusError::Io`] if the temp file cannot be
    /// written or the final rename fails. On any error `path` is unchanged.
    pub fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        let bytes = self.to_bytes()?;
        atomic::atomic_write(path, &bytes)
    }

    /// Resolve a selector (id or name) to the parsed per-file contents.
    fn lookup(&self, sel: &str) -> Option<&ArchiveFile> {
        let tracked = self.file(sel)?;
        self.files.iter().find(|f| f.id == tracked.id)
    }

    /// Build an [`Archive`] from a manifest and parsed per-file entries, for the
    /// `convert` mutate path (which returns a NEW archive). Not part of the
    /// public API. The caller guarantees the manifest and files share ids.
    pub(crate) fn from_directories(
        manifest: Manifest,
        files: Vec<ArchiveFile>,
        blobs: BTreeMap<String, Vec<u8>>,
    ) -> Archive {
        Archive {
            manifest,
            files,
            blobs,
        }
    }

    /// Read-only access to the parsed per-file entries, for the sibling
    /// `mutate_convert` module. Not part of the public API.
    pub(crate) fn files_ref(&self) -> &[ArchiveFile] {
        &self.files
    }

    /// Read-only access to content-addressed office payloads for sibling
    /// conversion and extraction modules.
    pub(crate) fn blobs_ref(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.blobs
    }

    /// Mutable access to content-addressed office payloads for the office
    /// ingest module.
    #[cfg(feature = "office")]
    pub(crate) fn blobs_mut(&mut self) -> &mut BTreeMap<String, Vec<u8>> {
        &mut self.blobs
    }

    /// Mutable access to the manifest, for the sibling `mutate` module. Not
    /// part of the public API: re-base/condense update the per-file manifest
    /// entry (versions, checkpoints, rebase audit) through this seam.
    pub(crate) fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// Mutable access to the parsed per-file contents, for the sibling `mutate`
    /// module. Not part of the public API: re-base folds a file's base/records
    /// and header `count` in place through this seam.
    pub(crate) fn files_mut(&mut self) -> &mut [ArchiveFile] {
        &mut self.files
    }

    /// Growable mutable access to BOTH directories at once, for the create
    /// (`add`) and destroy (`remove`) mutate paths. The slice `files_mut`
    /// cannot push or remove a file; handing the manifest and the file vec out
    /// together lets a caller mutate both in lockstep so [`Archive::to_bytes`]
    /// never sees a half-added or half-removed file. Not part of the public API.
    pub(crate) fn directories_mut(&mut self) -> (&mut Manifest, &mut Vec<ArchiveFile>) {
        (&mut self.manifest, &mut self.files)
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
        Archive {
            manifest,
            files,
            blobs: BTreeMap::new(),
        }
    }
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
    /// splice against the version it applies to, via the single production
    /// inversion helper ([`crate::invert::invert_forward_splices`]) so there is
    /// ONE inversion implementation, test and production alike.
    fn invert_forward_to_reverse(
        original: &[u8],
        forward: &[ChangeRecord],
    ) -> (Vec<u8>, Vec<ChangeRecord>) {
        crate::invert::invert_forward_splices(original, forward).expect("invert forward splices")
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

    #[test]
    fn new_seeds_an_empty_archive_whose_manifest_fields_round_trip() {
        // Archive::new seeds a files-free archive from a prepared manifest; its
        // archive-level `created`/`generator` survive a to_bytes/read round-trip.
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.created = Some("2026-10-08T20:30:00Z".to_string());
        manifest.generator = Some("walrus-fmt test".to_string());
        let a = Archive::new(manifest);
        assert_eq!(a.files().count(), 0, "a fresh archive tracks no files");

        let bytes = a.to_bytes().expect("serialise empty archive");
        let reopened = Archive::read(&bytes).expect("read empty archive");
        assert_eq!(
            reopened.manifest().created.as_deref(),
            Some("2026-10-08T20:30:00Z")
        );
        assert_eq!(
            reopened.manifest().generator.as_deref(),
            Some("walrus-fmt test")
        );
    }
}

// === FR-W4 writer round-trip tests (AC1) ===============================
//
// AC1: write-then-read reconstructs every index of every file byte-identically.
// These exercise the PUBLIC `Archive::write` + `Archive::open` surface, but the
// multi-file fixture is assembled through the `#[cfg(test)] pub(crate)`
// `from_parts_for_test` seam -- the same choice the FR-W3 reconstruct tests
// document: an external integration-test crate could only reach a fully `pub`
// constructor, which would widen the production API purely for tests (forbidden
// by the task). So the round-trip tests live in-crate here. `tempfile::TempDir`
// gives each test a private directory that is removed on drop.
#[cfg(test)]
mod write_tests {
    use super::*;
    use crate::manifest::{TrackedFile, Variant};
    use crate::record::ChangeRecord;
    use crate::wchg::{Direction, WchgHeader};

    /// One tracked file's test inputs: `(id, name, direction, base, records)`.
    type FileSpec = (
        &'static str,
        &'static str,
        Direction,
        Vec<u8>,
        Vec<ChangeRecord>,
    );

    /// Build an in-memory [`Archive`] from a list of per-file parts, deriving
    /// each manifest entry (count = splice count) and `.wchg` header from the
    /// direction/name/records so the public write path has everything it needs.
    fn archive_from(variant: Variant, files: &[FileSpec]) -> Archive {
        let mut manifest = Manifest::new(variant);
        let mut parts = Vec::new();
        for (id, name, direction, base, records) in files {
            let count = records.iter().filter(|r| r.is_splice()).count();
            manifest.files.push(TrackedFile::new_with_direction(
                *id, *name, *direction, count,
            ));
            let header = WchgHeader::new(*direction, 0, *name, "utf-8", count);
            parts.push((id.to_string(), base.clone(), header, records.clone()));
        }
        Archive::from_parts_for_test(manifest, parts)
    }

    /// Assert that two archives reconstruct byte-identically for every index of
    /// every tracked file in `0..=count`.
    fn assert_every_index_identical(original: &Archive, reopened: &Archive) {
        let ids: Vec<String> = original.files().map(|t| t.id.clone()).collect();
        for id in &ids {
            let count = original
                .file(id)
                .expect("file present in original")
                .versions
                .count;
            for k in 0..=count {
                let want = original
                    .reconstruct(&FileId::from(id.as_str()), Version::Index(k))
                    .expect("original reconstruct");
                let got = reopened
                    .reconstruct(&FileId::from(id.as_str()), Version::Index(k))
                    .expect("reopened reconstruct");
                assert_eq!(
                    want, got,
                    "file '{id}' index {k} differs after a write/read round-trip"
                );
            }
        }
    }

    fn forward_parts() -> FileSpec {
        (
            "fwd",
            "forward.txt",
            Direction::Forward,
            b"cat".to_vec(),
            vec![
                ChangeRecord::splice(3, 0, "s"),    // "cat" -> "cats"
                ChangeRecord::splice(0, 0, "the "), // "cats" -> "the cats"
            ],
        )
    }

    fn reverse_parts() -> FileSpec {
        (
            "rev",
            "reverse.txt",
            Direction::Reverse,
            b"the cats".to_vec(),
            vec![
                ChangeRecord::splice(0, 4, ""), // "the cats" -> "cats"
                ChangeRecord::splice(3, 1, ""), // "cats" -> "cat"
            ],
        )
    }

    #[test]
    fn write_then_read_round_trips_every_index_of_a_forward_file() {
        let original = archive_from(Variant::Wal, &[forward_parts()]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("fwd.wal");
        original.write(&path).expect("write");
        let reopened = Archive::open(&path).expect("reopen");
        assert_every_index_identical(&original, &reopened);
    }

    #[test]
    fn write_then_read_round_trips_every_index_of_a_reverse_file() {
        let original = archive_from(Variant::Wbl, &[reverse_parts()]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("rev.wbl");
        original.write(&path).expect("write");
        let reopened = Archive::open(&path).expect("reopen");
        assert_every_index_identical(&original, &reopened);
    }

    #[test]
    fn write_then_read_round_trips_a_mixed_direction_archive() {
        // One archive holds both a forward and a reverse file; each must round
        // trip from its OWN header direction (the variant is never consulted).
        let original = archive_from(Variant::Wal, &[forward_parts(), reverse_parts()]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mixed.wal");
        original.write(&path).expect("write");
        let reopened = Archive::open(&path).expect("reopen");
        assert_eq!(reopened.files().count(), 2);
        assert_every_index_identical(&original, &reopened);
    }

    #[test]
    fn write_then_read_round_trips_an_empty_archive() {
        let original = archive_from(Variant::Wal, &[]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.wal");
        original.write(&path).expect("write");
        let reopened = Archive::open(&path).expect("reopen");
        assert_eq!(reopened.files().count(), 0);
        assert_eq!(reopened.manifest().variant, Variant::Wal);
    }

    #[test]
    fn write_then_read_round_trips_content_addressed_blob_members() {
        let mut original = archive_from(Variant::Wal, &[]);
        let hash = "b".repeat(64);
        original
            .blobs
            .insert(hash, b"binary office payload".to_vec());

        let bytes = original.to_bytes().expect("serialize archive with blob");
        let reopened = Archive::read(&bytes).expect("read archive with blob");

        assert_eq!(reopened.blobs, original.blobs);
    }

    #[test]
    fn written_container_is_zstd_magic_and_the_reader_sniffs_it() {
        // INV-5 at the writer boundary: the emitted codec is what the reader
        // sniffs. The first bytes are the zstd magic, and `Archive::read`
        // accepts the in-memory bytes directly.
        let original = archive_from(Variant::Wal, &[forward_parts()]);
        let bytes = original.to_bytes().expect("to_bytes");
        assert_eq!(&bytes[0..4], &[0x28, 0xB5, 0x2F, 0xFD], "zstd magic");
        Archive::read(&bytes).expect("the writer's own output reads back");
    }

    #[test]
    fn wchg_record_content_with_trailing_spaces_survives_round_trip() {
        // FORMAT.md 4.1: trailing spaces inside an insert are DATA and must not
        // be trimmed by the writer or the reader.
        let records = vec![ChangeRecord::splice(0, 0, "hard break   ")];
        let original = archive_from(
            Variant::Wal,
            &[(
                "f0",
                "doc.txt",
                Direction::Forward,
                b"base".to_vec(),
                records,
            )],
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("spaces.wal");
        original.write(&path).expect("write");
        let reopened = Archive::open(&path).expect("reopen");
        // Index 0 is current: the base with the splice applied.
        assert_eq!(
            reopened
                .reconstruct(&FileId::from("f0"), Version::Index(0))
                .expect("reconstruct current"),
            b"hard break   base"
        );
    }

    #[test]
    fn write_is_transactional_a_second_write_replaces_cleanly() {
        // A successful second write over an existing archive leaves a valid,
        // fully-replaced archive (not a mix of old and new bytes).
        let first = archive_from(Variant::Wal, &[forward_parts()]);
        let second = archive_from(Variant::Wbl, &[reverse_parts()]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("replace.wal");
        first.write(&path).expect("first write");
        second.write(&path).expect("second write");
        let reopened = Archive::open(&path).expect("reopen");
        assert_eq!(reopened.manifest().variant, Variant::Wbl);
        assert_every_index_identical(&second, &reopened);
    }
}

// === Change 1: ustar-legal .wchg member name bound =====================
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::{
        Checkpoint, Direction, DocumentGroup, DocumentVersionRow, OfficeKind, TrackedFile, Variant,
    };

    #[test]
    fn wchg_member_name_bounds_a_deep_path_to_ustar_legal_length() {
        // A pathological deep name: the OLD rendering embedded the whole path,
        // overflowing the 100-byte ustar `name` field. The bounded rendering
        // keeps the WHOLE member path `files/f0/<name>.wchg` <= 100 bytes.
        let deep = "Walrus-TestData-Base/crates/walker/gui/target/debug/\
                    build/glutin_wgl_sys/out/wgl_bindings.rs";
        let rendered = wchg_member_name(deep);
        let member = format!("files/f0/{rendered}.wchg");
        assert!(
            member.len() <= 100,
            "member path must be ustar-legal (<= 100 bytes), got {}: {member}",
            member.len()
        );
    }

    #[test]
    fn bound_tail_bytes_keeps_a_utf8_boundary() {
        // A string of two-byte chars truncated where a char would split must
        // return valid UTF-8 and <= max bytes (walking FORWARD to a boundary).
        let s: String = "\u{00E9}".repeat(50); // 100 bytes (50 two-byte chars)
        let out = bound_tail_bytes(&s, 7);
        assert!(
            out.len() <= 7,
            "result must be <= max bytes, got {}",
            out.len()
        );
        // Valid UTF-8 by construction (it is a &str slice); every char is the
        // original two-byte char, so an odd `max` lands on a boundary of 6.
        assert!(out.chars().all(|c| c == '\u{00E9}'), "no split char");
        // Within-max input is returned unchanged.
        assert_eq!(bound_tail_bytes("abc", 10), "abc");
    }

    #[test]
    fn archive_resolves_document_groups_by_id_and_name() {
        let mut manifest = Manifest::new(Variant::Wal);
        manifest.groups.push(DocumentGroup {
            id: "g0".to_string(),
            name: "report.odt".to_string(),
            kind: OfficeKind::Odf,
            member_ids: Vec::new(),
            envelope_id: "f0".to_string(),
            version_map: vec![DocumentVersionRow {
                doc_version: 0,
                envelope_index: 0,
                members: BTreeMap::new(),
            }],
        });
        let archive = Archive::new(manifest);

        assert_eq!(archive.groups().count(), 1);
        assert_eq!(
            archive.group("g0").map(|group| group.name.as_str()),
            Some("report.odt")
        );
        assert_eq!(
            archive.group("report.odt").map(|group| group.id.as_str()),
            Some("g0")
        );
    }

    #[test]
    fn archive_resolves_group_versions_from_envelope_checkpoints_and_timestamps() {
        let mut manifest = Manifest::new(Variant::Wal);
        let mut envelope = TrackedFile::new_with_direction(
            "env",
            "report.odt/envelope.json",
            Direction::Forward,
            2,
        );
        envelope.checkpoints.push(Checkpoint {
            index: 2,
            label: "oldest".to_string(),
        });
        manifest.files.push(envelope);
        manifest.groups.push(DocumentGroup {
            id: "g0".to_string(),
            name: "report.odt".to_string(),
            kind: OfficeKind::Odf,
            member_ids: Vec::new(),
            envelope_id: "env".to_string(),
            version_map: (0..=2)
                .map(|doc_version| DocumentVersionRow {
                    doc_version,
                    envelope_index: doc_version,
                    members: BTreeMap::new(),
                })
                .collect(),
        });

        let mut first = ChangeRecord::splice(0, 0, "v1");
        first.ts = Some("2026-01-01T00:00:05Z".to_string());
        let mut second = ChangeRecord::splice(0, 0, "v2");
        second.ts = Some("2026-01-01T00:00:09Z".to_string());
        let archive = Archive {
            manifest,
            files: vec![ArchiveFile {
                id: "env".to_string(),
                base: Vec::new(),
                header: WchgHeader::new(Direction::Forward, 0, "envelope.json", "utf-8", 2),
                records: vec![first, second],
            }],
            blobs: BTreeMap::new(),
        };

        let timestamp =
            crate::timestamp::parse_ts_epoch("2026-01-01T00:00:06Z").expect("parse timestamp");
        assert_eq!(
            archive
                .resolve_group_version("g0", Version::Label("oldest".to_string()))
                .expect("resolve checkpoint")
                .doc_version,
            2
        );
        assert_eq!(
            archive
                .resolve_group_version("report.odt", Version::Timestamp(timestamp))
                .expect("resolve timestamp")
                .doc_version,
            1
        );
        assert_eq!(
            archive
                .resolve_group_version("g0", Version::Current)
                .expect("resolve current")
                .doc_version,
            0
        );
        assert_eq!(
            archive
                .resolve_group_version("g0", Version::Original)
                .expect("resolve oldest")
                .doc_version,
            2
        );
        assert_eq!(
            archive
                .resolve_group_version("g0", Version::Relative(1))
                .expect("resolve relative")
                .doc_version,
            1
        );
        assert!(matches!(
            archive.resolve_group_version("missing", Version::Current),
            Err(WalrusError::UnknownDocumentGroup { .. })
        ));
    }
}
