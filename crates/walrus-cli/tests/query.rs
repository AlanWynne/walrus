//! `query` listing behaviour (FR-W11 AC1, CLI-DESIGN 5.1).

mod common;

use common::{code_of, run_walrus, Fixture};

#[test]
fn bare_archive_defaults_to_query_listing_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &[&archive]);
    assert_eq!(code_of(&result), 0, "a bare archive lists (query default)");
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("notes.txt"), "lists the top-level file");
    assert!(stdout.contains("src/main.rs"), "lists the nested file");
}

#[test]
fn query_subcommand_lists_tracked_files_with_index_size_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["query", &archive]);
    assert_eq!(code_of(&result), 0);
    let stdout = String::from_utf8_lossy(&result.stdout);
    // notes.txt has two change records -> index size 2.
    let line = stdout
        .lines()
        .find(|l| l.contains("notes.txt"))
        .expect("notes.txt line");
    assert!(line.contains('2'), "terse line shows the index size (2)");
    // src/main.rs is single-version -> index size 0.
    let nested = stdout
        .lines()
        .find(|l| l.contains("src/main.rs"))
        .expect("nested line");
    assert!(nested.contains('0'), "nested file has index size 0");
}

#[test]
fn query_long_listing_shows_attributes_and_log_bytes() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["query", &archive, "-l"]);
    assert_eq!(code_of(&result), 0);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let line = stdout
        .lines()
        .find(|l| l.contains("notes.txt"))
        .expect("notes.txt long line");
    // The long line carries more columns than the terse form: index size, log
    // bytes, and the generation cap at minimum.
    let columns: Vec<&str> = line.split_whitespace().collect();
    assert!(
        columns.len() >= 4,
        "long listing has multiple columns, got {columns:?}"
    );
}

#[test]
fn query_mask_filters_to_matching_files() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    // A single `*` stays within one segment: selects notes.txt, not src/main.rs.
    let result = run_walrus(fx.dir.path(), &["query", &archive, "*"]);
    assert_eq!(code_of(&result), 0);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(stdout.contains("notes.txt"), "mask selects notes.txt");
    assert!(
        !stdout.contains("src/main.rs"),
        "a single * does not cross a / segment"
    );
}
