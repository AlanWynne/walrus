//! Behaviour tests for the full version-selector set, resolved per file (AC4).

mod common;

use common::{code_of, run_walx, Fixture};
use tempfile::TempDir;

/// Read an extracted file's bytes from a path beneath `out`.
fn read_out(out: &std::path::Path, rel: &str) -> Vec<u8> {
    let path = out.join(rel);
    std::fs::read(&path).unwrap_or_else(|_| panic!("read {}", path.display()))
}

#[test]
fn extract_version_original_restores_each_files_oldest_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--version",
            "original",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 0, "original is a clean success");
    // notes.txt original (far index 2) is "cat".
    assert_eq!(read_out(out.path(), "notes.txt"), b"cat");
}

#[test]
fn extract_version_current_and_default_both_restore_index_zero() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();

    let out_default = TempDir::new().expect("out dir");
    let out_default_arg = out_default.path().to_string_lossy().into_owned();
    let r1 = run_walx(
        fx.dir.path(),
        &["extract", &archive, "notes.txt", "-o", &out_default_arg],
    );
    assert_eq!(code_of(&r1), 0);

    let out_current = TempDir::new().expect("out dir");
    let out_current_arg = out_current.path().to_string_lossy().into_owned();
    let r2 = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--version",
            "current",
            "-o",
            &out_current_arg,
        ],
    );
    assert_eq!(code_of(&r2), 0);

    assert_eq!(read_out(out_default.path(), "notes.txt"), b"the cats");
    assert_eq!(
        read_out(out_current.path(), "notes.txt"),
        read_out(out_default.path(), "notes.txt"),
        "default equals --version current"
    );
}

#[test]
fn extract_relative_minus_one_restores_one_older() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    // Relative(+1) is one step older than current -> index 1 "cats".
    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--version",
            "+1",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 0);
    assert_eq!(read_out(out.path(), "notes.txt"), b"cats");
}

/// `--at` with a valid UTC shape falls back to each file's CURRENT version here:
/// the fixture is built via `add`/`update_with_splices`, which store records
/// with `ts: None`, and no public API injects a record timestamp. The library's
/// own `version.rs` tests cover newest-at-or-before SELECTION; this exercises
/// the full walx `--at` path (parse, validate shape, map, reconstruct).
#[test]
fn extract_at_valid_utc_falls_back_to_current_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--at",
            "2026-10-08T12:00:00Z",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 0, "valid --at is a clean success");
    assert_eq!(
        read_out(out.path(), "notes.txt"),
        b"the cats",
        "ts-less records fall back to current"
    );
}

#[test]
fn extract_at_malformed_timestamp_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--at",
            "2026-10-08 12:00:00",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 2, "a malformed --at is a usage error");
}

#[test]
fn extract_two_selectors_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--index",
            "0",
            "--version",
            "original",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 2, "two selectors is a usage error");
}

#[test]
fn extract_label_selector_is_rejected_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--label",
            "milestone",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 2, "--label is deferred and rejected");
}
