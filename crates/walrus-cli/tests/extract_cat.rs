//! `extract` and `cat` behaviour over fixtures (FR-W11, CLI-DESIGN 5.2).
//!
//! The execution engines are shared with `walx` and already covered there; these
//! tests prove `walrus` wires its parse layer to them (RC 0, beyond-last RC 4,
//! cat one-match-or-RC2).

mod common;

use common::{code_of, run_walrus, Fixture};
use tempfile::TempDir;

fn read_out(out: &std::path::Path, rel: &[&str]) -> Vec<u8> {
    let mut path = out.to_path_buf();
    for seg in rel {
        path.push(seg);
    }
    std::fs::read(&path).unwrap_or_else(|_| panic!("read {}", path.display()))
}

#[test]
fn extract_subcommand_matches_walx_behaviour_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    let result = run_walrus(fx.dir.path(), &["extract", &archive, "-o", &out_arg]);
    assert_eq!(code_of(&result), 0, "extract-all succeeds");
    assert_eq!(read_out(out.path(), &["notes.txt"]), b"the cats");
    assert_eq!(read_out(out.path(), &["src", "main.rs"]), b"fn main() {}");
}

#[test]
fn extract_index_beyond_last_writes_original_and_warns_rc4() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let out = TempDir::new().expect("out dir");
    let out_arg = out.path().to_string_lossy().into_owned();

    // notes.txt count is 2; index 9 is beyond last -> original + RC 4.
    let result = run_walrus(
        fx.dir.path(),
        &[
            "extract",
            &archive,
            "notes.txt",
            "--index",
            "9",
            "-o",
            &out_arg,
        ],
    );
    assert_eq!(code_of(&result), 4, "beyond-last numeric index exits RC 4");
    assert_eq!(read_out(out.path(), &["notes.txt"]), b"cat");
}

#[test]
fn cat_single_match_writes_bytes_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    let result = run_walrus(fx.dir.path(), &["cat", &archive, "notes.txt"]);
    assert_eq!(code_of(&result), 0);
    assert_eq!(result.stdout, b"the cats", "cat writes current bytes");
}

#[test]
fn cat_zero_or_multi_match_is_rc2() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    // `**` matches both tracked files -> more than one -> RC 2.
    let multi = run_walrus(fx.dir.path(), &["cat", &archive, "**"]);
    assert_eq!(code_of(&multi), 2, "multi-match cat is RC 2");
    // A mask matching nothing -> zero -> RC 2.
    let zero = run_walrus(fx.dir.path(), &["cat", &archive, "nope.txt"]);
    assert_eq!(code_of(&zero), 2, "zero-match cat is RC 2");
}

#[test]
fn cat_at_timestamp_selector_parses_rc0() {
    let fx = Fixture::build();
    let archive = fx.archive_arg();
    // The fixture records carry no ts, so --at resolves to current (index 0).
    let result = run_walrus(
        fx.dir.path(),
        &["cat", &archive, "notes.txt", "--at", "2026-10-11T16:18:44Z"],
    );
    assert_eq!(code_of(&result), 0, "a valid --at shape is accepted");
    assert_eq!(result.stdout, b"the cats");
}
