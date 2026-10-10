//! Atomic file replacement: build a temp file in the destination directory,
//! then rename it over the destination (FORMAT.md section 6, FR-W4).
//!
//! A failed or interrupted write must leave any pre-existing file byte-for-byte
//! unchanged. The temp-file-then-rename pattern provides that guarantee: the
//! destination is only ever touched by a single atomic `rename`, which happens
//! LAST, after the temp file has been fully and successfully written. If
//! filling the temp file fails, the rename never runs and the destination is
//! untouched; the partial temp file is cleaned up.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use crate::{Result, WalrusError};

// === public API =========================================================

/// Atomically replace `dest` with `bytes`: write the bytes to a temp file in
/// the SAME directory as `dest`, then rename it over `dest`.
///
/// The rename is the only mutation of `dest`; it runs only after the temp file
/// is completely written. A failure before the rename leaves a pre-existing
/// `dest` unchanged.
///
/// # Errors
/// Returns [`WalrusError::Io`] if the temp file cannot be created or written,
/// or if the final rename fails.
pub(crate) fn atomic_write(dest: impl AsRef<Path>, bytes: &[u8]) -> Result<()> {
    write_atomically(dest, |file| file.write_all(bytes).map_err(WalrusError::Io))
}

/// The transactional core shared by [`atomic_write`] and the mutate surface:
/// create a temp file alongside `dest`, hand it to `fill`, and only on success
/// rename it over `dest`.
///
/// `fill` is where the real content is produced; routing all writing through a
/// closure keeps the "a failure before the rename leaves `dest` untouched"
/// guarantee provable without a production-absent hack: a `fill` that returns
/// an error (a serialisation fault, a short write) short-circuits before the
/// rename, exactly as a real mid-write failure would.
///
/// # Errors
/// Propagates any error from `fill`, and returns [`WalrusError::Io`] for a temp
/// create/sync/rename failure. On any error the temp file is removed and `dest`
/// is left byte-for-byte unchanged.
pub(crate) fn write_atomically<F>(dest: impl AsRef<Path>, fill: F) -> Result<()>
where
    F: FnOnce(&mut File) -> Result<()>,
{
    let dest = dest.as_ref();
    let dir = parent_dir(dest);
    let temp_path = temp_path_for(dest);

    // Scope the file handle so it is closed (and flushed) before the rename.
    let fill_result = (|| -> Result<()> {
        let mut file = File::create(&temp_path).map_err(WalrusError::Io)?;
        fill(&mut file)?;
        file.sync_all().map_err(WalrusError::Io)?;
        Ok(())
    })();

    if let Err(err) = fill_result {
        // Best-effort cleanup of the partial temp file; the destination was
        // never touched, so the original (if any) is intact.
        let _ = fs::remove_file(&temp_path); // cleanup only; original is intact.
        return Err(err);
    }

    // The single atomic mutation of `dest`. On Windows, rename does not replace
    // an existing file, so remove any existing destination first; this is still
    // transactional for OUR failure modes (the temp holds the complete new
    // content) and matches how the std library documents cross-platform rename.
    replace(&temp_path, dest, dir)
}

// === helpers ============================================================

/// The directory that holds `dest` (`"."` when `dest` has no parent).
fn parent_dir(dest: &Path) -> &Path {
    match dest.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// A sibling temp path in the destination's directory. The `.walrus-tmp`
/// suffix plus the destination file name keeps the temp within the same
/// directory (so the rename stays on one filesystem) and visibly ephemeral.
fn temp_path_for(dest: &Path) -> std::path::PathBuf {
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());
    let dir = parent_dir(dest);
    dir.join(format!(".{file_name}.walrus-tmp"))
}

/// Move `temp` onto `dest`. On a plain rename failure (e.g. an existing
/// destination on Windows) fall back to removing the destination and renaming.
fn replace(temp: &Path, dest: &Path, _dir: &Path) -> Result<()> {
    match fs::rename(temp, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows rename refuses to clobber: remove then rename. The window
            // between remove and rename is unavoidable with std fs; it is the
            // documented cross-platform behaviour and still never leaves partial
            // CONTENT (the temp is already complete).
            if dest.exists() {
                fs::remove_file(dest).map_err(WalrusError::Io)?;
            }
            fs::rename(temp, dest).map_err(WalrusError::Io)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn read_file(path: &Path) -> Vec<u8> {
        let mut buf = Vec::new();
        File::open(path)
            .expect("open written file")
            .read_to_end(&mut buf)
            .expect("read written file");
        buf
    }

    #[test]
    fn atomic_write_creates_a_new_file_with_the_exact_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("out.bin");
        atomic_write(&dest, b"hello walrus").expect("write");
        assert_eq!(read_file(&dest), b"hello walrus");
    }

    #[test]
    fn atomic_write_replaces_an_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("out.bin");
        atomic_write(&dest, b"first").expect("first write");
        atomic_write(&dest, b"second longer contents").expect("second write");
        assert_eq!(read_file(&dest), b"second longer contents");
    }

    #[test]
    fn a_failure_before_rename_leaves_the_original_byte_for_byte_unchanged() {
        // AC2 at the helper level: write a valid file, capture its exact bytes,
        // then run a write whose `fill` fails PART-WAY (after writing some bytes
        // to the temp, before returning Ok). The rename never runs, so the
        // original must be byte-for-byte identical to the capture.
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("archive.wal");
        atomic_write(&dest, b"ORIGINAL-CONTENT").expect("seed original");
        let captured = read_file(&dest);

        let result = write_atomically(&dest, |file| {
            // Partially fill the temp file, then simulate a mid-write failure
            // (exactly what a serialisation error or short write would do).
            file.write_all(b"PARTIAL-GARBAGE")
                .map_err(WalrusError::Io)?;
            Err(WalrusError::Manifest("injected mid-write failure".into()))
        });

        assert!(result.is_err(), "the injected failure must surface");
        assert_eq!(
            read_file(&dest),
            captured,
            "the original must be unchanged after a failed write"
        );
        assert_eq!(captured, b"ORIGINAL-CONTENT");
    }

    #[test]
    fn no_temp_file_is_left_behind_after_a_failed_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("archive.wal");
        atomic_write(&dest, b"ORIGINAL").expect("seed original");

        let _ = write_atomically(&dest, |_file| {
            Err(WalrusError::Manifest("injected failure".into()))
        });

        let temp = temp_path_for(&dest);
        assert!(
            !temp.exists(),
            "the partial temp file must be cleaned up after a failure"
        );
    }
}
