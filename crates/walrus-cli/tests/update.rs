//! `update` behaviour over fixtures (FR-W11 AC3, CLI-DESIGN 5.4).

mod common;

use common::{code_of, reconstruct, record_count, run_walrus, Fixture};
use walrus_fmt::Version;

#[test]
fn update_existing_file_records_a_new_version_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    // Provide new content for notes.txt as a file in the cwd.
    std::fs::write(fx.dir.path().join("notes.txt"), b"the cats sleep").expect("write");
    let result = run_walrus(fx.dir.path(), &["update", &archive, "notes.txt"]);
    assert_eq!(code_of(&result), 0, "update of a tracked file succeeds");
    assert_eq!(
        reconstruct(&fx.archive, "notes.txt", Version::Current),
        b"the cats sleep"
    );
    assert_eq!(record_count(&fx.archive, "notes.txt"), 3, "a version added");
}

#[test]
fn update_missing_target_is_rc8() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    std::fs::write(fx.dir.path().join("ghost.txt"), b"boo").expect("write");
    let result = run_walrus(fx.dir.path(), &["update", &archive, "ghost.txt"]);
    assert_eq!(code_of(&result), 8, "update of an untracked file is RC 8");
}

#[test]
fn update_missing_target_leaves_archive_unchanged() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let before = std::fs::read(&fx.archive).expect("read before");
    std::fs::write(fx.dir.path().join("ghost.txt"), b"boo").expect("write");
    run_walrus(fx.dir.path(), &["update", &archive, "ghost.txt"]);
    let after = std::fs::read(&fx.archive).expect("read after");
    assert_eq!(
        before, after,
        "a failed update is transactional (unchanged)"
    );
}
