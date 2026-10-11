//! Behaviour tests for `walx cat` over fixture archives (AC3, AC2 analogue).

mod common;

use common::{code_of, run_walx, Fixture};

#[test]
fn cat_writes_exactly_one_files_raw_bytes_to_stdout_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = run_walx(fx.dir.path(), &["cat", &archive, "notes.txt"]);
    assert_eq!(code_of(&out), 0, "exactly-one match is RC 0");
    // Default selector is current (index 0): "the cats".
    assert_eq!(out.stdout, b"the cats", "raw current bytes on stdout");
}

#[test]
fn cat_zero_match_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = run_walx(fx.dir.path(), &["cat", &archive, "nope.txt"]);
    assert_eq!(code_of(&out), 2, "zero match is a usage error");
    assert!(out.stdout.is_empty(), "no bytes written on a usage error");
}

#[test]
fn cat_multi_match_mask_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    // `**` matches both notes.txt and src/main.rs.
    let out = run_walx(fx.dir.path(), &["cat", &archive, "**"]);
    assert_eq!(code_of(&out), 2, "multi match is a usage error");
    assert!(out.stdout.is_empty(), "no bytes written on a usage error");
}

#[test]
fn cat_never_writes_to_disk() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    // Snapshot the cwd (temp dir) entries before and after a successful cat.
    let before: Vec<_> = std::fs::read_dir(fx.dir.path())
        .expect("read dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    let out = run_walx(fx.dir.path(), &["cat", &archive, "notes.txt"]);
    assert_eq!(code_of(&out), 0);
    let after: Vec<_> = std::fs::read_dir(fx.dir.path())
        .expect("read dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(
        before, after,
        "cat created no files in the working directory"
    );
}

#[test]
fn cat_index_beyond_last_writes_original_and_warns_rc4() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    // notes.txt has count 2; index 9 is beyond last -> original + warn + RC 4.
    let out = run_walx(
        fx.dir.path(),
        &["cat", &archive, "notes.txt", "--index", "9"],
    );
    assert_eq!(code_of(&out), 4, "beyond-last numeric index exits RC 4");
    assert_eq!(out.stdout, b"cat", "the original bytes are written");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("notes.txt"), "warning names the file");
}
