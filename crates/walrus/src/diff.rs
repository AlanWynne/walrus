//! Text diff engine (FR-W6): turn an old-vs-new text pair into byte-offset
//! splices that reconstruct `new` from `old` through the one splice primitive.
//!
//! The adapter wraps the `similar` crate behind a thin diff-to-splice layer and
//! is gated behind the `diff` cargo feature so a playback-only embedder stays
//! lean (dependency boundary, invariant 2). It chooses a diff granularity
//! adaptively (line / word / grapheme), translates `similar`'s token-level edits
//! into byte-offset [`ChangeRecord`] splices against the original `old` bytes,
//! and rejects binary input with a typed error.

use similar::{Algorithm, ChangeTag, TextDiff};

use crate::record::ChangeRecord;
use crate::{Result, WalrusError};

// === Public surface =================================================

/// The diff granularity actually used for a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// Diff whole lines; chosen for line-dense content.
    Line,
    /// Diff words; the fallback for a single-line blob with word boundaries.
    Word,
    /// Diff grapheme clusters; the fallback for content with no word breaks and
    /// the path that guarantees char splits land on cluster boundaries.
    Grapheme,
}

impl Granularity {
    /// The string stored in the `.wchg` header `granularity_hint` field.
    pub fn hint(self) -> &'static str {
        match self {
            Granularity::Line => "line",
            Granularity::Word => "word",
            Granularity::Grapheme => "grapheme",
        }
    }
}

/// The result of a diff: the forward-sense splices plus the granularity used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    /// Byte-offset splices that, applied to `old` in order, reconstruct `new`.
    pub splices: Vec<ChangeRecord>,
    /// The granularity the diff was computed at.
    pub granularity: Granularity,
}

/// Diff `old` against `new`, producing byte-offset splices that reconstruct
/// `new` from `old`. Identical inputs yield zero splices.
///
/// # Errors
/// Returns [`WalrusError::BinaryInput`] if either input, though valid UTF-8,
/// contains a NUL byte (the binary-input detection rule for this layer).
pub fn diff(old: &str, new: &str) -> Result<DiffResult> {
    reject_binary(old, new)?;
    let granularity = choose_granularity(old, new);
    let splices = diff_at(old, new, granularity);
    Ok(DiffResult {
        splices,
        granularity,
    })
}

// === Private helpers ================================================

/// Reject input that is valid UTF-8 but contains a NUL byte as binary (D6).
///
/// # Errors
/// Returns [`WalrusError::BinaryInput`] when a NUL byte is present.
fn reject_binary(old: &str, new: &str) -> Result<()> {
    if old.as_bytes().contains(&0) || new.as_bytes().contains(&0) {
        return Err(WalrusError::BinaryInput {
            reason: "input contains a NUL byte".to_string(),
        });
    }
    Ok(())
}

/// Choose a diff granularity by sniffing the inputs (D5).
///
/// Line-dense content (any newline present in either input) diffs at LINE
/// granularity. A single-line blob with a space-separated word boundary diffs
/// at WORD granularity. Content with neither newline nor word boundary falls
/// back to GRAPHEME granularity so char-level splits land on cluster
/// boundaries.
fn choose_granularity(old: &str, new: &str) -> Granularity {
    if old.contains('\n') || new.contains('\n') {
        Granularity::Line
    } else if old.contains(char::is_whitespace) || new.contains(char::is_whitespace) {
        Granularity::Word
    } else {
        Granularity::Grapheme
    }
}

/// Diff `old` against `new` at `granularity`, producing byte-offset splices.
///
/// Walks `similar`'s per-token change stream while maintaining a running BYTE
/// cursor into the ORIGINAL `old`. Equal/Delete tokens advance the cursor by
/// their own byte length; Insert tokens contribute inserted bytes. Adjacent
/// delete/insert activity at one cursor position is coalesced into a single
/// `{at, delete_len, insert}` splice (D4), so a replacement is one record.
fn diff_at(old: &str, new: &str, granularity: Granularity) -> Vec<ChangeRecord> {
    let diff = build_text_diff(old, new, granularity);

    let mut splices = Vec::new();
    let mut cursor = 0usize; // byte offset into `old`
    let mut run_start = 0usize; // byte offset (into `old`) where the run began
    let mut delete_len = 0usize; // bytes deleted in the current run
    let mut insert = String::new(); // bytes inserted in the current run
    let mut in_run = false;
    // Splices apply SEQUENTIALLY via apply_record, each onto the document left
    // by the previous one; but `similar` reports offsets into the ORIGINAL
    // `old`. Carry a running signed byte delta (inserted minus deleted so far)
    // so each emitted `at` is valid for the evolving document.
    let mut shift: isize = 0;

    for change in diff.iter_all_changes() {
        let value = change.value();
        match change.tag() {
            ChangeTag::Equal => {
                flush_run(
                    &mut splices,
                    &mut in_run,
                    run_start,
                    &mut delete_len,
                    &mut insert,
                    &mut shift,
                );
                cursor += value.len();
            }
            ChangeTag::Delete => {
                if !in_run {
                    run_start = cursor;
                    in_run = true;
                }
                delete_len += value.len();
                cursor += value.len();
            }
            ChangeTag::Insert => {
                if !in_run {
                    run_start = cursor;
                    in_run = true;
                }
                insert.push_str(value);
            }
        }
    }
    flush_run(
        &mut splices,
        &mut in_run,
        run_start,
        &mut delete_len,
        &mut insert,
        &mut shift,
    );

    splices
}

/// Build a `similar::TextDiff` configured for the chosen granularity.
fn build_text_diff<'a>(
    old: &'a str,
    new: &'a str,
    granularity: Granularity,
) -> TextDiff<'a, 'a, 'a, str> {
    let mut config = TextDiff::configure();
    config.algorithm(Algorithm::Myers);
    match granularity {
        Granularity::Line => config.diff_lines(old, new),
        Granularity::Word => config.diff_words(old, new),
        Granularity::Grapheme => config.diff_graphemes(old, new),
    }
}

/// Emit the accumulated edit run (if any) as one coalesced splice and reset the
/// run state. The splice offset is the original-`old` position shifted by the
/// net byte delta of previously emitted splices, so the records apply
/// sequentially onto the evolving document. `shift` is advanced by this
/// splice's own delta.
fn flush_run(
    splices: &mut Vec<ChangeRecord>,
    in_run: &mut bool,
    run_start: usize,
    delete_len: &mut usize,
    insert: &mut String,
    shift: &mut isize,
) {
    if *in_run {
        let at = (run_start as isize + *shift) as usize;
        let inserted_len = insert.len();
        splices.push(ChangeRecord::splice(
            at,
            *delete_len,
            std::mem::take(insert),
        ));
        *shift += inserted_len as isize - *delete_len as isize;
        *delete_len = 0;
        *in_run = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::apply_record;

    // === helpers ====================================================

    /// Fold `splices` onto `old` using the one crate splice primitive
    /// (`reconstruct::apply_record`, D7) and return the resulting bytes.
    fn apply_all(old: &str, splices: &[ChangeRecord]) -> Vec<u8> {
        let mut doc = old.as_bytes().to_vec();
        for record in splices {
            apply_record(&mut doc, record).expect("splice must apply within bounds");
        }
        doc
    }

    /// Assert that diffing `old`->`new` reconstructs `new` byte-for-byte.
    fn assert_reconstructs(old: &str, new: &str) {
        let result = diff(old, new).expect("diff of utf-8 text must succeed");
        assert_eq!(
            apply_all(old, &result.splices),
            new.as_bytes(),
            "applying the diff splices to old must reproduce new byte-for-byte"
        );
    }

    // === AC3: binary rejection ======================================

    #[test]
    fn diff_rejects_a_string_containing_a_nul_byte_as_binary() {
        let nul = "bad\u{0000}here";
        assert!(matches!(
            diff("ok", nul),
            Err(WalrusError::BinaryInput { .. })
        ));
        assert!(matches!(
            diff(nul, "ok"),
            Err(WalrusError::BinaryInput { .. })
        ));
    }

    #[test]
    fn diff_of_plain_utf8_text_is_not_treated_as_binary() {
        assert!(diff("hello", "hallo").is_ok());
    }

    // === AC1: byte-identical reconstruction via apply_record ========

    #[test]
    fn diff_of_a_pure_insertion_reconstructs_new_exactly() {
        assert_reconstructs("the cat", "the black cat");
    }

    #[test]
    fn diff_of_a_pure_deletion_reconstructs_new_exactly() {
        assert_reconstructs("the black cat", "the cat");
    }

    #[test]
    fn diff_of_a_replacement_reconstructs_new_exactly() {
        assert_reconstructs("the quick fox", "the slow fox");
    }

    #[test]
    fn diff_of_an_edit_at_the_start_reconstructs_new_exactly() {
        assert_reconstructs("alpha beta gamma", "ALPHA beta gamma");
    }

    #[test]
    fn diff_of_an_edit_in_the_middle_reconstructs_new_exactly() {
        assert_reconstructs("alpha beta gamma", "alpha BETA gamma");
    }

    #[test]
    fn diff_of_an_edit_at_the_end_reconstructs_new_exactly() {
        assert_reconstructs("alpha beta gamma", "alpha beta GAMMA");
    }

    #[test]
    fn diff_from_an_empty_old_reconstructs_new_exactly() {
        assert_reconstructs("", "freshly written content\nwith two lines\n");
    }

    #[test]
    fn diff_to_an_empty_new_reconstructs_empty_exactly() {
        assert_reconstructs("content to be removed\nentirely\n", "");
    }

    #[test]
    fn diff_of_identical_inputs_produces_zero_splices() {
        let text = "unchanged content\nover two lines\n";
        let result = diff(text, text).expect("diff must succeed");
        assert!(
            result.splices.is_empty(),
            "identical inputs must produce no splices"
        );
        assert_eq!(apply_all(text, &result.splices), text.as_bytes());
    }

    #[test]
    fn diff_of_multiline_edits_reconstructs_new_exactly() {
        let old = "line one\nline two\nline three\nline four\n";
        let new = "line one\nline TWO changed\nline three\nadded line\nline four\n";
        assert_reconstructs(old, new);
    }

    // === AC2: adaptive granularity + grapheme safety ================

    #[test]
    fn line_dense_content_is_diffed_at_line_granularity() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nb\nc changed\nd\ne\n";
        let result = diff(old, new).expect("diff must succeed");
        assert_eq!(result.granularity, Granularity::Line);
    }

    #[test]
    fn a_blob_without_line_breaks_falls_back_to_word_or_char_granularity() {
        let old = "the quick brown fox jumps over the lazy dog";
        let new = "the quick brown cat jumps over the lazy dog";
        let result = diff(old, new).expect("diff must succeed");
        assert_ne!(result.granularity, Granularity::Line);
    }

    #[test]
    fn char_level_diff_splits_on_grapheme_boundaries_for_cjk_text() {
        // Build CJK fixtures from char escapes to keep source strictly ASCII.
        // U+4F60 U+597D U+4E16 U+754C == "ni hao shi jie" (hello world).
        let hello: String = ['\u{4F60}', '\u{597D}'].iter().collect();
        let world: String = ['\u{4E16}', '\u{754C}'].iter().collect();
        let old = format!("{hello}{world}");
        // Replace the middle character to force a mid-string char-level edit.
        let new = format!("{hello}\u{4E2D}{world}");

        let result = diff(&old, &new).expect("diff must succeed");

        // Every splice boundary must land on a char boundary of `old`.
        for record in &result.splices {
            if let crate::record::Op::Splice { at, delete_len, .. } = &record.op {
                assert!(
                    old.is_char_boundary(*at),
                    "splice start {at} split a multi-byte character"
                );
                assert!(
                    old.is_char_boundary(at + delete_len),
                    "splice end split a multi-byte character"
                );
            }
        }

        // And it must still reconstruct byte-for-byte.
        assert_eq!(apply_all(&old, &result.splices), new.as_bytes());
    }

    #[test]
    fn chosen_granularity_is_recorded_in_the_wchg_header_hint() {
        use crate::wchg::{Direction, WchgHeader};

        let old = "a\nb\nc\n";
        let new = "a\nB\nc\n";
        let result = diff(old, new).expect("diff must succeed");

        let mut header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", 1);
        header.granularity_hint = Some(result.granularity.hint().to_string());

        let line = serde_json::to_string(&header).expect("serialise header");
        let parsed = WchgHeader::parse(&line).expect("parse header");
        assert_eq!(parsed.granularity_hint.as_deref(), Some("line"));
    }

    #[test]
    fn granularity_hint_strings_are_stable() {
        assert_eq!(Granularity::Line.hint(), "line");
        assert_eq!(Granularity::Word.hint(), "word");
        assert_eq!(Granularity::Grapheme.hint(), "grapheme");
    }
}
