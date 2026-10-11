//! Shim file management for the `walrus alias` helper (CLI-DESIGN section 4).
//!
//! A shim is a single file in a bin directory that forwards to the real
//! `walrus` binary. On POSIX it is a symlink to the binary; on Windows it is a
//! small `.cmd` launcher carrying a marker line. The helper manages ONLY this
//! one file -- it never edits a shell rc file or the registry.
//!
//! Every function here takes its inputs explicitly (the bin dir, the alias
//! name, the target binary path) so the command layer and its tests can drive
//! them against a `TempDir` without reading or mutating the real environment.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The marker embedded in a Windows `.cmd` shim identifying it as ours.
const WINDOWS_MARKER: &str = "WALRUS_ALIAS_SHIM";

/// Whether a `wal` entry on PATH is our own shim, a foreign binary, or absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShimKind {
    /// A shim we created (safe to re-point or remove).
    Ours,
    /// A `wal` we did not create (never shadow or delete it).
    Foreign,
}

/// The on-disk path of the shim for `name` inside `dir` (the `.cmd` suffix is
/// added on Windows).
pub fn shim_path(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        dir.join(format!("{name}.cmd"))
    } else {
        dir.join(name)
    }
}

/// Classify an existing `wal` file at `path` as ours or foreign.
///
/// On POSIX ours is a symlink whose target basename is `walrus`; on Windows
/// ours is a file containing the [`WINDOWS_MARKER`] line. Anything else at the
/// path is treated as foreign and must never be touched.
pub fn classify(path: &Path) -> ShimKind {
    #[cfg(windows)]
    {
        match fs::read_to_string(path) {
            Ok(text) if text.contains(WINDOWS_MARKER) => ShimKind::Ours,
            _ => ShimKind::Foreign,
        }
    }
    #[cfg(not(windows))]
    {
        match fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => match fs::read_link(path) {
                Ok(target) if is_walrus_target(&target) => ShimKind::Ours,
                _ => ShimKind::Foreign,
            },
            _ => ShimKind::Foreign,
        }
    }
}

/// True when a symlink target points at the real `walrus` binary.
#[cfg(not(windows))]
fn is_walrus_target(target: &Path) -> bool {
    target
        .file_name()
        .map(|n| n == std::ffi::OsStr::new("walrus"))
        .unwrap_or(false)
}

/// Create (or re-point) our shim at `path` forwarding to `walrus_bin`.
///
/// If a shim already exists it is replaced idempotently. On POSIX this is a
/// symlink; on Windows a `.cmd` launcher with the ownership marker.
///
/// # Errors
/// Returns an I/O error if the shim cannot be written or linked.
pub fn create(path: &Path, walrus_bin: &Path) -> io::Result<()> {
    // Replace any prior shim so a re-point is idempotent (never duplicates).
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        fs::remove_file(path)?;
    }
    write_shim(path, walrus_bin)
}

/// Platform shim writer: a symlink on POSIX, a marked `.cmd` on Windows.
#[cfg(windows)]
fn write_shim(path: &Path, walrus_bin: &Path) -> io::Result<()> {
    let body = format!(
        "@rem {marker}\r\n@\"{bin}\" %*\r\n",
        marker = WINDOWS_MARKER,
        bin = walrus_bin.display()
    );
    fs::write(path, body)
}

/// Platform shim writer: a symlink on POSIX, a marked `.cmd` on Windows.
#[cfg(not(windows))]
fn write_shim(path: &Path, walrus_bin: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(walrus_bin, path)
}

/// Delete the shim at `path` only if it is ours; returns whether it was deleted.
///
/// A foreign `wal` is never touched. A missing path is a no-op.
///
/// # Errors
/// Returns an I/O error if our shim exists but cannot be deleted.
pub fn remove_if_ours(path: &Path) -> io::Result<bool> {
    if !exists(path) {
        return Ok(false);
    }
    if classify(path) == ShimKind::Ours {
        fs::remove_file(path)?;
        return Ok(true);
    }
    Ok(false)
}

/// True when a file (or symlink) exists at `path`, following the symlink-aware
/// check POSIX needs for dangling links.
pub fn exists(path: &Path) -> bool {
    path.exists() || fs::symlink_metadata(path).is_ok()
}

// === tests ===========================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn shim_path_adds_cmd_suffix_only_on_windows() {
        let dir = TempDir::new().expect("temp dir");
        let path = shim_path(dir.path(), "wal");
        let name = path.file_name().expect("file name").to_string_lossy();
        if cfg!(windows) {
            assert_eq!(name, "wal.cmd", "Windows shims carry the .cmd suffix");
        } else {
            assert_eq!(name, "wal", "POSIX shims are the bare alias name");
        }
    }

    #[test]
    fn a_shim_we_create_classifies_as_ours() {
        let dir = TempDir::new().expect("temp dir");
        let shim = shim_path(dir.path(), "wal");
        let fake_bin = dir.path().join("walrus");
        fs::write(&fake_bin, b"binary").expect("write fake walrus");

        create(&shim, &fake_bin).expect("create shim");
        assert_eq!(classify(&shim), ShimKind::Ours, "our own shim is Ours");
    }

    #[test]
    fn a_plain_file_classifies_as_foreign() {
        let dir = TempDir::new().expect("temp dir");
        let foreign = shim_path(dir.path(), "wal");
        fs::write(&foreign, b"#!/bin/sh\necho pywal\n").expect("write foreign");
        assert_eq!(
            classify(&foreign),
            ShimKind::Foreign,
            "a file we did not create is Foreign"
        );
    }

    #[test]
    fn remove_if_ours_deletes_ours_and_spares_foreign() {
        let dir = TempDir::new().expect("temp dir");
        let fake_bin = dir.path().join("walrus");
        fs::write(&fake_bin, b"binary").expect("write fake walrus");

        let ours = shim_path(dir.path(), "wal");
        create(&ours, &fake_bin).expect("create shim");
        assert!(
            remove_if_ours(&ours).expect("remove ours"),
            "ours is deleted"
        );
        assert!(!exists(&ours), "the shim is gone");

        let foreign = shim_path(dir.path(), "other");
        fs::write(&foreign, b"not ours").expect("write foreign");
        assert!(
            !remove_if_ours(&foreign).expect("spare foreign"),
            "a foreign file is reported as not-deleted"
        );
        assert!(exists(&foreign), "the foreign file is left in place");
    }

    #[test]
    fn create_repoints_idempotently_without_duplicating() {
        let dir = TempDir::new().expect("temp dir");
        let fake_bin = dir.path().join("walrus");
        fs::write(&fake_bin, b"binary").expect("write fake walrus");
        let shim = shim_path(dir.path(), "wal");

        create(&shim, &fake_bin).expect("first create");
        create(&shim, &fake_bin).expect("re-point");
        assert_eq!(classify(&shim), ShimKind::Ours, "still ours after re-point");

        let shim_name = shim.file_name().expect("shim name").to_owned();
        let shim_copies = fs::read_dir(dir.path())
            .expect("read dir")
            .filter(|e| {
                e.as_ref()
                    .map(|e| e.file_name() == shim_name)
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(shim_copies, 1, "no duplicate shim after a re-point");
    }
}
