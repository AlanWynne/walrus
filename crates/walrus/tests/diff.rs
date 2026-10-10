//! Integration tests for the FR-W6 public diff surface.
//!
//! These exercise ONLY the public API (`walrus_fmt::diff`, `DiffResult`,
//! `Granularity`, and the public `ChangeRecord`/`Op`) and prove end-to-end that
//! the produced splices reconstruct `new` from `old`. The strict `apply_record`
//! byte-identity assertions live in the crate's unit tests; this file guards the
//! public contract the Walker/CLI streams will call.
#![cfg(feature = "diff")]

use walrus_fmt::{diff, Granularity, Op};

/// Apply the public splices onto `old` over the public `Op::Splice` fields and
/// return the resulting bytes. Mirrors the crate's one splice primitive but
/// stays on the public surface (apply_record is crate-private).
fn apply_public(old: &str, result: &walrus_fmt::DiffResult) -> Vec<u8> {
    let mut doc = old.as_bytes().to_vec();
    for record in &result.splices {
        if let Op::Splice {
            at,
            delete_len,
            insert,
        } = &record.op
        {
            doc.splice(*at..(*at + *delete_len), insert.bytes());
        }
    }
    doc
}

#[test]
fn public_diff_of_an_insert_reconstructs_new_through_the_public_api() {
    let old = "a line\nanother line\n";
    let new = "a line\na new middle line\nanother line\n";
    let result = diff(old, new).expect("diff of utf-8 text succeeds");
    assert_eq!(apply_public(old, &result), new.as_bytes());
}

#[test]
fn public_diff_of_a_delete_reconstructs_new_through_the_public_api() {
    let old = "keep this\nremove this\nkeep that\n";
    let new = "keep this\nkeep that\n";
    let result = diff(old, new).expect("diff of utf-8 text succeeds");
    assert_eq!(apply_public(old, &result), new.as_bytes());
}

#[test]
fn public_diff_of_a_replace_reconstructs_new_through_the_public_api() {
    let old = "the quick brown fox";
    let new = "the quick red fox";
    let result = diff(old, new).expect("diff of utf-8 text succeeds");
    assert_eq!(apply_public(old, &result), new.as_bytes());
}

#[test]
fn public_diff_exposes_the_chosen_granularity() {
    let line_dense = diff("a\nb\nc\n", "a\nB\nc\n").expect("diff succeeds");
    assert_eq!(line_dense.granularity, Granularity::Line);
}

#[test]
fn public_diff_rejects_binary_input_with_an_error() {
    assert!(diff("fine", "has a \u{0000} nul").is_err());
}
