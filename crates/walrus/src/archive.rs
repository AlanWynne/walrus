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

use crate::container;
use crate::manifest::{Manifest, TrackedFile};
use crate::record::ChangeRecord;
use crate::wchg::WchgHeader;
use crate::{Result, WalrusError};

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

    /// Resolve a selector (id or name) to the parsed per-file contents.
    fn lookup(&self, sel: &str) -> Option<&ArchiveFile> {
        let tracked = self.file(sel)?;
        self.files.iter().find(|f| f.id == tracked.id)
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
