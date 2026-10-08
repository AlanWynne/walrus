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

mod container;
mod error;
mod manifest;
mod record;
mod reconstruct;
mod version;

#[cfg(any(
    feature = "export-html",
    feature = "export-md",
    feature = "export-csv",
    feature = "export-json",
    feature = "export-pdf",
    feature = "export-docx",
))]
pub mod export;

pub use error::{Result, WalrusError};
pub use manifest::{Checkpoint, DocumentInfo, Manifest, Variant, Versions};
pub use record::{ChangeRecord, Op};
pub use version::Version;

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

    /// Resolve a [`Version`] selector to a concrete index in `0..=len()`.
    pub fn resolve(&self, version: Version) -> Result<usize> {
        version::resolve(&self.manifest, self.len(), version)
    }

    /// Reconstruct the document bytes at the requested version.
    pub fn reconstruct(&self, version: Version) -> Result<Vec<u8>> {
        let index = self.resolve(version)?;
        reconstruct::reconstruct(&self.base, &self.log, index)
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
        assert_eq!(doc.reconstruct(Version::Head).unwrap(), b"cats");
    }
}
