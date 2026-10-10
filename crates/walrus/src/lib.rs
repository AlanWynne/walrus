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
mod container;
mod error;
mod manifest;
mod reconstruct;
mod record;
mod version;
mod wchg;
mod write;

pub mod export;

pub use archive::{Archive, FileId};
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

    /// Resolve a [`Version`] selector to a concrete index in `0..=len()`.
    pub fn resolve(&self, version: Version) -> Result<usize> {
        version::resolve(&self.manifest, self.len(), version)
    }

    /// Reconstruct the document bytes at the requested version.
    pub fn reconstruct(&self, version: Version) -> Result<Vec<u8>> {
        let index = self.resolve(version)?;
        reconstruct::reconstruct(&self.base, &self.log, index)
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
        assert_eq!(doc.reconstruct(Version::Head).unwrap(), b"cats");
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
        assert_eq!(reopened.reconstruct(Version::Head).unwrap(), b"the cats");
    }
}
