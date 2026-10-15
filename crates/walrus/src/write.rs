//! The container writer: serialise a multi-file archive into `zstd(tar{...})`
//! (FORMAT.md section 2.1, FR-W4).
//!
//! This is the inverse of the per-file reader in `container.rs` / `archive.rs`.
//! It emits exactly the members the reader collects:
//!   - `manifest.json` at the archive root (pretty JSON),
//!   - per tracked file `files/<id>/base` (the stored base bytes),
//!   - per tracked file `files/<id>/<name>.wchg` (the JSONL change log: the
//!     header line FIRST, then one record per line, every line LF-terminated).
//!
//! The whole ustar archive is zstd-compressed (the default outer codec, so a
//! write-then-read round-trip matches what `container::sniff_codec` reads back).

use std::io::Write;

use crate::manifest::Manifest;
use crate::record::ChangeRecord;
use crate::wchg::WchgHeader;
use crate::Result;

/// Default zstd compression level. Level 19 favours a small artefact for files
/// that are written once and shipped (e.g. the example fixtures); it is well
/// within zstd's range and still fast for the small documents Walrus stores.
/// It matches the level the FR-W2 reader fixtures compress with, so a
/// round-trip produces the same codec framing.
const ZSTD_LEVEL: i32 = 19;

// === per-file input view ================================================

/// A borrowed view of one tracked file's writable parts: its id (the
/// `files/<id>/` directory), base bytes, parsed `.wchg` header, and change
/// records. The caller ([`crate::Archive`]) owns the data; the writer only
/// borrows it to serialise, so no base/log bytes are cloned in the hot path.
pub(crate) struct FileParts<'a> {
    /// The stable, filesystem-safe id (matches the manifest `id`).
    pub id: &'a str,
    /// The `.wchg` member name (the readable `<name>.wchg` rendering).
    pub wchg_name: &'a str,
    /// The stored base bytes.
    pub base: &'a [u8],
    /// The parsed `.wchg` header, written as the first `.wchg` line.
    pub header: &'a WchgHeader,
    /// The ordered change records, written one per subsequent line.
    pub records: &'a [ChangeRecord],
}

// === public (crate) entry points ========================================

/// Serialise a whole archive to its compressed `zstd(tar{...})` bytes.
///
/// Emits `manifest.json` plus, for every entry in `files`, a `files/<id>/base`
/// and a `files/<id>/<name>.wchg` member, in manifest order.
///
/// # Errors
/// Returns [`crate::WalrusError::Json`] if the manifest, a header, or a record
/// fails to serialise, and [`crate::WalrusError::Io`] on a tar/zstd framing
/// failure.
pub(crate) fn write_archive(manifest: &Manifest, files: &[FileParts<'_>]) -> Result<Vec<u8>> {
    let tar_bytes = build_tar(manifest, files)?;
    zstd::stream::encode_all(&tar_bytes[..], ZSTD_LEVEL).map_err(Into::into)
}

// === tar assembly =======================================================

/// Build the uncompressed ustar archive: `manifest.json` first, then each
/// file's `base` and `.wchg` members in manifest order.
fn build_tar(manifest: &Manifest, files: &[FileParts<'_>]) -> Result<Vec<u8>> {
    let manifest_json = serde_json::to_vec_pretty(manifest)?;

    let mut builder = tar::Builder::new(Vec::new());
    append_member(&mut builder, "manifest.json", &manifest_json)?;
    for file in files {
        let base_path = format!("files/{}/base", file.id);
        append_member(&mut builder, &base_path, file.base)?;

        let wchg_path = format!("files/{}/{}.wchg", file.id, file.wchg_name);
        let wchg_bytes = encode_wchg(file.header, file.records)?;
        append_member(&mut builder, &wchg_path, &wchg_bytes)?;
    }
    let archive = builder.into_inner()?;
    Ok(archive)
}

/// Append a single file member with the given path and contents. The fixed
/// mode/mtime keep the output deterministic (byte-identical for identical
/// inputs regardless of when it is written).
fn append_member(builder: &mut tar::Builder<Vec<u8>>, path: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_ustar();
    header.set_path(path)?;
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder.append(&header, data)?;
    Ok(())
}

/// Encode one file's `.wchg` stream: the header line first, then one record per
/// line, every line terminated by a single LF (FORMAT.md section 4.1). Record
/// content is serialised by serde_json and is never trimmed, so trailing spaces
/// inside an `insert` string survive byte-for-byte.
fn encode_wchg(header: &WchgHeader, records: &[ChangeRecord]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let header_line = serde_json::to_vec(header)?;
    out.write_all(&header_line)?;
    out.push(b'\n');
    for record in records {
        let line = serde_json::to_vec(record)?;
        out.write_all(&line)?;
        out.push(b'\n');
    }
    Ok(out)
}
