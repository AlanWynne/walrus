//! In-memory history source for tests and headless harnesses.
//!
//! This needs no `walrus-fmt`: it holds canned files, each with ordered
//! versions (index 0 = current) carrying bytes and a mime hint. The integration
//! harness in `tests/` uses it to drive the trait-level byte path without a live
//! archive. The parent module exposes this only under tests or the `testsupport`
//! feature.

use super::{
    FileInfo, FileRef, HistoryEntry, HistorySource, Result, SpliceSummary, VersionId, VersionMeta,
};

/// A single canned file: its reference, a mime hint, and ordered versions.
struct Entry {
    reference: FileRef,
    mime: String,
    /// Versions in index order: slot 0 is the CURRENT version.
    versions: Vec<Vec<u8>>,
}

/// A deterministic, format-agnostic [`HistorySource`] backed by in-memory
/// bytes. Build one with [`InMemorySource::sample`] or
/// [`InMemorySource::single`].
pub struct InMemorySource {
    entries: Vec<Entry>,
}

impl InMemorySource {
    /// A sample source covering the mime types the renderers select on:
    /// markdown, csv, html, json, and plain text. `README.md` carries two
    /// versions (0 = current, 1 = older) so the byte path can be exercised
    /// at more than one index.
    pub fn sample() -> Self {
        Self {
            entries: vec![
                Entry {
                    reference: FileRef::new(0, "README.md"),
                    mime: "text/markdown".to_string(),
                    versions: vec![
                        b"# Current\n\ncurrent readme".to_vec(),
                        b"# Older\n\nolder readme".to_vec(),
                    ],
                },
                Entry {
                    reference: FileRef::new(1, "data.csv"),
                    mime: "text/csv".to_string(),
                    versions: vec![b"a,b,c\n1,2,3".to_vec()],
                },
                Entry {
                    reference: FileRef::new(2, "page.html"),
                    mime: "text/html".to_string(),
                    versions: vec![b"<h1>Title</h1>".to_vec()],
                },
                Entry {
                    reference: FileRef::new(3, "data.json"),
                    mime: "application/json".to_string(),
                    versions: vec![br#"{"k":1}"#.to_vec()],
                },
                Entry {
                    reference: FileRef::new(4, "notes.txt"),
                    mime: "text/plain".to_string(),
                    versions: vec![b"plain notes".to_vec()],
                },
            ],
        }
    }

    /// A source with a single file of the given name, mime, and current
    /// bytes (index 0). Handy for a focused harness test.
    pub fn single(name: &str, mime: &str, bytes: &[u8]) -> Self {
        Self {
            entries: vec![Entry {
                reference: FileRef::new(0, name),
                mime: mime.to_string(),
                versions: vec![bytes.to_vec()],
            }],
        }
    }

    /// Look up the [`FileRef`] of a canned file by name.
    ///
    /// # Panics
    /// Panics if no canned file has that name (a test wiring error).
    pub fn file_named(&self, name: &str) -> FileRef {
        self.entries
            .iter()
            .find(|e| e.reference.name() == name)
            .map(|e| e.reference.clone())
            .expect("test asked for a file name the sample source does not define")
    }

    fn lookup(&self, f: &FileRef) -> Option<&Entry> {
        self.entries.iter().find(|e| &e.reference == f)
    }
}

impl HistorySource for InMemorySource {
    fn files(&self) -> Vec<FileRef> {
        self.entries.iter().map(|e| e.reference.clone()).collect()
    }

    fn versions(&self, f: &FileRef) -> Vec<VersionId> {
        match self.lookup(f) {
            Some(e) => (0..e.versions.len()).map(VersionId::Index).collect(),
            None => Vec::new(),
        }
    }

    fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>> {
        let entry = self
            .lookup(f)
            .ok_or_else(|| anyhow::anyhow!("no such file: {}", f.name()))?;
        let index = match v {
            VersionId::Index(index) => *index,
            // An in-memory source never emits `Heads`; defensive arm only.
            VersionId::Heads(_) => {
                return Err(anyhow::anyhow!("InMemorySource has no Automerge heads"));
            }
        };
        entry
            .versions
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("version index {} out of range", index))
    }

    fn metadata(&self, _f: &FileRef, _v: &VersionId) -> VersionMeta {
        VersionMeta {
            label: String::new(),
            timestamp: None,
            author: None,
        }
    }

    fn content_hint(&self, f: &FileRef) -> (String, String) {
        match self.lookup(f) {
            Some(e) => (e.mime.clone(), e.reference.name().to_string()),
            None => (String::new(), String::new()),
        }
    }

    fn file_info(&self, f: &FileRef) -> FileInfo {
        match self.lookup(f) {
            Some(e) => FileInfo {
                index_size: e.versions.len(),
                generations: None,
                size_bytes: e.versions.first().map(|v| v.len() as u64).unwrap_or(0),
                log_bytes: e.versions.iter().map(|v| v.len() as u64).sum(),
                mime: e.mime.clone(),
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

    // SYNTHESIS RULE (deterministic, documented): the in-memory source has no
    // real change log, so `history()` synthesises one entry per non-current
    // version slot. For a file with N versions it produces N-1 entries, one for
    // each version index 0..N-1 (the final slot N-1 is the oldest, treated as
    // the original base and so produces no entry). Each entry's splice summary
    // is derived from adjacent byte lengths: `at` is 0, `deleted_len` is the
    // older version's length, and `inserted_len` is the current version's
    // length. Timestamp and note are always None (the canned source carries
    // neither). The only contract is determinism and that tests assert the
    // exact vec.
    fn history(&self, f: &FileRef) -> Vec<HistoryEntry> {
        let Some(entry) = self.lookup(f) else {
            return Vec::new();
        };
        let versions = &entry.versions;
        (0..versions.len().saturating_sub(1))
            .map(|index| HistoryEntry {
                index,
                timestamp: None,
                note: None,
                splice: SpliceSummary {
                    at: 0,
                    deleted_len: versions[index + 1].len(),
                    inserted_len: versions[index].len(),
                },
            })
            .collect()
    }
}
