//! `condense` behaviour over fixtures (FR-W11, CLI-DESIGN 5.6).

mod common;

use common::{code_of, reconstruct, record_count, run_walrus, Fixture};
use walrus_fmt::Version;

#[test]
fn condense_keep_zero_collapses_to_current() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let current = reconstruct(&fx.archive, "notes.txt", Version::Current);
    let result = run_walrus(fx.dir.path(), &["condense", &archive, "notes.txt"]);
    assert_eq!(code_of(&result), 0, "default keep 0 condenses");
    assert_eq!(record_count(&fx.archive, "notes.txt"), 0, "log collapsed");
    assert_eq!(
        reconstruct(&fx.archive, "notes.txt", Version::Current),
        current,
        "current is unchanged"
    );
}

#[test]
fn condense_keep_n_retains_recent() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    // notes.txt has count 2; --keep 1 retains indices [0, 1].
    let result = run_walrus(
        fx.dir.path(),
        &["condense", &archive, "notes.txt", "--keep", "1"],
    );
    assert_eq!(code_of(&result), 0);
    assert_eq!(record_count(&fx.archive, "notes.txt"), 1, "one record kept");
    assert_eq!(
        reconstruct(&fx.archive, "notes.txt", Version::Index(1)),
        b"cats"
    );
}

#[test]
fn condense_no_match_is_rc8() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["condense", &archive, "ghost.txt"]);
    assert_eq!(
        code_of(&result),
        8,
        "a no-match condense is RC 8 (pre-check)"
    );
}
