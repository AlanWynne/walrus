//! CLI-layer name helpers the `walrus-fmt` library deliberately does not
//! expose: expanding a segment-aware glob to the exact stored names it selects.
//!
//! The library's mutating ops (`remove`/`condense`/`convert`) match by EXACT
//! id-or-name, not glob, so the CLI must expand a `/`-glob mask to a set of
//! exact names here and drive the library op once per matched name (context:
//! the library-quirk note).

use walrus_fmt::Archive;

use crate::mask;

/// The stored names a segment-aware glob `mask` selects, in `archive.files()`
/// order (CLI-DESIGN 5.4c semantics via [`mask::matches`]). An exact name or id
/// with no wildcard yields at most one entry; a non-matching mask yields an
/// empty vector (the caller decides whether that is RC 8).
pub fn matching(archive: &Archive, mask: &str) -> Vec<String> {
    archive
        .files()
        .map(|f| f.name.clone())
        .filter(|name| mask::matches(mask, name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use walrus_fmt::{Archive, ChangeRecord, Manifest, Variant};

    /// Build the canonical multi-file fixture in memory: `notes.txt` with two
    /// splices over the original base, and a nested single-version
    /// `src/main.rs`.
    fn fixture() -> Archive {
        let mut a = Archive::new(Manifest::new(Variant::Wal));
        a.add("notes.txt", b"cat", None, None).expect("add notes");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(3, 0, "s")])
            .expect("notes save 1");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(0, 0, "the ")])
            .expect("notes save 2");
        a.add("src/main.rs", b"fn main() {}", None, None)
            .expect("add nested");
        a
    }

    #[test]
    fn matching_expands_a_segment_glob_to_the_selected_stored_names() {
        let archive = fixture();
        // A single `*` stays within one segment: it selects the top-level file
        // but not the nested one.
        assert_eq!(matching(&archive, "*"), vec!["notes.txt".to_string()]);
        // An exact name with no wildcard selects exactly that name.
        assert_eq!(
            matching(&archive, "src/main.rs"),
            vec!["src/main.rs".to_string()]
        );
        // A `**` spans `/` and selects every tracked file.
        assert_eq!(
            matching(&archive, "**"),
            vec!["notes.txt".to_string(), "src/main.rs".to_string()]
        );
        // A non-matching mask selects nothing.
        assert!(matching(&archive, "missing.txt").is_empty());
    }
}
