//! Format-agnostic history source seam (FR-K1).
//!
//! The [`HistorySource`] trait is the single abstraction the transport,
//! player, content renderers, and export consume so they never learn the
//! concrete archive format. Phase 1 ships a `WalrusSource` adapter over
//! `walrus-fmt`; Phase 2 adds an Automerge adapter. Keeping the GUI coded to
//! this trait isolates the format-specific code to one adapter per backend.
//!
//! NOTE: this module MUST NOT reference `walrus_fmt` types in its signatures.
//! The one place allowed to touch `walrus_fmt` is `source/walrus.rs`
//! (`WalrusSource`), which is a later, separate task. Nothing here imports it.

use anyhow::Result;

// The one adapter allowed to touch `walrus_fmt` (FR-K2). Re-exported so callers
// use `source::WalrusSource` without learning the module layout. This module's
// own signatures stay format-agnostic; only `walrus.rs` names archive types.
mod walrus;
pub use walrus::WalrusSource;

// The Automerge adapter (FR-K11 FEAT-003). It is the ONE module allowed to name
// `automerge` types; it enters the build ONLY behind the `automerge` feature so
// the lean Phase-1 build is the default and `walrus-fmt`/`walx` never see it
// (INV-2). The Rust module is name-aliased `automerge_src` via `#[path]` so it
// does not collide with the `automerge` CRATE name, while the file stays
// `source/automerge.rs`.
#[cfg(feature = "automerge")]
#[path = "automerge.rs"]
mod automerge_src;
#[cfg(feature = "automerge")]
pub use automerge_src::AutomergeSource;

// === magic-byte sniff ===================================================

// The sniff helper and its magic-byte constants live here (NOT in the
// feature-gated automerge adapter) because the open path must classify a file
// in BOTH feature states: a feature-OFF build still needs to RECOGNISE an
// Automerge file so it can report that support is not built in, rather than
// mis-opening it as a walrus container. None of these items name an `automerge`
// type, so they are dependency-free (INV-2) and compile with the feature off.

/// The leading bytes that identify an Automerge document container.
pub const AUTOMERGE_MAGIC: [u8; 4] = [0x85, 0x6F, 0x4A, 0x83];
/// The leading bytes of a zstd frame (a Walrus container codec).
pub const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
/// The leading bytes of a gzip member (a Walrus container codec).
pub const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

/// Which backend a file's leading bytes identify.
///
/// The file extension is a hint only; the magic bytes decide (FR-K11.1,
/// FORMAT.md section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// An Automerge document (leading [`AUTOMERGE_MAGIC`]).
    Automerge,
    /// A Walrus `.wal`/`.wbl` container (zstd/gzip, or anything else so the
    /// walrus reader can surface a precise error).
    Walrus,
}

/// Classify a file by its leading bytes.
///
/// Returns [`Backend::Automerge`] only for the exact [`AUTOMERGE_MAGIC`] prefix;
/// zstd/gzip prefixes, unrecognised bytes, and inputs shorter than four bytes
/// all fall through to [`Backend::Walrus`] (which errors cleanly if the file is
/// not actually a Walrus container).
pub fn sniff_backend(head: &[u8]) -> Backend {
    if head.starts_with(&AUTOMERGE_MAGIC) {
        Backend::Automerge
    } else {
        Backend::Walrus
    }
}

/// A plain walker-side Automerge change-hash: the 32-byte hash rendered as a
/// lowercase hex string.
///
/// It carries NO `automerge` type, so `source/mod.rs` and every GUI consumer
/// stay free of the `automerge` dependency (INV-2) and compile with the feature
/// off. The automerge adapter is the only code that converts between this and
/// `automerge::ChangeHash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeadId(String);

impl HeadId {
    /// Build a `HeadId` from its lowercase-hex rendering.
    pub fn new(hex: impl Into<String>) -> Self {
        Self(hex.into())
    }

    /// The lowercase-hex rendering of the change hash.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identifies a specific version of a tracked file.
///
/// Index semantics (locked format decision): index 0 is the CURRENT version
/// and a HIGHER index is an OLDER version. Index 0 is NOT the original base.
/// Stepping "back in time" INCREASES the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionId {
    /// A linear version index where 0 is CURRENT and higher is OLDER.
    Index(usize),
    /// A set of Automerge heads identifying a point in the change DAG.
    ///
    /// Produced ONLY by `AutomergeSource`; walrus/in-memory sources never
    /// produce it. The vec is the frontier (usually one head, more at a merge).
    Heads(Vec<HeadId>),
}

/// Metadata describing a single version of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionMeta {
    /// Human-readable label for the version (e.g. a checkpoint name).
    pub label: String,
    /// Unix timestamp (seconds) when the version was recorded, if known.
    pub timestamp: Option<i64>,
    /// Author/actor who recorded the version, if known.
    pub author: Option<String>,
}

/// Per-file facts the navigator lists and FR-K7 colours, read through the seam.
///
/// This is a plain walker-side value: it carries NO `walrus_fmt` types, so the
/// FR-K1 AC2 boundary (no archive types outside `source/walrus.rs`) holds. It
/// bundles everything both the FR-K4 listing (index size, generation cap,
/// current size) and the FR-K7 bloat ratio (`R = log_bytes / size_bytes`) need,
/// so both compute from the trait alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    /// Number of versions, equal to `versions(f).len()`.
    pub index_size: usize,
    /// Generation (retention) cap; `None` means unlimited.
    pub generations: Option<u32>,
    /// Current-version byte length (display value + FR-K7 bloat denominator).
    pub size_bytes: u64,
    /// Total change-log byte size (FR-K7 bloat numerator).
    pub log_bytes: u64,
    /// The file's MIME type.
    pub mime: String,
}

/// A one-record summary of a splice in a file's change log (FR-K9).
///
/// Plain walker-side value carrying NO `walrus_fmt` types, so the FR-K1 AC2
/// boundary (no archive types outside `source/walrus.rs`) holds. All lengths are
/// byte counts: `at` is the byte offset the splice applied at, `deleted_len` the
/// bytes removed there, `inserted_len` the bytes inserted after deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpliceSummary {
    /// Byte offset the splice applied at.
    pub at: usize,
    /// Number of bytes removed at `at`.
    pub deleted_len: usize,
    /// Number of bytes inserted at `at` after deletion.
    pub inserted_len: usize,
}

/// One change-log entry surfaced to the FR-K9 history viewer.
///
/// A plain walker-side value (NO `walrus_fmt` types), produced by
/// [`HistorySource::history`]. Entries are listed in version-index order (0 =
/// current, higher = older) to match the viewer's listing; checkpoints are not
/// entries (only splices mutate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// The version index this record PRODUCED (0 = current, higher = older),
    /// matching [`VersionId::Index`] semantics and the viewer's listing order.
    pub index: usize,
    /// Epoch-seconds timestamp parsed from the record's `ts`, when present.
    pub timestamp: Option<i64>,
    /// The record's free-form note, when present.
    pub note: Option<String>,
    /// The splice at/deleted_len/inserted_len summary.
    pub splice: SpliceSummary,
}

/// Identifies a tracked file within an archive.
///
/// A `FileRef` carries the minimal data needed to look a file up in its source
/// and to feed [`HistorySource::content_hint`]: a stable id plus the file's
/// name. Fields are private; use [`FileRef::new`] and the accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    id: usize,
    name: String,
}

impl FileRef {
    /// Build a `FileRef` from its stable id and name.
    pub fn new(id: usize, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
        }
    }

    /// The stable id used to look this file up in its source.
    pub fn id(&self) -> usize {
        self.id
    }

    /// The file's name (used for display and content-type sniffing).
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// A format-agnostic view over a document-history archive.
///
/// Implementors adapt a concrete backend (a `.wal`/`.wbl` archive, an Automerge
/// document, ...) to this trait. The GUI depends only on the trait, so a new
/// backend is one new implementor and nothing else changes.
pub trait HistorySource {
    /// List every tracked file in the archive.
    fn files(&self) -> Vec<FileRef>;

    /// List the versions of `f` in index order (0 = current, higher = older).
    fn versions(&self, f: &FileRef) -> Vec<VersionId>;

    /// Reconstruct the bytes of file `f` at version `v`.
    ///
    /// # Errors
    /// Returns an error if `f` is not a tracked file, `v` is out of range, or
    /// the backend fails to reconstruct the requested version.
    fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>>;

    /// The version addressing value for the linear position `index` (0 =
    /// current, higher = older) of file `f`.
    ///
    /// Walrus/in-memory sources return `VersionId::Index(index)`;
    /// `AutomergeSource` returns the `VersionId::Heads` for that linear point.
    /// The default returns `Index(index)` so existing sources need no change.
    /// `versions(f)` is used only to compute the far index at open; the
    /// per-frame version ALWAYS comes from this method.
    fn version_at(&self, _f: &FileRef, index: usize) -> VersionId {
        VersionId::Index(index)
    }

    /// A short structured-view note for the status bar, or `None` when the
    /// source has nothing extra to label.
    ///
    /// `AutomergeSource` overrides this to flag a structured (JSON) view;
    /// walrus/in-memory sources inherit the `None` default.
    fn status_note(&self, _f: &FileRef) -> Option<String> {
        None
    }

    /// Metadata (label/timestamp/author) for file `f` at version `v`.
    fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta;

    /// A `(mime, name)` hint for picking a renderer/exporter for file `f`.
    fn content_hint(&self, f: &FileRef) -> (String, String);

    /// Per-file facts for the navigator listing and the FR-K7 bloat ratio.
    ///
    /// Returns index size, generation cap, current size, the change-log byte
    /// figure, and the MIME. A stale or foreign `f` yields a zeroed `FileInfo`
    /// with an empty mime rather than an error (listings must not panic on a
    /// bad ref).
    fn file_info(&self, f: &FileRef) -> FileInfo;

    /// The change-log history of `f` for the FR-K9 history viewer.
    ///
    /// Entries are returned in VERSION-INDEX order (index 0 = current FIRST,
    /// then increasing index = older), matching the order `versions` lists and
    /// the viewer renders. Only mutating splices become entries; checkpoint
    /// records (which produce no version step) are SKIPPED. Each entry carries
    /// the index it produced, the record's timestamp and note when present
    /// (`None` otherwise), and a [`SpliceSummary`]. A stale or foreign `f`
    /// yields an EMPTY `Vec` rather than an error (the viewer must not panic on
    /// a bad ref).
    fn history(&self, f: &FileRef) -> Vec<HistoryEntry>;
}

#[cfg(any(test, feature = "testsupport"))]
pub mod testsupport;

// === Tests ==========================================================
#[cfg(test)]
mod tests {
    use super::*;

    /// A test-only in-memory source. It needs no `walrus-fmt`: it holds canned
    /// files, each with ordered versions (index 0 = current) carrying bytes and
    /// metadata, plus a mime hint. This is the FR-K1 AC1 fake source.
    struct FakeSource {
        files: Vec<FakeFile>,
    }

    struct FakeFile {
        reference: FileRef,
        mime: String,
        /// Versions in index order: slot 0 is the CURRENT version.
        versions: Vec<FakeVersion>,
    }

    struct FakeVersion {
        bytes: Vec<u8>,
        meta: VersionMeta,
    }

    impl FakeSource {
        fn sample() -> Self {
            let readme = FakeFile {
                reference: FileRef::new(0, "README.md"),
                mime: "text/markdown".to_string(),
                versions: vec![
                    FakeVersion {
                        bytes: b"current readme".to_vec(),
                        meta: VersionMeta {
                            label: "head".to_string(),
                            timestamp: Some(300),
                            author: Some("alice".to_string()),
                        },
                    },
                    FakeVersion {
                        bytes: b"older readme".to_vec(),
                        meta: VersionMeta {
                            label: "v1".to_string(),
                            timestamp: Some(100),
                            author: None,
                        },
                    },
                ],
            };
            let data = FakeFile {
                reference: FileRef::new(1, "data.csv"),
                mime: "text/csv".to_string(),
                versions: vec![FakeVersion {
                    bytes: b"a,b,c".to_vec(),
                    meta: VersionMeta {
                        label: "head".to_string(),
                        timestamp: None,
                        author: Some("bob".to_string()),
                    },
                }],
            };
            Self {
                files: vec![readme, data],
            }
        }

        fn lookup(&self, f: &FileRef) -> Option<&FakeFile> {
            self.files.iter().find(|file| &file.reference == f)
        }
    }

    impl HistorySource for FakeSource {
        fn files(&self) -> Vec<FileRef> {
            self.files.iter().map(|f| f.reference.clone()).collect()
        }

        fn versions(&self, f: &FileRef) -> Vec<VersionId> {
            match self.lookup(f) {
                Some(file) => (0..file.versions.len()).map(VersionId::Index).collect(),
                None => Vec::new(),
            }
        }

        fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>> {
            let file = self
                .lookup(f)
                .ok_or_else(|| anyhow::anyhow!("no such file: {}", f.name()))?;
            let index = match v {
                VersionId::Index(index) => *index,
                // The fake source never emits `Heads`; defensive arm only.
                VersionId::Heads(_) => {
                    return Err(anyhow::anyhow!("FakeSource has no Automerge heads"));
                }
            };
            file.versions
                .get(index)
                .map(|version| version.bytes.clone())
                .ok_or_else(|| anyhow::anyhow!("version index {} out of range", index))
        }

        fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta {
            let empty = || VersionMeta {
                label: String::new(),
                timestamp: None,
                author: None,
            };
            let index = match v {
                VersionId::Index(index) => *index,
                // The fake source never emits `Heads`; defensive arm only.
                VersionId::Heads(_) => return empty(),
            };
            self.lookup(f)
                .and_then(|file| file.versions.get(index))
                .map(|version| version.meta.clone())
                .unwrap_or_else(empty)
        }

        fn content_hint(&self, f: &FileRef) -> (String, String) {
            match self.lookup(f) {
                Some(file) => (file.mime.clone(), file.reference.name().to_string()),
                None => (String::new(), String::new()),
            }
        }

        fn file_info(&self, f: &FileRef) -> FileInfo {
            match self.lookup(f) {
                Some(file) => FileInfo {
                    index_size: file.versions.len(),
                    generations: None,
                    size_bytes: file
                        .versions
                        .first()
                        .map(|v| v.bytes.len() as u64)
                        .unwrap_or(0),
                    log_bytes: file.versions.iter().map(|v| v.bytes.len() as u64).sum(),
                    mime: file.mime.clone(),
                },
                None => FileInfo {
                    index_size: 0,
                    generations: None,
                    size_bytes: 0,
                    log_bytes: 0,
                    mime: String::new(),
                },
            }
        }

        // Same deterministic synthesis rule as `InMemorySource::history`: one
        // entry per non-current version slot, timestamp/note carried from the
        // canned `VersionMeta` when present (FakeSource DOES hold metadata), the
        // splice summary derived from adjacent byte lengths.
        fn history(&self, f: &FileRef) -> Vec<HistoryEntry> {
            let Some(file) = self.lookup(f) else {
                return Vec::new();
            };
            let versions = &file.versions;
            (0..versions.len().saturating_sub(1))
                .map(|index| HistoryEntry {
                    index,
                    timestamp: versions[index].meta.timestamp,
                    note: None,
                    splice: SpliceSummary {
                        at: 0,
                        deleted_len: versions[index + 1].bytes.len(),
                        inserted_len: versions[index].bytes.len(),
                    },
                })
                .collect()
        }
    }

    // === magic-byte sniff (FR-K11) ======================================

    #[test]
    fn sniff_backend_classifies_the_automerge_magic_as_automerge() {
        let head = [0x85, 0x6F, 0x4A, 0x83, 0x00, 0x11];
        assert_eq!(sniff_backend(&head), Backend::Automerge);
    }

    #[test]
    fn sniff_backend_classifies_zstd_and_gzip_prefixes_as_walrus() {
        assert_eq!(sniff_backend(&[0x28, 0xB5, 0x2F, 0xFD]), Backend::Walrus);
        assert_eq!(sniff_backend(&[0x1F, 0x8B, 0x08, 0x00]), Backend::Walrus);
    }

    #[test]
    fn sniff_backend_classifies_junk_and_short_input_as_walrus() {
        assert_eq!(sniff_backend(&[0xDE, 0xAD, 0xBE, 0xEF]), Backend::Walrus);
        assert_eq!(sniff_backend(&[0x85, 0x6F]), Backend::Walrus);
        assert_eq!(sniff_backend(&[]), Backend::Walrus);
    }

    // === VersionId::Heads + HeadId (FR-K11 FEAT-001) ====================

    #[test]
    fn head_id_round_trips_its_hex_via_new_and_as_str() {
        let head = HeadId::new("ab12cd");
        assert_eq!(head.as_str(), "ab12cd");
    }

    #[test]
    fn version_heads_is_not_equal_to_version_index() {
        let heads = VersionId::Heads(vec![HeadId::new("ab12cd")]);
        assert_ne!(heads, VersionId::Index(0));
    }

    #[test]
    fn version_at_defaults_to_the_matching_index() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let readme = source.file_named("README.md");
        assert_eq!(source.version_at(&readme, 1), VersionId::Index(1));
    }

    #[test]
    fn status_note_defaults_to_none() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let readme = source.file_named("README.md");
        assert_eq!(source.status_note(&readme), None);
    }

    #[test]
    fn files_lists_every_tracked_file_in_order() {
        let source = FakeSource::sample();
        let files = source.files();
        let names: Vec<&str> = files.iter().map(FileRef::name).collect();
        assert_eq!(names, vec!["README.md", "data.csv"]);
    }

    #[test]
    fn versions_returns_indices_with_zero_as_current() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        let versions = source.versions(readme);
        assert_eq!(
            versions,
            vec![VersionId::Index(0), VersionId::Index(1)],
            "versions must be in index order with 0 as current"
        );
    }

    #[test]
    fn reconstruct_at_index_zero_returns_current_bytes() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        let bytes = source
            .reconstruct(readme, &VersionId::Index(0))
            .expect("index 0 is the current version and must reconstruct");
        assert_eq!(bytes, b"current readme");
    }

    #[test]
    fn reconstruct_at_higher_index_returns_older_bytes() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        let bytes = source
            .reconstruct(readme, &VersionId::Index(1))
            .expect("index 1 is an older version and must reconstruct");
        assert_eq!(bytes, b"older readme");
    }

    #[test]
    fn reconstruct_out_of_range_index_is_an_error() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        let result = source.reconstruct(readme, &VersionId::Index(99));
        assert!(result.is_err(), "an out-of-range index must error");
    }

    #[test]
    fn metadata_returns_label_timestamp_and_author_for_version() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        let meta = source.metadata(readme, &VersionId::Index(1));
        assert_eq!(
            meta,
            VersionMeta {
                label: "v1".to_string(),
                timestamp: Some(100),
                author: None,
            }
        );
    }

    #[test]
    fn content_hint_returns_mime_and_name() {
        let source = FakeSource::sample();
        let data = &source.files()[1];
        let (mime, name) = source.content_hint(data);
        assert_eq!(mime, "text/csv");
        assert_eq!(name, "data.csv");
    }

    // === file_info (FR-K4, Option A) ====================================

    #[test]
    fn file_info_reports_index_size_as_version_count() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        assert_eq!(
            source.file_info(readme).index_size,
            source.versions(readme).len(),
            "index size must equal the number of versions"
        );
        assert_eq!(source.file_info(readme).index_size, 2);
    }

    #[test]
    fn file_info_reports_generations_none_for_in_memory_sources() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        assert_eq!(source.file_info(readme).generations, None);
    }

    #[test]
    fn file_info_size_bytes_is_the_current_version_length() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        // Slot 0 is the current version: b"current readme".
        assert_eq!(
            source.file_info(readme).size_bytes,
            "current readme".len() as u64
        );
    }

    #[test]
    fn file_info_log_bytes_sums_version_byte_lengths_for_in_memory_sources() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        let expected = ("current readme".len() + "older readme".len()) as u64;
        assert_eq!(source.file_info(readme).log_bytes, expected);
    }

    #[test]
    fn file_info_mime_matches_the_content_hint() {
        let source = FakeSource::sample();
        let data = &source.files()[1];
        assert_eq!(source.file_info(data).mime, "text/csv");
    }

    #[test]
    fn file_info_for_an_unknown_ref_is_zeroed() {
        let source = FakeSource::sample();
        let foreign = FileRef::new(99, "ghost.txt");
        let info = source.file_info(&foreign);
        assert_eq!(
            info,
            FileInfo {
                index_size: 0,
                generations: None,
                size_bytes: 0,
                log_bytes: 0,
                mime: String::new(),
            }
        );
    }

    // === InMemorySource file_info =======================================

    #[test]
    fn in_memory_file_info_size_bytes_is_the_current_version_length() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let readme = source.file_named("README.md");
        // Slot 0 current: b"# Current\n\ncurrent readme".
        assert_eq!(
            source.file_info(&readme).size_bytes,
            "# Current\n\ncurrent readme".len() as u64
        );
    }

    #[test]
    fn in_memory_file_info_log_bytes_sums_version_byte_lengths() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let readme = source.file_named("README.md");
        let expected =
            ("# Current\n\ncurrent readme".len() + "# Older\n\nolder readme".len()) as u64;
        assert_eq!(source.file_info(&readme).log_bytes, expected);
    }

    #[test]
    fn in_memory_file_info_mime_matches_the_content_hint() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let data = source.file_named("data.csv");
        assert_eq!(source.file_info(&data).mime, "text/csv");
        assert_eq!(source.file_info(&data).generations, None);
    }

    // === history (FR-K9) ================================================

    #[test]
    fn fake_history_synthesizes_one_entry_per_non_current_version_in_index_order() {
        let source = FakeSource::sample();
        let readme = &source.files()[0]; // two versions -> exactly one entry.
        let history = source.history(readme);
        let indices: Vec<usize> = history.iter().map(|e| e.index).collect();
        assert_eq!(
            indices,
            vec![0],
            "one entry at index 0 for a 2-version file"
        );
    }

    #[test]
    fn fake_history_splice_summary_is_derived_from_adjacent_version_lengths() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        // Slot 0 "current readme" (14), slot 1 "older readme" (12).
        let history = source.history(readme);
        assert_eq!(
            history[0].splice,
            SpliceSummary {
                at: 0,
                deleted_len: "older readme".len(),
                inserted_len: "current readme".len(),
            }
        );
    }

    #[test]
    fn fake_history_carries_the_producing_versions_timestamp() {
        let source = FakeSource::sample();
        let readme = &source.files()[0];
        // Slot 0's canned metadata carries timestamp Some(300).
        let history = source.history(readme);
        assert_eq!(history[0].timestamp, Some(300));
        assert_eq!(history[0].note, None);
    }

    #[test]
    fn fake_history_for_a_single_version_file_is_empty() {
        let source = FakeSource::sample();
        let data = &source.files()[1]; // one version -> no entries.
        assert!(source.history(data).is_empty());
    }

    #[test]
    fn fake_history_for_an_unknown_ref_is_empty() {
        let source = FakeSource::sample();
        let foreign = FileRef::new(99, "ghost.txt");
        assert!(source.history(&foreign).is_empty());
    }

    #[test]
    fn in_memory_history_synthesizes_one_entry_per_non_current_version() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let readme = source.file_named("README.md"); // two versions -> one entry.
        let history = source.history(&readme);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].index, 0);
        assert_eq!(history[0].timestamp, None);
        assert_eq!(history[0].note, None);
        assert_eq!(
            history[0].splice,
            SpliceSummary {
                at: 0,
                deleted_len: "# Older\n\nolder readme".len(),
                inserted_len: "# Current\n\ncurrent readme".len(),
            }
        );
    }

    #[test]
    fn in_memory_history_for_a_single_version_file_is_empty() {
        use super::testsupport::InMemorySource;
        let source = InMemorySource::sample();
        let data = source.file_named("data.csv"); // one version.
        assert!(source.history(&data).is_empty());
    }
}
