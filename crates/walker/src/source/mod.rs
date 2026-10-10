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

/// Identifies a specific version of a tracked file.
///
/// Index semantics (locked format decision): index 0 is the CURRENT version
/// and a HIGHER index is an OLDER version. Index 0 is NOT the original base.
/// Stepping "back in time" INCREASES the index.
///
/// The enum carries a single variant today. A `Heads(..)` variant is added in
/// Phase 2 for the Automerge DAG; the enum is deliberately open so that
/// addition is churn-free (callers already match on `VersionId`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionId {
    /// A linear version index where 0 is CURRENT and higher is OLDER.
    Index(usize),
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

    /// Metadata (label/timestamp/author) for file `f` at version `v`.
    fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta;

    /// A `(mime, name)` hint for picking a renderer/exporter for file `f`.
    fn content_hint(&self, f: &FileRef) -> (String, String);
}

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
            let VersionId::Index(index) = v;
            file.versions
                .get(*index)
                .map(|version| version.bytes.clone())
                .ok_or_else(|| anyhow::anyhow!("version index {} out of range", index))
        }

        fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta {
            let VersionId::Index(index) = v;
            self.lookup(f)
                .and_then(|file| file.versions.get(*index))
                .map(|version| version.meta.clone())
                .unwrap_or_else(|| VersionMeta {
                    label: String::new(),
                    timestamp: None,
                    author: None,
                })
        }

        fn content_hint(&self, f: &FileRef) -> (String, String) {
            match self.lookup(f) {
                Some(file) => (file.mime.clone(), file.reference.name().to_string()),
                None => (String::new(), String::new()),
            }
        }
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
}
