//! Deterministic directory recursion for `add` / `update` `-R` (CLI-DESIGN
//! 5.4b/5.4c).
//!
//! A directory argument becomes the recursion root; every regular file beneath
//! it (at any depth) is collected in SORTED relative-path order. Symlinks are
//! not followed and non-regular files (devices, sockets, FIFOs) are skipped
//! with a stderr note (not a failure). Each stored name is
//! `<root-name>/<relative>` with the OS separator normalised to `/`
//! (normalize-on-write).

use std::path::Path;

use anyhow::{Context, Result};

/// A collected source file: its stored archive name, bytes, and source-file
/// mtime UTC stamp.
pub struct Collected {
    /// The stored name (`<root-name>/<relative>`, `/`-separated).
    pub name: String,
    /// The file's bytes.
    pub bytes: Vec<u8>,
    /// The file's last-modified instant as a fixed-shape UTC string.
    pub ts: String,
}

/// Walk `root` (a directory) and collect every regular file beneath it in
/// sorted relative-path order. The stored name of each is the directory's own
/// name followed by the file's path relative to `root`, `/`-separated.
///
/// # Errors
/// Returns an I/O error if the directory cannot be read; a skipped special file
/// or symlink is a stderr note, not an error.
pub fn collect_dir(root: &Path) -> Result<Vec<Collected>> {
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string());

    let mut rel_paths: Vec<Vec<String>> = Vec::new();
    walk(root, &mut Vec::new(), &mut rel_paths)?;
    // Deterministic order: sort by the normalised relative path.
    rel_paths.sort();

    let mut out = Vec::with_capacity(rel_paths.len());
    for segments in rel_paths {
        let rel = segments.join("/");
        let disk_path = segments.iter().fold(root.to_path_buf(), |p, s| p.join(s));
        let bytes = std::fs::read(&disk_path)
            .with_context(|| format!("reading {}", disk_path.display()))?;
        let ts = crate::clock::file_mtime_utc(&disk_path);
        out.push(Collected {
            name: format!("{root_name}/{rel}"),
            bytes,
            ts,
        });
    }
    Ok(out)
}

/// Recursively gather regular-file relative paths (as segment vectors) beneath
/// `dir`, skipping symlinks and non-regular entries with a stderr note.
fn walk(dir: &Path, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("listing directory {}", dir.display()))?;
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", entry.path().display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if file_type.is_symlink() {
            eprintln!("walrus: {}: symlink skipped", entry.path().display());
            continue;
        }
        if file_type.is_dir() {
            prefix.push(name);
            walk(&entry.path(), prefix, out)?;
            prefix.pop();
            continue;
        }
        if !file_type.is_file() {
            eprintln!("walrus: {}: special file skipped", entry.path().display());
            continue;
        }
        let mut segments = prefix.clone();
        segments.push(name);
        out.push(segments);
    }
    Ok(())
}
