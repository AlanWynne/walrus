//! The FR-W7 `remove` mutate op (design section 8, M1).
//!
//! `remove` matches tracked files by id-or-name and drops them from BOTH
//! directories in lockstep via the [`crate::Archive::directories_mut`] seam, so
//! [`crate::Archive::to_bytes`] never sees a half-removed file. The match rules:
//! zero matches is [`WalrusError::NotTracked`]; one match removes regardless of
//! `force`; multiple matches without `force` is [`WalrusError::AmbiguousRemove`]
//! and removes nothing; multiple matches with `force` removes all.

use crate::archive::{Archive, ArchiveFile};
use crate::manifest::Manifest;
use crate::{Result, WalrusError};

impl Archive {
    /// Remove the tracked file(s) matched by `mask` (an id or an original name).
    ///
    /// # Errors
    /// Returns [`WalrusError::NotTracked`] when `mask` matches no file, and
    /// [`WalrusError::AmbiguousRemove`] when it matches more than one without
    /// `force` (nothing is removed in that case).
    pub fn remove(&mut self, mask: &str, force: bool) -> Result<()> {
        let matches: Vec<(String, String)> = self
            .manifest()
            .files
            .iter()
            .filter(|f| f.id == mask || f.name == mask)
            .map(|f| (f.id.clone(), f.name.clone()))
            .collect();

        match matches.len() {
            0 => Err(WalrusError::NotTracked {
                name: mask.to_string(),
            }),
            1 => {
                let (id, _) = &matches[0];
                let (manifest, files) = self.directories_mut();
                remove_tracked(manifest, files, id);
                Ok(())
            }
            n if !force => Err(WalrusError::AmbiguousRemove {
                mask: mask.to_string(),
                count: n,
                matches: matches
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
            _ => {
                let (manifest, files) = self.directories_mut();
                for (id, _) in &matches {
                    remove_tracked(manifest, files, id);
                }
                Ok(())
            }
        }
    }
}

// === free function ======================================================

/// Remove the file with `id` from BOTH directories in lockstep, returning the
/// removed parsed file (or `None` if absent). Keeping the manifest entry and the
/// parsed file in step is what guarantees [`Archive::to_bytes`] never zips a
/// half-removed file.
pub(crate) fn remove_tracked(
    manifest: &mut Manifest,
    files: &mut Vec<ArchiveFile>,
    id: &str,
) -> Option<ArchiveFile> {
    manifest.files.retain(|f| f.id != id);
    files
        .iter()
        .position(|f| f.id == id)
        .map(|pos| files.remove(pos))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, Variant};
    use crate::wchg::Direction;

    /// An archive with two files sharing the name "dup.txt" plus one unique.
    fn archive_with_duplicates() -> Archive {
        let mut a = Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new());
        a.add("dup.txt", b"one", None, Some(Direction::Forward))
            .expect("add 1");
        a.add("solo.txt", b"solo", None, Some(Direction::Forward))
            .expect("add solo");
        // Force a second tracked file with the SAME name but a distinct id:
        // the public add would UPDATE the existing name, so seed directly with
        // a hand-chosen id that cannot collide with a minted `f<N>`.
        let (manifest, files) = a.directories_mut();
        let tracked = crate::manifest::TrackedFile::new_with_direction(
            "dup-extra",
            "dup.txt",
            Direction::Forward,
            0,
        );
        let header = crate::wchg::WchgHeader::new(Direction::Forward, 0, "dup.txt", "utf-8", 0);
        let af = ArchiveFile {
            id: "dup-extra".to_string(),
            base: b"two".to_vec(),
            header,
            records: Vec::new(),
        };
        crate::mutate_add::push_tracked(manifest, files, tracked, af);
        a
    }

    #[test]
    fn remove_zero_match_returns_not_tracked() {
        let mut a = Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new());
        a.add("doc.txt", b"x", None, None).expect("add");
        let err = a.remove("nope", false).expect_err("zero match must error");
        assert!(matches!(err, WalrusError::NotTracked { .. }));
    }

    #[test]
    fn remove_single_match_removes_regardless_of_force() {
        let mut a = Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new());
        a.add("doc.txt", b"x", None, None).expect("add");
        a.remove("doc.txt", false).expect("single match removes");
        assert!(a.file("doc.txt").is_none(), "manifest entry gone");
        assert!(a.base("doc.txt").is_none(), "parsed file gone");
    }

    #[test]
    fn remove_single_match_with_force_also_removes() {
        let mut a = Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new());
        a.add("doc.txt", b"x", None, None).expect("add");
        a.remove("doc.txt", true)
            .expect("single match removes with force");
        assert!(a.file("doc.txt").is_none());
    }

    #[test]
    fn remove_multi_match_without_force_returns_ambiguous_listing_matches() {
        let mut a = archive_with_duplicates();
        let err = a
            .remove("dup.txt", false)
            .expect_err("multi match without force must error");
        match err {
            WalrusError::AmbiguousRemove { count, matches, .. } => {
                assert_eq!(count, 2);
                assert!(matches.contains("dup.txt"), "lists the matched names");
            }
            other => panic!("expected AmbiguousRemove, got {other:?}"),
        }
        // Nothing removed.
        assert_eq!(
            a.files().filter(|f| f.name == "dup.txt").count(),
            2,
            "an ambiguous remove removes nothing"
        );
    }

    #[test]
    fn remove_multi_match_with_force_removes_all() {
        let mut a = archive_with_duplicates();
        a.remove("dup.txt", true).expect("force removes all");
        assert_eq!(
            a.files().filter(|f| f.name == "dup.txt").count(),
            0,
            "all duplicates removed"
        );
        // The unique file survives.
        assert!(a.file("solo.txt").is_some(), "unmatched file untouched");
    }
}
