//! A pure, version-aware string matcher over reconstructed document bytes.
//!
//! This module is deliberately independent of the archive reader and the
//! reconstruct engine: it takes ALREADY-RECONSTRUCTED bytes plus the version
//! index they belong to and reports every occurrence of a needle. Keeping the
//! matcher pure honours the dependency boundary (invariant 2) and -- crucially
//! for the roadmap -- lets the single-version primitive [`search_version`]
//! compose into an all-versions result set whose entries carry differing
//! [`Match::version_index`] values. [`search_versions`] is the shared
//! composition point used by the CLI.
//!
//! The pure line/column deriver lives in the [`line_col`] submodule so this
//! file stays within the per-file line cap (NFR-1); it is a by-concern split
//! with no behaviour change.

mod line_col;

use crate::error::Result;
#[cfg(feature = "search-regex")]
use crate::error::WalrusError;

use line_col::line_col;

// === Public types =======================================================

/// How a needle is matched against the reconstructed bytes.
///
/// `Literal` (the default) matches a byte-for-byte substring. The
/// case-insensitivity toggle lives on [`SearchOptions`], not here, so it
/// composes with every mode. The `Regex` variant exists only when the
/// `search-regex` cargo feature is enabled, so the default build cannot name
/// it and never links the `regex` crate (invariant 2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum MatchMode {
    /// Match the needle as a literal substring (default).
    #[default]
    Literal,
    /// Match the needle as a regular expression. Available only under the
    /// `search-regex` feature.
    #[cfg(feature = "search-regex")]
    Regex,
}

/// Options controlling a search.
///
/// `case_insensitive` applies to BOTH literal and regex modes. For literal
/// mode the comparison lowercases needle and haystack with Rust's Unicode
/// `char` lowercasing; for regex mode it compiles the pattern with the
/// case-insensitive flag.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SearchOptions {
    /// The match mode (literal by default).
    pub mode: MatchMode,
    /// Fold ASCII/Unicode case when comparing (opt-in).
    pub case_insensitive: bool,
}

/// One occurrence of the needle in one reconstructed version.
///
/// The `version_index` is a FIELD ON EACH MATCH, not a report-level scalar, so
/// a result set spanning many versions is simply a `Vec<Match>` with mixed
/// `version_index` values -- the load-bearing shape for all-versions search.
///
/// # Line and column rule
/// `byte_offset` is the authoritative locator: the 0-based byte index of the
/// match start in the reconstructed bytes. From it:
/// - `line` is 1-based and equals `1 + (count of LF bytes 0x0A strictly before
///   byte_offset)`. LF is the sole line terminator (consistent with FORMAT.md
///   4.1 making LF the only `.wchg` terminator).
/// - `col` is 1-based and measured in UNICODE SCALAR VALUES (chars), not bytes:
///   `1 + (number of chars in the slice from the byte after the last LF
///   at-or-before byte_offset, up to byte_offset)`.
/// - CR (0x0D) is NOT a line terminator; in a CRLF document the CR counts as an
///   ordinary trailing character on its line.
/// - If `byte_offset` is not on a UTF-8 char boundary (reachable only for a
///   regex over invalid UTF-8), the column falls back to counting BYTES rather
///   than chars so the deriver stays total and never panics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The tracked file's stored name/path (filled by the caller).
    pub file_name: String,
    /// The version index this match was found in.
    pub version_index: usize,
    /// The change timestamp of that version, if known (filled by the caller).
    pub version_ts: Option<String>,
    /// 0-based byte offset of the match start in the reconstructed bytes.
    pub byte_offset: usize,
    /// 1-based line number (LF-counted; see the type-level rule).
    pub line: usize,
    /// 1-based column in Unicode scalar values (see the type-level rule).
    pub col: usize,
    /// The exact matched text (the needle for literal mode; the matched slice
    /// for regex mode), as a lossy UTF-8 rendering of the matched bytes.
    pub matched_text: String,
}

// === Single-version primitive ==========================================

/// Search ONE reconstructed version's bytes for `needle`, returning every
/// occurrence as a [`Match`] tagged with `version_index` and `version_ts`.
///
/// This is the single-version primitive; the all-versions search planned on the
/// roadmap composes it over an index range (see [`search_versions`]) without a
/// redesign, because each [`Match`] already carries its own version index.
///
/// An EMPTY literal needle matches nothing and returns `Ok(vec![])`. An empty
/// regex is invalid and returns [`WalrusError::SearchPattern`].
///
/// # Errors
/// Only the regex mode can error, and only when the `search-regex` feature is
/// enabled: an invalid or empty pattern yields [`WalrusError::SearchPattern`].
/// Literal and case-insensitive searches never error.
pub fn search_version(
    file_name: &str,
    version_index: usize,
    version_ts: Option<&str>,
    bytes: &[u8],
    needle: &str,
    opts: &SearchOptions,
) -> Result<Vec<Match>> {
    if needle.is_empty() {
        match &opts.mode {
            MatchMode::Literal => return Ok(Vec::new()),
            #[cfg(feature = "search-regex")]
            MatchMode::Regex => {
                return Err(WalrusError::SearchPattern("empty pattern".to_string()));
            }
        }
    }

    let spans = match &opts.mode {
        MatchMode::Literal => literal_spans(bytes, needle, opts.case_insensitive),
        #[cfg(feature = "search-regex")]
        MatchMode::Regex => regex_spans(bytes, needle, opts.case_insensitive)?,
    };

    Ok(spans
        .into_iter()
        .map(|(offset, len)| {
            let (line, col) = line_col(bytes, offset);
            Match {
                file_name: file_name.to_string(),
                version_index,
                version_ts: version_ts.map(str::to_string),
                byte_offset: offset,
                line,
                col,
                matched_text: String::from_utf8_lossy(&bytes[offset..offset + len]).into_owned(),
            }
        })
        .collect())
}

/// Byte-offset + byte-length spans of every literal occurrence of `needle` in
/// `bytes`. When `case_insensitive`, both sides are compared through Rust's
/// Unicode `char` lowercasing; otherwise the comparison is byte-for-byte.
///
/// Overlapping scan: after a hit the scan advances by ONE byte position (not by
/// the match length), so adjacent and overlapping occurrences are all reported
/// (e.g. "aa" in "aaa" at offsets 0 and 1). Offsets and lengths are always in
/// the ORIGINAL `bytes`, never the lowercased copy.
fn literal_spans(bytes: &[u8], needle: &str, case_insensitive: bool) -> Vec<(usize, usize)> {
    if case_insensitive {
        return literal_spans_caseless(bytes, needle);
    }
    let needle = needle.as_bytes();
    let mut out = Vec::new();
    if needle.len() > bytes.len() {
        return out;
    }
    let mut start = 0usize;
    while start + needle.len() <= bytes.len() {
        if &bytes[start..start + needle.len()] == needle {
            out.push((start, needle.len()));
        }
        start += 1;
    }
    out
}

/// Case-insensitive literal spans over the ORIGINAL bytes.
///
/// FOLDING RULE: both needle and haystack are lowercased with Rust's Unicode
/// `char::to_lowercase`. The haystack is matched one ORIGINAL char at a time:
/// for each candidate start char, successive haystack chars are folded and
/// compared against the folded needle char stream until the needle is exhausted
/// (a match) or a char diverges (no match). The reported span is always in
/// ORIGINAL bytes, so a case fold that changes byte length never shifts the
/// offset. When the haystack is not valid UTF-8 the matcher falls back to a
/// byte-exact ASCII-only caseless compare so offsets stay authoritative.
fn literal_spans_caseless(bytes: &[u8], needle: &str) -> Vec<(usize, usize)> {
    let haystack = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        // Not valid UTF-8: fall back to an ASCII-only caseless byte compare.
        Err(_) => return ascii_caseless_spans(bytes, needle),
    };
    let folded_needle: String = needle.chars().flat_map(char::to_lowercase).collect();
    let mut out = Vec::new();
    for (start, _) in haystack.char_indices() {
        if let Some(consumed) = folded_match_len(&haystack[start..], &folded_needle) {
            out.push((start, consumed));
        }
    }
    out
}

/// If `hay` (valid UTF-8) begins with a region whose Unicode-lowercasing equals
/// `folded_needle`, return the number of ORIGINAL bytes that region consumed;
/// otherwise `None`. A single original char may fold to several needle chars.
fn folded_match_len(hay: &str, folded_needle: &str) -> Option<usize> {
    let mut need = folded_needle.chars().peekable();
    let mut consumed = 0usize;
    for hc in hay.chars() {
        if need.peek().is_none() {
            break;
        }
        for lc in hc.to_lowercase() {
            match need.next() {
                Some(nc) if nc == lc => {}
                _ => return None,
            }
        }
        consumed += hc.len_utf8();
    }
    if need.peek().is_none() {
        Some(consumed)
    } else {
        None
    }
}

/// Byte-exact ASCII case-insensitive spans, used as the fallback when the
/// haystack is not valid UTF-8 (so offsets stay authoritative in the original
/// bytes). Only ASCII letters are folded; non-ASCII bytes compare exactly.
fn ascii_caseless_spans(bytes: &[u8], needle: &str) -> Vec<(usize, usize)> {
    let needle = needle.as_bytes();
    let mut out = Vec::new();
    if needle.is_empty() || needle.len() > bytes.len() {
        return out;
    }
    let mut start = 0usize;
    while start + needle.len() <= bytes.len() {
        if bytes[start..start + needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            out.push((start, needle.len()));
        }
        start += 1;
    }
    out
}

/// Byte-offset + byte-length spans of every regex match of `pattern` in
/// `bytes`. The haystack is searched as text (invalid UTF-8 is replaced
/// lossily for the scan, but reported offsets map back to the lossy string;
/// callers search reconstructed UTF-8 documents in practice). An empty or
/// invalid pattern is a typed [`WalrusError::SearchPattern`].
#[cfg(feature = "search-regex")]
fn regex_spans(bytes: &[u8], pattern: &str, case_insensitive: bool) -> Result<Vec<(usize, usize)>> {
    if pattern.is_empty() {
        return Err(WalrusError::SearchPattern("empty pattern".to_string()));
    }
    let re = regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| WalrusError::SearchPattern(e.to_string()))?;
    let haystack = String::from_utf8_lossy(bytes);
    Ok(re
        .find_iter(&haystack)
        .map(|m| (m.start(), m.end() - m.start()))
        .collect())
}

/// Search an inclusive range of version indices, concatenating each version's
/// matches in order. The CLI reconstructs each version and passes the indexed
/// byte slices here, keeping the matcher independent from archive I/O.
///
/// `versions` yields fallible `(version_index, version_ts, bytes)` triples;
/// the caller reconstructs lazily so only one version's bytes need to be held.
///
/// # Errors
/// Propagates an error from the version iterator or [`search_version`].
pub fn search_versions<I>(
    file_name: &str,
    versions: I,
    needle: &str,
    opts: &SearchOptions,
) -> Result<Vec<Match>>
where
    I: IntoIterator<Item = Result<(usize, Option<String>, Vec<u8>)>>,
{
    let mut all = Vec::new();
    for version in versions {
        let (version_index, version_ts, bytes) = version?;
        let mut hits = search_version(
            file_name,
            version_index,
            version_ts.as_deref(),
            &bytes,
            needle,
            opts,
        )?;
        all.append(&mut hits);
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_literal_substring_reports_line_and_byte_offset() {
        let bytes = b"hello walrus\nsecond line\n";
        let opts = SearchOptions::default();
        let hits = search_version(
            "doc.txt",
            0,
            Some("2026-01-01T00:00:00Z"),
            bytes,
            "walrus",
            &opts,
        )
        .expect("literal search");
        assert_eq!(hits.len(), 1);
        let m = &hits[0];
        assert_eq!(m.byte_offset, 6);
        assert_eq!(m.line, 1);
        assert_eq!(m.col, 7);
        assert_eq!(m.matched_text, "walrus");
        assert_eq!(m.file_name, "doc.txt");
        assert_eq!(m.version_index, 0);
        assert_eq!(m.version_ts.as_deref(), Some("2026-01-01T00:00:00Z"));
    }

    #[test]
    fn search_literal_absent_needle_returns_no_matches() {
        let bytes = b"hello walrus";
        let opts = SearchOptions::default();
        let hits =
            search_version("doc.txt", 0, None, bytes, "penguin", &opts).expect("literal search");
        assert!(hits.is_empty());
    }

    #[test]
    fn search_reports_every_occurrence_in_one_version() {
        let bytes = b"ababab";
        let opts = SearchOptions::default();
        let hits = search_version("doc.txt", 0, None, bytes, "ab", &opts).expect("search");
        let offsets: Vec<usize> = hits.iter().map(|m| m.byte_offset).collect();
        assert_eq!(offsets, vec![0, 2, 4]);
    }

    #[test]
    fn search_line_and_column_are_one_based_across_multiple_lines() {
        // "walrus" starts at byte 10 on line 3, column 1.
        let bytes = b"one\ntwo\nwalrus\n";
        let opts = SearchOptions::default();
        let hits = search_version("doc.txt", 0, None, bytes, "walrus", &opts).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].byte_offset, 8);
        assert_eq!(hits[0].line, 3);
        assert_eq!(hits[0].col, 1);
    }

    #[test]
    fn search_column_counts_unicode_scalars_not_bytes() {
        // Two 2-byte chars precede "x" on one line; the column is 3 (chars),
        // not 5 (bytes).
        let text = "\u{00E9}\u{00E9}x";
        let bytes = text.as_bytes();
        let opts = SearchOptions::default();
        let hits = search_version("doc.txt", 0, None, bytes, "x", &opts).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].byte_offset, 4);
        assert_eq!(hits[0].line, 1);
        assert_eq!(hits[0].col, 3);
    }

    #[test]
    fn search_crlf_document_treats_cr_as_ordinary_character() {
        // "b" sits after "a\r\n": line 2 (one LF before it), and the CR on
        // line 1 is an ordinary char, so "b" is column 1 of line 2.
        let bytes = b"a\r\nb";
        let opts = SearchOptions::default();
        let hits = search_version("doc.txt", 0, None, bytes, "b", &opts).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].byte_offset, 3);
        assert_eq!(hits[0].line, 2);
        assert_eq!(hits[0].col, 1);

        // The CR itself is the last column of line 1 (ordinary char).
        let cr_hits = search_version("doc.txt", 0, None, bytes, "\r", &opts).expect("search cr");
        assert_eq!(cr_hits.len(), 1);
        assert_eq!(cr_hits[0].line, 1);
        assert_eq!(cr_hits[0].col, 2);
    }

    #[test]
    fn search_case_insensitive_matches_mixed_case() {
        let bytes = b"Hello WALRUS world";
        let opts = SearchOptions {
            case_insensitive: true,
            ..SearchOptions::default()
        };
        let hits = search_version("doc.txt", 0, None, bytes, "walrus", &opts).expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].byte_offset, 6);
        // Case-sensitive search finds nothing.
        let strict = search_version(
            "doc.txt",
            0,
            None,
            bytes,
            "walrus",
            &SearchOptions::default(),
        )
        .expect("strict");
        assert!(strict.is_empty());
    }

    #[test]
    fn search_empty_needle_returns_no_matches() {
        let bytes = b"anything";
        let hits = search_version("doc.txt", 0, None, bytes, "", &SearchOptions::default())
            .expect("empty literal");
        assert!(hits.is_empty());
        let ci = search_version(
            "doc.txt",
            0,
            None,
            bytes,
            "",
            &SearchOptions {
                case_insensitive: true,
                ..SearchOptions::default()
            },
        )
        .expect("empty ci");
        assert!(ci.is_empty());
    }

    #[test]
    fn match_carries_the_version_index_it_was_found_in() {
        let bytes = b"walrus";
        let hits = search_version(
            "doc.txt",
            42,
            None,
            bytes,
            "walrus",
            &SearchOptions::default(),
        )
        .expect("search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].version_index, 42);
    }

    #[test]
    fn search_versions_range_tags_matches_with_their_own_index() {
        // Two hand-built versions; each carries its own index on the match.
        let v3 = b"walrus here".to_vec();
        let v5 = b"a walrus and a walrus".to_vec();
        let versions = vec![
            Ok((3usize, Some("2020-01-01T00:00:00Z".to_string()), v3)),
            Ok((5usize, Some("2021-01-01T00:00:00Z".to_string()), v5)),
        ];
        let hits = search_versions("doc.txt", versions, "walrus", &SearchOptions::default())
            .expect("range search");
        // One hit in v3, two in v5.
        let indices: Vec<usize> = hits.iter().map(|m| m.version_index).collect();
        assert_eq!(indices, vec![3, 5, 5]);
        assert_eq!(hits[0].version_ts.as_deref(), Some("2020-01-01T00:00:00Z"));
        assert_eq!(hits[1].version_ts.as_deref(), Some("2021-01-01T00:00:00Z"));
    }

    #[cfg(feature = "search-regex")]
    #[test]
    fn search_regex_matches_pattern_and_reports_offset() {
        let bytes = b"id=42 and id=7";
        let opts = SearchOptions {
            mode: MatchMode::Regex,
            ..SearchOptions::default()
        };
        let hits = search_version("doc.txt", 0, None, bytes, r"id=\d+", &opts).expect("regex");
        let found: Vec<&str> = hits.iter().map(|m| m.matched_text.as_str()).collect();
        assert_eq!(found, vec!["id=42", "id=7"]);
        assert_eq!(hits[0].byte_offset, 0);
    }

    #[cfg(feature = "search-regex")]
    #[test]
    fn search_regex_invalid_pattern_is_a_typed_error() {
        let bytes = b"anything";
        let opts = SearchOptions {
            mode: MatchMode::Regex,
            ..SearchOptions::default()
        };
        let err = search_version("doc.txt", 0, None, bytes, "(", &opts)
            .expect_err("invalid pattern must error");
        assert!(matches!(err, WalrusError::SearchPattern(_)));
    }

    #[cfg(feature = "search-regex")]
    #[test]
    fn search_regex_empty_pattern_is_a_typed_error() {
        let opts = SearchOptions {
            mode: MatchMode::Regex,
            ..SearchOptions::default()
        };
        let err = search_version("doc.txt", 0, None, b"anything", "", &opts)
            .expect_err("empty regex must error");
        assert!(matches!(err, WalrusError::SearchPattern(_)));
    }
}
