//! Portable wildcard expansion of `add` / `update` FILE-INPUT operands
//! (CLI-DESIGN 5.4f).
//!
//! On POSIX the shell pre-expands an unquoted `*.md`; on Windows cmd.exe and
//! PowerShell do NOT, so walrus receives the literal operand. This module makes
//! the two behave identically by expanding a metacharacter operand itself with
//! the portable `glob` crate.
//!
//! Per-operand resolution order (section 5.4f):
//! 1. an EXISTING literal path (file or directory) wins as-is -- no globbing;
//! 2. else a metacharacter operand (`*`, `?`, `[`) is globbed relative to the
//!    current working directory, matches are sorted deterministically, and a
//!    no-match is an error (RC 2 at the command layer);
//! 3. else a literal, non-existent, no-metacharacter operand passes through
//!    unchanged so the existing downstream file-not-found read error is kept.
//!
//! Expansion happens BEFORE any byte is read and before `-R` directory handling,
//! so it changes which operands exist, never how a resolved path is ingested.
//! This is CLI argument handling only: `walrus-fmt` and FORMAT.md are untouched.

use std::fmt;
use std::path::{Path, PathBuf};

/// An error from resolving a single file-input operand.
#[derive(Debug, PartialEq, Eq)]
pub enum InputGlobError {
    /// A metacharacter pattern that matched no path on disk (section 5.4f -> RC 2).
    NoMatch { pattern: String },
    /// The glob pattern itself was malformed.
    Glob { pattern: String, detail: String },
}

impl fmt::Display for InputGlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputGlobError::NoMatch { pattern } => {
                write!(f, "no files match pattern '{pattern}'")
            }
            InputGlobError::Glob { pattern, detail } => {
                write!(f, "invalid pattern '{pattern}': {detail}")
            }
        }
    }
}

impl std::error::Error for InputGlobError {}

/// True if `s` contains a glob metacharacter (`*`, `?`, or `[`), per section
/// 5.4f step 2. These are the characters the `glob` crate treats specially.
fn has_glob_metacharacter(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

/// Resolve ONE file-input operand to a list of concrete paths following the
/// exact section-5.4f order (literal-path-wins -> glob -> literal passthrough).
///
/// # Errors
/// Returns [`InputGlobError::NoMatch`] when a metacharacter pattern matches no
/// path, or [`InputGlobError::Glob`] when the pattern itself is malformed.
pub fn resolve_file_operand(operand: &str) -> Result<Vec<PathBuf>, InputGlobError> {
    // 1. Literal-path-wins: a real file/dir (including one whose name contains a
    //    metacharacter, and the POSIX pre-expanded case) is used verbatim.
    if Path::new(operand).exists() {
        return Ok(vec![PathBuf::from(operand)]);
    }

    // 2. Metacharacter operand: expand with the portable glob, sorted.
    if has_glob_metacharacter(operand) {
        // Normalise `\` to `/` in the PATTERN only, so a Windows `sub\*.md` or a
        // drive-qualified `c:\dir\*.md` reaches the globber in the `/`-form it
        // accepts. The raw operand already lost step 1, so a real path is never
        // rewritten here.
        let normalized = operand.replace('\\', "/");
        let paths = glob::glob(&normalized).map_err(|e| InputGlobError::Glob {
            pattern: operand.to_string(),
            detail: e.to_string(),
        })?;
        let mut matches: Vec<PathBuf> = Vec::new();
        // A read error on a single candidate is skipped (flatten drops Err
        // entries); a truly broken pattern already surfaced above as a
        // PatternError from glob::glob.
        for path in paths.flatten() {
            matches.push(path);
        }
        if matches.is_empty() {
            return Err(InputGlobError::NoMatch {
                pattern: operand.to_string(),
            });
        }
        // Deterministic batch order (section 5.4f DETERMINISM).
        matches.sort();
        return Ok(matches);
    }

    // 3. Literal, non-existent, no metacharacter: unchanged passthrough so the
    //    downstream read produces the familiar file-not-found error.
    Ok(vec![PathBuf::from(operand)])
}

/// Resolve every file-input operand in order and concatenate the results, so a
/// command passes its whole `files` list through one call.
///
/// # Errors
/// Propagates the first [`InputGlobError`] from any operand.
pub fn resolve_file_operands(operands: &[String]) -> Result<Vec<PathBuf>, InputGlobError> {
    let mut out = Vec::new();
    for operand in operands {
        out.extend(resolve_file_operand(operand)?);
    }
    Ok(out)
}

// === tests ==========================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, MutexGuard};

    use tempfile::TempDir;

    // The resolver globs relative to the process CWD; a single lock serialises
    // the few tests that change it so they stay deterministic under a parallel
    // test runner.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    /// Set the process CWD to `dir` for the lifetime of the returned guard,
    /// restoring the previous CWD on drop.
    struct CwdGuard {
        _lock: MutexGuard<'static, ()>,
        previous: PathBuf,
    }

    impl CwdGuard {
        fn enter(dir: &Path) -> CwdGuard {
            let lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let previous = std::env::current_dir().expect("read cwd");
            std::env::set_current_dir(dir).expect("set cwd");
            CwdGuard {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    fn touch(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, body).expect("write fixture");
    }

    #[test]
    fn literal_existing_path_wins_without_globbing() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "plain.md", "body");
        let operand = tmp.path().join("plain.md").to_string_lossy().into_owned();
        let got = resolve_file_operand(&operand).expect("resolve");
        assert_eq!(got, vec![PathBuf::from(operand)]);
    }

    #[test]
    fn glob_star_expands_to_sorted_matches_in_one_directory() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "b.md", "b");
        touch(tmp.path(), "a.md", "a");
        touch(tmp.path(), "c.txt", "c");
        touch(tmp.path(), "sub/d.md", "d");
        let _cwd = CwdGuard::enter(tmp.path());
        let got = resolve_file_operand("*.md").expect("resolve");
        assert_eq!(got, vec![PathBuf::from("a.md"), PathBuf::from("b.md")]);
    }

    #[test]
    fn double_star_recurses() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "top.md", "t");
        touch(tmp.path(), "sub/d.md", "d");
        let _cwd = CwdGuard::enter(tmp.path());
        let mut got = resolve_file_operand("**/*.md").expect("resolve");
        got.sort();
        assert_eq!(
            got,
            vec![PathBuf::from("sub/d.md"), PathBuf::from("top.md")]
        );
    }

    #[test]
    fn no_match_is_error() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "a.md", "a");
        let _cwd = CwdGuard::enter(tmp.path());
        let err = resolve_file_operand("*.zzz").expect_err("should not match");
        assert_eq!(
            err,
            InputGlobError::NoMatch {
                pattern: "*.zzz".to_string()
            }
        );
    }

    #[test]
    fn literal_nonexistent_no_metachar_passes_through() {
        let tmp = TempDir::new().expect("tempdir");
        let _cwd = CwdGuard::enter(tmp.path());
        let got = resolve_file_operand("ghost.md").expect("resolve");
        assert_eq!(got, vec![PathBuf::from("ghost.md")]);
    }

    #[test]
    fn backslash_pattern_is_normalized() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "sub/d.md", "d");
        let _cwd = CwdGuard::enter(tmp.path());
        let got = resolve_file_operand("sub\\*.md").expect("resolve");
        assert_eq!(got, vec![PathBuf::from("sub/d.md")]);
    }

    #[test]
    fn resolve_file_operands_concatenates_in_order() {
        let tmp = TempDir::new().expect("tempdir");
        touch(tmp.path(), "a.md", "a");
        touch(tmp.path(), "keep.txt", "k");
        let _cwd = CwdGuard::enter(tmp.path());
        let got =
            resolve_file_operands(&["*.md".to_string(), "keep.txt".to_string()]).expect("resolve");
        assert_eq!(got, vec![PathBuf::from("a.md"), PathBuf::from("keep.txt")]);
    }
}
