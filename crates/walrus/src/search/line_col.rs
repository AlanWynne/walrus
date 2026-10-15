//! The pure line/column deriver for [`crate::search::Match`].
//!
//! Split into its own file (by concern) so the matcher module stays within the
//! per-file line cap (NFR-1). This is deterministic, panic-free logic with no
//! dependency on the matcher's types; the full rule it implements is documented
//! on [`crate::search::Match`].

/// Derive the 1-based `(line, col)` of `offset` in `bytes` per the rule
/// documented on [`crate::search::Match`]: line = 1 + LF bytes strictly before
/// `offset`; column = 1 + Unicode scalar values from the byte after the last LF
/// at-or-before `offset` up to `offset`. If `offset` is not on a UTF-8 char
/// boundary (regex over invalid UTF-8 only), the column counts BYTES so the
/// deriver stays total.
pub(super) fn line_col(bytes: &[u8], offset: usize) -> (usize, usize) {
    let offset = offset.min(bytes.len());
    let before = &bytes[..offset];
    let line = 1 + before.iter().filter(|&&b| b == b'\n').count();
    // Byte index just after the last LF at-or-before offset (0 if none).
    let line_start = match before.iter().rposition(|&b| b == b'\n') {
        Some(pos) => pos + 1,
        None => 0,
    };
    let segment = &bytes[line_start..offset];
    let col = match std::str::from_utf8(segment) {
        Ok(text) => 1 + text.chars().count(),
        // Not valid UTF-8 (only reachable for a regex over invalid bytes):
        // count bytes so the column stays defined.
        Err(_) => 1 + segment.len(),
    };
    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_at_start_is_one_one() {
        assert_eq!(line_col(b"walrus", 0), (1, 1));
    }

    #[test]
    fn line_col_counts_lf_bytes_for_the_line() {
        // Offset 8 lands on line 3 ("one\ntwo\n" is 8 bytes, two LFs before 8).
        assert_eq!(line_col(b"one\ntwo\nwalrus\n", 8), (3, 1));
    }

    #[test]
    fn line_col_column_counts_unicode_scalars_not_bytes() {
        // Two 2-byte chars precede offset 4; the column is 3 (chars), not 5.
        let bytes = "\u{00E9}\u{00E9}x".as_bytes();
        assert_eq!(line_col(bytes, 4), (1, 3));
    }

    #[test]
    fn line_col_treats_cr_as_ordinary_character() {
        // "a\r\nb": the CR is at offset 1, an ordinary char on line 1 col 2.
        assert_eq!(line_col(b"a\r\nb", 1), (1, 2));
        // "b" is at offset 3: line 2 (one LF before), column 1.
        assert_eq!(line_col(b"a\r\nb", 3), (2, 1));
    }

    #[test]
    fn line_col_offset_past_end_is_clamped() {
        // An out-of-range offset clamps to the end rather than panicking.
        assert_eq!(line_col(b"ab", 99), (1, 3));
    }

    #[test]
    fn line_col_non_char_boundary_falls_back_to_byte_count() {
        // Offset 1 lands inside a 2-byte UTF-8 char: the column counts bytes.
        let bytes = "\u{00E9}".as_bytes();
        assert_eq!(line_col(bytes, 1), (1, 2));
    }
}
