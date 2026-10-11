//! `remove` behaviour over fixtures (FR-W11, CLI-DESIGN 5.5).

mod common;

use common::{code_of, run_walrus, Fixture};
use walrus_fmt::Archive;

#[test]
fn remove_single_match_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["remove", &archive, "notes.txt"]);
    assert_eq!(code_of(&result), 0, "single-match remove succeeds");
    let a = Archive::open(&fx.archive).expect("open");
    assert!(a.file("notes.txt").is_none(), "the file is gone");
    assert!(a.file("src/main.rs").is_some(), "the other file remains");
}

#[test]
fn remove_multi_match_without_force_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    // `**` matches both tracked files.
    let result = run_walrus(fx.dir.path(), &["remove", &archive, "**"]);
    assert_eq!(code_of(&result), 2, "multi-match without --force is RC 2");
    let a = Archive::open(&fx.archive).expect("open");
    assert_eq!(a.files().count(), 2, "nothing removed");
}

#[test]
fn remove_force_removes_all() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["remove", &archive, "**", "--force"]);
    assert_eq!(code_of(&result), 0, "--force removes every match");
    let a = Archive::open(&fx.archive).expect("open");
    assert_eq!(a.files().count(), 0, "all files removed");
}

#[test]
fn remove_no_match_is_rc8() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["remove", &archive, "ghost.txt"]);
    assert_eq!(code_of(&result), 8, "a no-match remove is RC 8");
}
