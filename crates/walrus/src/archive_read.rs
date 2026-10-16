//! Container-member collection and per-file read helpers for
//! [`crate::archive::Archive`].
//!
//! Kept separate from `archive.rs` so the archive coordinator stays within the
//! source-file size limit.

use std::collections::BTreeMap;
use std::io::Read;

use crate::archive::ArchiveFile;
use crate::manifest::TrackedFile;
use crate::{container, Result, WalrusError};

/// Collect every tar member into a map keyed by member path in a single pass.
/// Iterating the manifest remains the authoritative directory; the map just
/// avoids relying on tar member ordering.
pub(super) fn read_members(tar_bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
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
pub(super) fn read_tracked_file(
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

/// Max bytes for the advisory `<name>` part of the `files/<id>/<name>.wchg`
/// member. ustar caps the whole `name` field at 100 bytes; reserve headroom for
/// `files/` (6), a short-id budget (16), the `/` (1), and the `.wchg` suffix
/// (5): `100 - 28 = 72`.
const WCHG_NAME_MAX_BYTES: usize = 72;

/// A filesystem-safe `<name>` for the `files/<id>/<name>.wchg` member. Path
/// separators become `_`, and the result is bounded so the whole member path
/// remains ustar-legal even for deep paths.
pub(super) fn wchg_member_name(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' => '_',
            other => other,
        })
        .collect();
    bound_tail_bytes(&slug, WCHG_NAME_MAX_BYTES)
}

/// Return the last `<= max` bytes of `s` on a UTF-8 char boundary.
pub(super) fn bound_tail_bytes(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut start = s.len() - max;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

/// Match the single `*.wchg` member inside a file's `files/<id>/` directory.
fn find_wchg_member<'a>(dir: &str, members: &'a BTreeMap<String, Vec<u8>>) -> Option<&'a Vec<u8>> {
    members
        .iter()
        .find(|(path, _)| path.starts_with(dir) && path.ends_with(".wchg"))
        .map(|(_, bytes)| bytes)
}
