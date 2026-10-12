//! `query` listing behaviour (FR-W11 AC1, CLI-DESIGN 5.1).

mod common;

use common::{code_of, run_walrus, Fixture};

use walrus_fmt::{Archive, Manifest, Variant};

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
fn query_long_versions_column_counts_recorded_versions_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["query", &archive, "-l"]);
    assert_eq!(code_of(&result), 0);
    let stdout = String::from_utf8_lossy(&result.stdout);
    // notes.txt has two change records -> Versions column (first field) == 2.
    let notes = stdout
        .lines()
        .find(|l| l.contains("notes.txt"))
        .expect("notes.txt line");
    let notes_versions: usize = notes
        .split_whitespace()
        .next()
        .expect("versions field")
        .parse()
        .expect("versions is numeric");
    assert_eq!(notes_versions, 2, "notes.txt Versions column is 2");
    // src/main.rs is single-version -> Versions == 0.
    let nested = stdout
        .lines()
        .find(|l| l.contains("src/main.rs"))
        .expect("nested line");
    let nested_versions: usize = nested
        .split_whitespace()
        .next()
        .expect("versions field")
        .parse()
        .expect("versions is numeric");
    assert_eq!(nested_versions, 0, "nested file Versions column is 0");
}

#[test]
fn query_long_listing_is_an_aligned_versions_lastwrite_bytes_name_table() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["query", &archive, "-l"]);
    assert_eq!(code_of(&result), 0);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let mut lines = stdout.lines();
    let header = lines.next().expect("header row");
    assert!(header.contains("Versions"), "header has Versions");
    assert!(header.contains("LastWriteTime"), "header has LastWriteTime");
    assert!(header.contains("Bytes"), "header has Bytes");
    assert!(header.contains("Name"), "header has Name");
    // A separator row of dash runs follows the header.
    let separator = lines.next().expect("separator row");
    assert!(
        separator
            .split_whitespace()
            .all(|f| f.chars().all(|c| c == '-')),
        "separator row is runs of '-', got {separator:?}"
    );
    // The notes.txt data row parses to Versions == 2, numeric Bytes, Name
    // `notes.txt`. The fixture carries no stamp, so its LastWriteTime cell is
    // the single `-` placeholder; a stamped LastWriteTime is asserted directly
    // in `query_long_lastwrite_is_the_current_version_on_a_multi_version_wal`.
    let notes = stdout
        .lines()
        .find(|l| l.contains("notes.txt"))
        .expect("notes.txt data row");
    let fields: Vec<&str> = notes.split_whitespace().collect();
    assert_eq!(fields[0].parse::<usize>().expect("versions numeric"), 2);
    // fields[1] = LastWriteTime placeholder, fields[2] = Bytes, fields[3] = Name.
    assert_eq!(fields[1], "-", "unstamped fixture LastWriteTime is '-'");
    assert!(fields[2].parse::<u64>().is_ok(), "Bytes is numeric");
    assert_eq!(fields[3], "notes.txt", "Name field");
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

#[test]
fn query_long_lastwrite_is_the_current_version_on_a_multi_version_wal() {
    // HIGH-1 guard: a forward (.wal) file with two STAMPED saves must show the
    // SECOND (current-version) stamp as LastWriteTime, not the first-edit time.
    let dir = tempfile::tempdir().expect("temp dir");
    let archive_path = dir.path().join("multi.wal");
    let early = "2020-01-01T00:00:00Z";
    let later = "2026-10-11T16:18:44Z";

    let mut a = Archive::new(Manifest::new(Variant::Wal));
    a.add("doc.txt", b"hello", None, None).expect("create");
    // First recorded save stamped EARLY.
    a.add_at("doc.txt", b"hello world", None, None, early)
        .expect("save 1 (oldest)");
    // Second recorded save stamped LATER (this is the current version).
    a.add_at("doc.txt", b"hello world again", None, None, later)
        .expect("save 2 (current)");
    a.write(&archive_path).expect("write archive");

    let archive = archive_path.to_string_lossy().into_owned();
    let result = run_walrus(dir.path(), &["query", &archive, "-l"]);
    assert_eq!(code_of(&result), 0);
    let stdout = String::from_utf8_lossy(&result.stdout);
    let row = stdout
        .lines()
        .find(|l| l.contains("doc.txt"))
        .expect("doc.txt row");
    let fields: Vec<&str> = row.split_whitespace().collect();
    // LastWriteTime is fields[1] (date) + fields[2] (time) == the LATER stamp.
    assert_eq!(fields[1], "2026-10-11", "date is the later save");
    assert_eq!(fields[2], "16:18:44", "time is the later save");
}
