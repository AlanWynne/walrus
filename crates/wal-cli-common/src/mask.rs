//! Segment-aware `/` glob matching for file masks (CLI-DESIGN 5.4c).
//!
//! A mask is matched against a stored, `/`-normalized name with these glob
//! semantics (one rule shared across the CLI): `*` matches within a single
//! path segment and never crosses `/`; `**` matches across segments including
//! `/`; `?` matches a single non-`/` character; a mask with no wildcard is an
//! exact whole-name match. The matcher operates purely on `/`-separated names,
//! never the OS separator.

/// True when `mask` matches the stored, `/`-normalized `name` under the
/// segment-aware glob semantics of CLI-DESIGN 5.4c.
///
/// `*` stays within one segment; `**` spans `/`; `?` matches one non-`/`
/// character; a mask with no wildcard must equal `name` exactly.
pub fn matches(mask: &str, name: &str) -> bool {
    glob(mask.as_bytes(), name.as_bytes())
}

/// Recursive glob matcher over byte slices. `*` and `?` never cross a `/`;
/// `**` (two consecutive `*`) spans any run including `/`.
fn glob(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some(b'*') => {
            if pattern.get(1) == Some(&b'*') {
                glob_double_star(&pattern[2..], text)
            } else {
                glob_single_star(&pattern[1..], text)
            }
        }
        Some(&b'?') => match text.first() {
            Some(&c) if c != b'/' => glob(&pattern[1..], &text[1..]),
            _ => false,
        },
        Some(&p) => match text.first() {
            Some(&c) if c == p => glob(&pattern[1..], &text[1..]),
            _ => false,
        },
    }
}

/// Match `**`: the rest of the pattern may begin at any position in `text`,
/// including after `/` separators.
fn glob_double_star(rest: &[u8], text: &[u8]) -> bool {
    for split in 0..=text.len() {
        if glob(rest, &text[split..]) {
            return true;
        }
    }
    false
}

/// Match a single `*`: the rest of the pattern may begin at any position up to
/// the next `/`, which `*` may not cross.
fn glob_single_star(rest: &[u8], text: &[u8]) -> bool {
    for split in 0..=text.len() {
        if glob(rest, &text[split..]) {
            return true;
        }
        // A single star stops at a segment boundary.
        if text.get(split) == Some(&b'/') {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn star_matches_within_one_segment_only_not_across_slash() {
        assert!(matches("src/*", "src/main.rs"));
        assert!(!matches("src/*", "src/net/http.rs"));
    }

    #[test]
    fn double_star_matches_across_slashes() {
        assert!(matches("src/**", "src/main.rs"));
        assert!(matches("src/**", "src/net/http.rs"));
    }

    #[test]
    fn question_matches_one_non_slash_char() {
        assert!(matches("a?c", "abc"));
        assert!(!matches("a?c", "a/c"));
        assert!(!matches("a?c", "ac"));
    }

    #[test]
    fn no_wildcard_is_exact_whole_name_match() {
        assert!(matches("notes.txt", "notes.txt"));
        assert!(!matches("notes", "notes.txt"));
        assert!(!matches("notes.txt", "src/notes.txt"));
    }
}
