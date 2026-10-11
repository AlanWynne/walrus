//! `convert` behaviour over fixtures (FR-W11 AC4, CLI-DESIGN 5.7).

mod common;

use common::{code_of, run_walrus, Fixture};
use walrus_fmt::{Archive, Direction, FileId, Variant, Version};

/// Reconstruct every index of a tracked file in an on-disk archive.
fn all_indices(archive: &std::path::Path, name: &str) -> Vec<Vec<u8>> {
    let a = Archive::open(archive).expect("open");
    let tracked = a.file(name).expect("tracked");
    let id = FileId::from(tracked.id.as_str());
    let count = tracked.versions.count;
    (0..=count)
        .map(|k| a.reconstruct(&id, Version::Index(k)).expect("reconstruct"))
        .collect()
}

#[test]
fn convert_whole_archive_flips_variant_and_round_trips_byte_identical() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let before = all_indices(&fx.archive, "notes.txt");

    let result = run_walrus(fx.dir.path(), &["convert", &archive, "--to", "wbl"]);
    assert_eq!(code_of(&result), 0, "whole-archive convert succeeds");

    let a = Archive::open(&fx.archive).expect("open");
    assert_eq!(a.manifest().variant, Variant::Wbl, "variant flipped to wbl");
    let after = all_indices(&fx.archive, "notes.txt");
    assert_eq!(before, after, "every index reconstructs byte-identically");
}

#[test]
fn convert_masked_leaves_variant_and_is_per_file() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(
        fx.dir.path(),
        &["convert", &archive, "notes.txt", "--to", "wbl"],
    );
    assert_eq!(code_of(&result), 0);
    let a = Archive::open(&fx.archive).expect("open");
    assert_eq!(
        a.manifest().variant,
        Variant::Wal,
        "masked convert keeps variant"
    );
    assert_eq!(
        a.file("notes.txt").expect("notes").direction,
        Direction::Reverse,
        "the matched file flipped"
    );
    assert_eq!(
        a.file("src/main.rs").expect("nested").direction,
        Direction::Forward,
        "the unmatched file is untouched"
    );
}

#[test]
fn convert_suggests_rename_but_does_not_touch_disk() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let before_name = fx.archive.clone();
    let result = run_walrus(fx.dir.path(), &["convert", &archive, "--to", "wbl"]);
    assert_eq!(code_of(&result), 0);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("renaming"),
        "a rename is suggested on stderr"
    );
    assert!(before_name.exists(), "the on-disk file is NOT renamed");
}

#[test]
fn convert_output_name_writes_new_file() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let out = fx.dir.path().join("converted.wbl");
    let out_arg = out.to_string_lossy().into_owned();
    let result = run_walrus(
        fx.dir.path(),
        &["convert", &archive, "--to", "wbl", "-o", &out_arg],
    );
    assert_eq!(code_of(&result), 0);
    assert!(out.exists(), "the -o target is written");
    let a = Archive::open(&out).expect("open new");
    assert_eq!(a.manifest().variant, Variant::Wbl);
}

#[test]
fn convert_no_match_is_rc8() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(
        fx.dir.path(),
        &["convert", &archive, "ghost.txt", "--to", "wbl"],
    );
    assert_eq!(
        code_of(&result),
        8,
        "a masked convert matching nothing is RC 8"
    );
}
