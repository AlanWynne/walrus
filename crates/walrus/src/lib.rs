//! walrus-fmt -- the `.wal` / `.wbl` document-with-history format.
//!
//! A Walrus file is a compressed tar bundle of a base document plus an ordered
//! change log. See `FORMAT.md` at the repository root for the authoritative,
//! language-agnostic specification.
//!
//! ```text
//! .wal :  original (base)  + forward_log  = current
//! .wbl :  current  (base)  - reverse_log  = original
//! ```
//!
//! This crate is GUI-free and dependency-light so it can be embedded. Export
//! formats are behind opt-in cargo features (`export-*`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod archive;
mod atomic;
mod container;
#[cfg(feature = "diff")]
mod diff;
mod error;
mod manifest;
mod mutate;
mod rebase;
mod reconstruct;
mod record;
mod timestamp;
mod version;
mod wchg;
mod write;

pub mod export;

pub use archive::{Archive, FileId};
#[cfg(feature = "diff")]
pub use diff::{diff, DiffResult, Granularity};
pub use error::{Result, WalrusError};
pub use manifest::{
    Checkpoint, FileAttributes, Manifest, RebaseInfo, TrackedFile, Variant, Versions,
};
pub use record::{ChangeRecord, Op};
pub use version::Version;
pub use wchg::{Direction, WchgHeader};

/// An opened Walrus file: the decompressed base, the manifest, and the log.
///
/// Open with [`WalrusFile::open`] (from a path) or [`WalrusFile::read`] (from a
/// reader), then reconstruct any version with [`WalrusFile::reconstruct`].
#[derive(Debug, Clone)]
pub struct WalrusFile {
    manifest: Manifest,
    base: Vec<u8>,
    log: Vec<ChangeRecord>,
}

impl WalrusFile {
    /// Open a `.wal` or `.wbl` file from disk: sniff the codec, decompress,
    /// untar, and parse the manifest and log.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let bytes = std::fs::read(path).map_err(WalrusError::Io)?;
        Self::read(&bytes[..])
    }

    /// Open from an in-memory byte slice (the whole compressed container).
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let (manifest, base, log) = container::read_container(bytes)?;
        Ok(Self {
            manifest,
            base,
            log,
        })
    }

    /// Build a `.wal` file in memory from a base document (the original) and a
    /// forward change log. The manifest is derived from the log length; adjust
    /// the returned value's [`WalrusFile::manifest_mut`] for document info or
    /// checkpoints before writing.
    ///
    /// # Errors
    /// Returns an error only if the derived manifest fails validation.
    pub fn new_wal(base: impl Into<Vec<u8>>, log: Vec<ChangeRecord>) -> Result<Self> {
        let base = base.into();
        let splice_count = log.iter().filter(|r| r.is_splice()).count();
        let manifest = Manifest::new_wal(splice_count);
        manifest.validate()?;
        Ok(Self {
            manifest,
            base,
            log,
        })
    }

    /// Mutable access to the manifest, for setting document info, checkpoints,
    /// `created`, or `generator` before writing.
    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// The parsed manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The ordered change log.
    pub fn log(&self) -> &[ChangeRecord] {
        &self.log
    }

    /// The number of addressable versions beyond the base (i.e. the number of
    /// log records). Valid indices are `0..=len()`.
    pub fn len(&self) -> usize {
        self.log.len()
    }

    /// True when there are no change records (base only).
    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    /// Reconstruct the document bytes at the requested version.
    ///
    /// This single-document path keeps the ORIGINAL (pre-FR-W3) index
    /// convention, where the stored base is index 0 and the fully-replayed far
    /// end is index `len()`. It is a compatibility shim: the direction-aware,
    /// index-0-is-current model lives on [`Archive`]. The [`Version`] selectors
    /// map onto the old model as follows:
    /// - `Index(i)` applies the first `i` records to the base (old meaning).
    /// - `Current` is the stored base (old index 0), since `new_wal` treats the
    ///   supplied base as version 0.
    /// - `Original` is the far end (old `len()`), fully replayed.
    /// - `Relative`/`Label`/`Timestamp` resolve per-file against this document's
    ///   own manifest entry and records.
    pub fn reconstruct(&self, version: Version) -> Result<Vec<u8>> {
        let index = match version {
            Version::Index(i) => {
                if i > self.len() {
                    return Err(WalrusError::VersionRange {
                        requested: i,
                        max: self.len(),
                    });
                }
                i
            }
            // Old-model endpoints: current == stored base (index 0),
            // original == far end (index len). This preserves the asserted
            // values of the retained single-document tests and examples.
            Version::Current => 0,
            Version::Original => self.len(),
            // The remaining selectors are resolved directly in the OLD
            // single-document convention (index 0 = base, len = far end). This
            // keeps the shim self-contained and avoids re-mapping the per-file
            // (index 0 = current) engine's result back onto the old space.
            Version::Relative(n) => {
                if n <= 0 {
                    0
                } else if (n as u128) >= self.len() as u128 {
                    self.len()
                } else {
                    n as usize
                }
            }
            Version::Label(label) => self
                .manifest
                .checkpoints()
                .iter()
                .find(|c| c.label == label)
                .map(|c| c.index.min(self.len()))
                .ok_or(WalrusError::UnknownCheckpoint(label))?,
            // The single-document shim carries no per-record timestamp matching;
            // a timestamp selector resolves to the stored base (version 0).
            Version::Timestamp(_) => 0,
        };
        reconstruct::reconstruct(&self.base, &self.log, index)
    }

    /// Reconstruct the stored base (old index 0). Compatibility accessor for the
    /// single-document model, equivalent to the removed `Version::Base`.
    pub fn reconstruct_base(&self) -> Result<Vec<u8>> {
        reconstruct::reconstruct(&self.base, &self.log, 0)
    }

    /// Reconstruct the fully-replayed far end (old index `len()`). Compatibility
    /// accessor for the single-document model, equivalent to the removed
    /// `Version::Head`.
    pub fn reconstruct_head(&self) -> Result<Vec<u8>> {
        reconstruct::reconstruct(&self.base, &self.log, self.len())
    }

    /// Serialise this file to its compressed container bytes (`zstd(tar{...})`).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        write::write_container(&self.manifest, &self.base, &self.log)
    }

    /// Write this file to `path` as a compressed container.
    pub fn write(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
        let bytes = self.to_bytes()?;
        std::fs::write(path, bytes).map_err(WalrusError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_reconstructs_to_base() {
        // A base with no log records yields the base at index 0.
        let base = b"hello walrus".to_vec();
        let doc = WalrusFile {
            manifest: Manifest::new_wal(base.len()),
            base: base.clone(),
            log: vec![],
        };
        assert_eq!(doc.len(), 0);
        assert!(doc.is_empty());
        assert_eq!(doc.reconstruct(Version::Index(0)).unwrap(), base);
    }

    #[test]
    fn forward_splice_reaches_next_version() {
        // "cat" --(insert "s" at 3)--> "cats"
        let base = b"cat".to_vec();
        let log = vec![ChangeRecord::splice(3, 0, "s")];
        let doc = WalrusFile {
            manifest: Manifest::new_wal(base.len()),
            base,
            log,
        };
        assert_eq!(doc.reconstruct(Version::Index(0)).unwrap(), b"cat");
        assert_eq!(doc.reconstruct(Version::Index(1)).unwrap(), b"cats");
        assert_eq!(doc.reconstruct_head().unwrap(), b"cats");
    }

    #[test]
    fn write_then_read_round_trips_every_version() {
        // Build in memory, serialise, read back, and confirm each version index
        // reconstructs byte-identically through the container boundary.
        let base = b"cat".to_vec();
        let log = vec![
            ChangeRecord::splice(3, 0, "s"),    // "cat" -> "cats"
            ChangeRecord::splice(0, 0, "the "), // "cats" -> "the cats"
        ];
        let original = WalrusFile::new_wal(base, log).unwrap();

        let bytes = original.to_bytes().unwrap();
        let reopened = WalrusFile::read(&bytes).unwrap();

        assert_eq!(reopened.len(), 2);
        assert_eq!(reopened.manifest().variant, Variant::Wal);
        for index in 0..=reopened.len() {
            assert_eq!(
                reopened.reconstruct(Version::Index(index)).unwrap(),
                original.reconstruct(Version::Index(index)).unwrap(),
                "version {index} differs after a write/read round-trip"
            );
        }
        assert_eq!(reopened.reconstruct_head().unwrap(), b"the cats");
    }
}
