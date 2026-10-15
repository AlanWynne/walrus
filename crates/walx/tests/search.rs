//! Behaviour tests for `walx search` over a fixture archive.
//!
//! These drive the built binary end to end (reusing the shared `common`
//! fixture), asserting on return codes and on the presence of report header
//! fields and table columns, not on exact whitespace.

mod common;

use common::{code_of, run_walx, Fixture};

/// stdout of a finished process as a lossy UTF-8 string.
fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn search_literal_found_prints_markdown_and_exits_zero() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walx(fx.dir.path(), &["search", archive, "cats"]);
    assert_eq!(code_of(&output), 0, "a found match exits 0");
    let out = stdout_of(&output);
    // Markdown header carries the archive, the searched index, and a timestamp.
    assert!(out.contains("Search report"), "{out}");
    assert!(out.contains("fixture.wal"), "archive in header: {out}");
    assert!(out.contains("Version index: 0"), "index in header: {out}");
    assert!(
        out.contains("Version timestamp:"),
        "timestamp in header: {out}"
    );
    // Per-file table columns.
    assert!(
        out.contains("| Line | Col | Byte offset | Match |"),
        "{out}"
    );
}

#[test]
fn search_no_match_exits_one() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walx(fx.dir.path(), &["search", archive, "penguin"]);
    assert_eq!(code_of(&output), 1, "a valid zero-hit search exits 1");
}

#[test]
fn search_case_insensitive_flag_matches_mixed_case() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    // "CATS" is absent case-sensitively but present with -i.
    let strict = run_walx(fx.dir.path(), &["search", archive, "CATS"]);
    assert_eq!(code_of(&strict), 1, "case-sensitive finds nothing");
    let caseless = run_walx(fx.dir.path(), &["search", archive, "CATS", "-i"]);
    assert_eq!(code_of(&caseless), 0, "case-insensitive finds the match");
}

#[test]
fn search_plain_format_omits_markdown_table_syntax() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walx(
        fx.dir.path(),
        &["search", archive, "cats", "--format", "plain"],
    );
    assert_eq!(code_of(&output), 0);
    let out = stdout_of(&output);
    assert!(
        !out.contains("| Line |"),
        "plain output must omit the Markdown table header: {out}"
    );
    assert!(
        out.contains("notes.txt:"),
        "plain output lists the file: {out}"
    );
}

#[test]
fn search_file_mask_restricts_to_matching_files() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    // "main" lives only in src/main.rs, so a mask of notes.txt finds nothing.
    let output = run_walx(fx.dir.path(), &["search", archive, "main", "notes.txt"]);
    assert_eq!(
        code_of(&output),
        1,
        "the mask excludes the only file with 'main'"
    );
    let hit = run_walx(fx.dir.path(), &["search", archive, "main", "src/main.rs"]);
    assert_eq!(code_of(&hit), 0, "the mask includes src/main.rs");
}

#[test]
fn search_two_selectors_is_usage_error_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walx(
        fx.dir.path(),
        &[
            "search",
            archive,
            "cats",
            "--index",
            "0",
            "--version",
            "original",
        ],
    );
    assert_eq!(code_of(&output), 2, "two selectors is a usage error");
}

#[test]
fn search_index_beyond_last_warns_and_exits_rc4() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    // notes.txt has only indices 0..=2; index 9 clamps to the original + RC 4.
    let output = run_walx(fx.dir.path(), &["search", archive, "cat", "--index", "9"]);
    assert_eq!(
        code_of(&output),
        4,
        "a beyond-last numeric index clamps to RC 4"
    );
}

#[test]
fn search_regex_flag_matches_pattern() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    // The regex feature is enabled for the binary, so --regex works.
    let output = run_walx(fx.dir.path(), &["search", archive, "ca.s", "--regex"]);
    assert_eq!(code_of(&output), 0, "the pattern 'ca.s' matches 'cats'");
}
