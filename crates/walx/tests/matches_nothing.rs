//! The RC-8 (extract) vs RC-2 (cat) "matches nothing" disambiguation (5.6).

mod common;

use common::{code_of, run_walx, Fixture};
use tempfile::TempDir;

#[test]
fn extract_mask_matching_nothing_is_rc8() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walx(
        fx.dir.path(),
        &["extract", &archive, "no-such-mask-*", "-o", &out_arg],
    );
    assert_eq!(code_of(&result), 8, "an empty extract match set is RC 8");
}

#[test]
fn extract_over_empty_archive_is_rc8() {
    let fx = Fixture::empty();
    let archive = fx.archive.to_string_lossy().into_owned();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    // Omitted mask over an EMPTY archive selects nothing -> RC 8.
    let result = run_walx(fx.dir.path(), &["extract", &archive, "-o", &out_arg]);
    assert_eq!(code_of(&result), 8, "extract over an empty archive is RC 8");
}

#[test]
fn cat_mask_matching_nothing_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive.to_string_lossy().into_owned();
    let result = run_walx(fx.dir.path(), &["cat", &archive, "no-such-mask-*"]);
    assert_eq!(code_of(&result), 2, "cat zero-match is RC 2, never RC 8");
}
