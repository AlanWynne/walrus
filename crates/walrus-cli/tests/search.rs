//! Behaviour tests for `walrus search` over a fixture archive.
//!
//! These drive the built `walrus` binary end to end, asserting on return codes
//! and on the presence of report header fields and table columns, not on exact
//! whitespace. The behaviour must match `walx search` (shared engine).

mod common;

use tempfile::TempDir;
use walrus_fmt::{Archive, Manifest, Variant};

use common::{code_of, run_walrus, Fixture};

/// stdout of a finished process as a lossy UTF-8 string.
fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn search_literal_found_prints_markdown_and_exits_zero() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(fx.dir.path(), &["search", archive, "cats"]);
    assert_eq!(code_of(&output), 0, "a found match exits 0");
    let out = stdout_of(&output);
    assert!(out.contains("Search report"), "{out}");
    assert!(out.contains("fixture.wal"), "archive in header: {out}");
    assert!(out.contains("Version index: 0"), "index in header: {out}");
    assert!(
        out.contains("Version timestamp:"),
        "timestamp in header: {out}"
    );
    assert!(
        out.contains("| Line | Col | Byte offset | Match |"),
        "{out}"
    );
}

#[test]
fn search_no_match_exits_one() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(fx.dir.path(), &["search", archive, "penguin"]);
    assert_eq!(code_of(&output), 1, "a valid zero-hit search exits 1");
}

#[test]
fn search_case_insensitive_flag_matches_mixed_case() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let strict = run_walrus(fx.dir.path(), &["search", archive, "CATS"]);
    assert_eq!(code_of(&strict), 1, "case-sensitive finds nothing");
    let caseless = run_walrus(fx.dir.path(), &["search", archive, "CATS", "-i"]);
    assert_eq!(code_of(&caseless), 0, "case-insensitive finds the match");
}

#[test]
fn search_plain_format_omits_markdown_table_syntax() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(
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
    let output = run_walrus(fx.dir.path(), &["search", archive, "main", "notes.txt"]);
    assert_eq!(
        code_of(&output),
        1,
        "the mask excludes the only file with 'main'"
    );
    let hit = run_walrus(fx.dir.path(), &["search", archive, "main", "src/main.rs"]);
    assert_eq!(code_of(&hit), 0, "the mask includes src/main.rs");
}

#[test]
fn search_two_selectors_is_usage_error_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(
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
    let output = run_walrus(fx.dir.path(), &["search", archive, "cat", "--index", "9"]);
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
    let output = run_walrus(fx.dir.path(), &["search", archive, "ca.s", "--regex"]);
    assert_eq!(code_of(&output), 0, "the pattern 'ca.s' matches 'cats'");
}

#[test]
fn search_all_versions_reports_each_matching_index() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(fx.dir.path(), &["search", archive, "cat", "--all-versions"]);
    assert_eq!(code_of(&output), 0);
    let out = stdout_of(&output);
    assert!(out.contains("- Versions: all"), "{out}");
    for index in 0..=2 {
        assert!(
            out.contains(&format!("Version index: {index}")),
            "missing version {index} in report:\n{out}"
        );
    }
}

#[test]
fn search_all_versions_omits_versions_without_matches() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(fx.dir.path(), &["search", archive, "the", "--all-versions"]);
    assert_eq!(code_of(&output), 0);
    let out = stdout_of(&output);
    assert!(out.contains("Version index: 0"), "{out}");
    assert!(!out.contains("Version index: 1"), "{out}");
    assert!(!out.contains("Version index: 2"), "{out}");
    assert!(out.contains("1 match(es) in 1 file(s)"), "{out}");
}

#[test]
fn search_all_versions_with_no_hits_exits_one() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(
        fx.dir.path(),
        &["search", archive, "penguin", "--all-versions"],
    );
    assert_eq!(code_of(&output), 1);
    assert!(stdout_of(&output).contains("0 match(es) in 0 file(s)"));
}

#[test]
fn search_all_versions_plain_format_prefixes_each_match_with_its_index() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(
        fx.dir.path(),
        &[
            "search",
            archive,
            "cat",
            "--all-versions",
            "--format",
            "plain",
        ],
    );
    assert_eq!(code_of(&output), 0);
    let out = stdout_of(&output);
    assert!(out.contains(" versions all\n"), "{out}");
    for index in 0..=2 {
        assert!(
            out.contains(&format!("notes.txt:{index}:")),
            "plain output omitted version {index}:\n{out}"
        );
    }
    assert!(!out.contains("| Line |"), "{out}");
}

#[test]
fn search_all_versions_respects_the_file_mask() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let hit = run_walrus(
        fx.dir.path(),
        &["search", archive, "cat", "notes.txt", "--all-versions"],
    );
    assert_eq!(code_of(&hit), 0);
    assert!(stdout_of(&hit).contains("notes.txt"));

    let miss = run_walrus(
        fx.dir.path(),
        &["search", archive, "cat", "src/main.rs", "--all-versions"],
    );
    assert_eq!(code_of(&miss), 1);

    let single_version = run_walrus(
        fx.dir.path(),
        &["search", archive, "fn", "src/main.rs", "--all-versions"],
    );
    assert_eq!(code_of(&single_version), 0);
    assert!(stdout_of(&single_version).contains("Version index: 0"));
}

#[test]
fn search_all_versions_reports_each_version_timestamp_for_both_directions() {
    for (file_name, variant) in [("dated.wal", Variant::Wal), ("dated.wbl", Variant::Wbl)] {
        let dir = TempDir::new().expect("temp dir");
        let archive = dir.path().join(file_name);
        let mut fixture = Archive::new(Manifest::new(variant));
        fixture
            .add("dated.txt", b"red", None, None)
            .expect("add base");
        fixture
            .update_at("dated.txt", b"red blue", None, "2026-01-02T00:00:00Z")
            .expect("add first timestamped version");
        fixture
            .update_at("dated.txt", b"green blue", None, "2026-01-03T00:00:00Z")
            .expect("add second timestamped version");
        fixture.write(&archive).expect("write archive");

        let output = run_walrus(
            dir.path(),
            &[
                "search",
                archive.to_str().expect("archive path"),
                "blue",
                "--all-versions",
            ],
        );
        assert_eq!(code_of(&output), 0);
        let out = stdout_of(&output);
        let current = out
            .find("Version index: 0 (timestamp: 2026-01-03T00:00:00Z)")
            .expect("current version timestamp");
        let previous = out
            .find("Version index: 1 (timestamp: 2026-01-02T00:00:00Z)")
            .expect("previous version timestamp");
        assert!(
            current < previous,
            "versions are current-to-original:\n{out}"
        );
        assert!(!out.contains("Version index: 2"), "{out}");
    }
}

#[test]
fn search_all_versions_rejects_a_version_selector() {
    let fx = Fixture::build();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(
        fx.dir.path(),
        &["search", archive, "cat", "--all-versions", "--index", "0"],
    );
    assert_eq!(code_of(&output), 2);
}

#[test]
fn search_all_versions_works_with_reverse_log_archives() {
    let fx = Fixture::build_named("fixture.wbl", walrus_fmt::Variant::Wbl);
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(fx.dir.path(), &["search", archive, "cat", "--all-versions"]);
    assert_eq!(code_of(&output), 0);
    let out = stdout_of(&output);
    for index in 0..=2 {
        assert!(
            out.contains(&format!("Version index: {index}")),
            "missing version {index} in reverse-log report:\n{out}"
        );
    }
}

#[test]
fn search_all_versions_validates_regex_even_without_selected_files() {
    let fx = Fixture::empty();
    let archive = fx.archive.to_str().expect("archive path");
    let output = run_walrus(
        fx.dir.path(),
        &["search", archive, "(", "--regex", "--all-versions"],
    );
    assert_eq!(code_of(&output), 2);
}
