//! stdin ingestion via `-` + `--name` (FR-W11 AC9, CLI-DESIGN 5.4e).

mod common;

use common::{code_of, reconstruct, run_walrus, run_walrus_stdin, Fixture};
use tempfile::TempDir;
use walrus_fmt::Version;

#[test]
fn add_from_stdin_dash_with_name_creates_file() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("s.wal").to_string_lossy().into_owned();
    let result = run_walrus_stdin(
        dir.path(),
        &["add", &archive, "-", "--name", "piped.txt"],
        b"from stdin",
    );
    assert_eq!(code_of(&result), 0, "stdin add creates the file");
    assert_eq!(
        reconstruct(
            std::path::Path::new(&archive),
            "piped.txt",
            Version::Current
        ),
        b"from stdin"
    );
}

#[test]
fn dash_without_name_is_rc2() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("s.wal").to_string_lossy().into_owned();
    let result = run_walrus_stdin(dir.path(), &["add", &archive, "-"], b"x");
    assert_eq!(code_of(&result), 2, "'-' without --name is RC 2");
}

#[test]
fn name_without_dash_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    std::fs::write(fx.dir.path().join("f.txt"), b"x").expect("write");
    let result = run_walrus(
        fx.dir.path(),
        &["add", &archive, "f.txt", "--name", "n.txt"],
    );
    assert_eq!(code_of(&result), 2, "--name without '-' is RC 2");
}

#[test]
fn dash_mixed_with_file_is_rc2() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("s.wal").to_string_lossy().into_owned();
    std::fs::write(dir.path().join("f.txt"), b"x").expect("write");
    let result = run_walrus_stdin(
        dir.path(),
        &["add", &archive, "-", "f.txt", "--name", "n.txt"],
        b"x",
    );
    assert_eq!(code_of(&result), 2, "'-' mixed with files is RC 2");
}

#[test]
fn dash_with_recurse_is_rc2() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("s.wal").to_string_lossy().into_owned();
    let result = run_walrus_stdin(
        dir.path(),
        &["add", &archive, "-", "--name", "n.txt", "-R"],
        b"x",
    );
    assert_eq!(code_of(&result), 2, "'-' with -R is RC 2");
}

#[test]
fn stdin_binary_input_is_rejected_nonzero_and_unchanged() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("s.wal").to_string_lossy().into_owned();
    let result = run_walrus_stdin(
        dir.path(),
        &["add", &archive, "-", "--name", "n.txt"],
        &[0xFF, 0xFE, 0x00],
    );
    assert_ne!(code_of(&result), 0, "binary stdin is rejected");
    assert!(
        !std::path::Path::new(&archive).exists(),
        "a rejected create leaves no archive behind"
    );
}

#[test]
fn update_from_stdin_untracked_is_rc8() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus_stdin(
        fx.dir.path(),
        &["update", &archive, "-", "--name", "ghost.txt"],
        b"boo",
    );
    assert_eq!(
        code_of(&result),
        8,
        "stdin update of an untracked name is RC 8"
    );
}

#[test]
fn stdin_name_verbatim_and_illegal_name_is_rc2() {
    let dir = TempDir::new().expect("tmp");
    let archive = dir.path().join("s.wal").to_string_lossy().into_owned();
    // Verbatim nested name stored as given.
    let ok = run_walrus_stdin(
        dir.path(),
        &["add", &archive, "-", "--name", "a/b/c.txt"],
        b"nested",
    );
    assert_eq!(code_of(&ok), 0);
    assert_eq!(
        reconstruct(
            std::path::Path::new(&archive),
            "a/b/c.txt",
            Version::Current
        ),
        b"nested"
    );
    // An escaping name is refused RC 2.
    let bad = run_walrus_stdin(
        dir.path(),
        &["add", &archive, "-", "--name", "../escape.txt"],
        b"x",
    );
    assert_eq!(code_of(&bad), 2, "an escaping --name is RC 2");
}
