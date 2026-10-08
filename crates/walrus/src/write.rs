//! The container writer: serialise manifest + base + log into zstd(tar).
//!
//! This is the inverse of `container::read_container`. It emits the fixed tar
//! members (`manifest.json`, `base`, `log.jsonl`) in a ustar archive and
//! compresses the whole archive with zstd (the default outer codec, see
//! `FORMAT.md` section 2).

use std::io::Write;

use crate::manifest::Manifest;
use crate::record::ChangeRecord;
use crate::Result;

/// Default zstd compression level. Level 19 favours a small artefact for files
/// that are written once and shipped (e.g. the example fixtures); it is well
/// within zstd's range and still fast for the small documents Walrus stores.
const ZSTD_LEVEL: i32 = 19;

/// Serialise a container to its compressed `zstd(tar{...})` bytes.
///
/// The log is written as `log.jsonl` (one JSON record per line); `manifest` and
/// `base` are written verbatim as their named members.
pub(crate) fn write_container(
    manifest: &Manifest,
    base: &[u8],
    log: &[ChangeRecord],
) -> Result<Vec<u8>> {
    let tar_bytes = build_tar(manifest, base, log)?;
    zstd::stream::encode_all(&tar_bytes[..], ZSTD_LEVEL).map_err(Into::into)
}

/// Build the uncompressed ustar archive with the three fixed members.
fn build_tar(manifest: &Manifest, base: &[u8], log: &[ChangeRecord]) -> Result<Vec<u8>> {
    let manifest_json = serde_json::to_vec_pretty(manifest)?;
    let log_jsonl = encode_log(log)?;

    let mut builder = tar::Builder::new(Vec::new());
    append_member(&mut builder, "manifest.json", &manifest_json)?;
    append_member(&mut builder, "base", base)?;
    append_member(&mut builder, "log.jsonl", &log_jsonl)?;
    let archive = builder.into_inner()?;
    Ok(archive)
}

/// Append a single file member with the given path and contents.
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

/// Encode the change log as newline-delimited JSON (one record per line).
fn encode_log(log: &[ChangeRecord]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for record in log {
        let line = serde_json::to_vec(record)?;
        out.write_all(&line)?;
        out.push(b'\n');
    }
    Ok(out)
}
