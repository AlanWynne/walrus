//! The FR-W7 `convert` op (decision c, INV-3): flip selected files between the
//! forward and reverse directions, returning a NEW [`Archive`].
//!
//! Per-file flip is the primitive; whole-archive is the no-mask degenerate case.
//! A flip inverts the file's splice list against its base via the single
//! inversion helper ([`crate::invert::invert_forward_splices`]) and re-bases:
//! Forward -> Reverse makes the current the new base with records walking
//! current -> original; Reverse -> Forward is the symmetric inverse. `count` is
//! INVARIANT across a flip, so checkpoints keep their indices and index 0 stays
//! current in both directions (so every index reconstructs byte-identically).
//!
//! When `mask == None` (whole-archive) the manifest `variant` and its two
//! dependent descriptors (`base_role`, `log_direction`) are re-derived TOGETHER
//! so the converted manifest passes [`crate::Manifest::validate`] on re-open
//! (H-2). A masked convert leaves `variant` and both descriptors untouched.
//! `convert` never renames the on-disk file.

use crate::archive::{Archive, ArchiveFile};
use crate::manifest::{Variant, Versions};
use crate::record::ChangeRecord;
use crate::wchg::{Direction, WchgHeader};
use crate::{invert, Result};

impl Archive {
    /// Convert the direction of the file(s) selected by `mask` (an id or name;
    /// `None` selects every file). `to == None` flips each selected file to the
    /// opposite of its current direction; `to == Some(d)` makes each selected
    /// file direction `d` (a file already in `d` is a no-op). Returns a NEW
    /// [`Archive`]; the receiver is left unchanged.
    ///
    /// # Errors
    /// Propagates [`crate::WalrusError::SpliceOutOfBounds`] from the inversion of
    /// an inconsistent file.
    pub fn convert(&self, mask: Option<&str>, to: Option<Direction>) -> Result<Archive> {
        let mut manifest = self.manifest().clone();
        let mut new_files: Vec<ArchiveFile> = Vec::with_capacity(self.files().count());

        for tracked in manifest.files.clone() {
            let selected = match mask {
                None => true,
                Some(m) => tracked.id == m || tracked.name == m,
            };
            let af = self.archive_file_for(&tracked.id);
            let target = to.unwrap_or(opposite(tracked.direction));

            if !selected || target == tracked.direction {
                // Keep the file as-is (not selected, or already in `to`).
                new_files.push(clone_archive_file(af));
                continue;
            }

            // Flip: invert the splice list against the current base.
            let (new_base, new_records) = flip_file(&af.base, &af.records)?;
            let count = new_records.iter().filter(|r| r.is_splice()).count();
            let header = WchgHeader::new(
                target,
                af.header.generations,
                af.header.name.clone(),
                af.header.encoding.clone(),
                count,
            );
            new_files.push(ArchiveFile {
                id: tracked.id.clone(),
                base: new_base,
                header,
                records: new_records,
            });

            // Update this file's manifest entry (count INVARIANT, encoding and
            // checkpoints preserved; index 0 stays current).
            if let Some(entry) = manifest.files.iter_mut().find(|f| f.id == tracked.id) {
                entry.direction = target;
                entry.versions = Versions::for_direction(target, count);
            }
        }

        // Whole-archive convert: re-derive variant + both descriptors TOGETHER.
        if mask.is_none() {
            let new_variant = predominant_variant(&new_files, &manifest);
            manifest.variant = new_variant;
            manifest.base_role = new_variant.base_role().to_string();
            manifest.log_direction = new_variant.log_direction().to_string();
        }

        Ok(Archive::from_directories(manifest, new_files))
    }

    /// Borrow the parsed per-file entry for `id` (present by construction, since
    /// the manifest and the parsed files share ids).
    fn archive_file_for(&self, id: &str) -> &ArchiveFile {
        self.files_ref()
            .iter()
            .find(|f| f.id == id)
            .expect("manifest and parsed files share ids")
    }
}

// === helpers ============================================================

/// The opposite direction.
fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::Forward => Direction::Reverse,
        Direction::Reverse => Direction::Forward,
    }
}

/// Flip a file: invert its splice list against its stored `base`, returning the
/// new base and records. For a FORWARD file (`base` = original, `records`
/// forward) this yields `(current, reverse_records)`; for a REVERSE file
/// (`base` = current, `records` reverse) it yields `(original, forward_records)`
/// -- the inversion helper is direction-agnostic, it simply inverts a splice
/// list applied to a given version.
fn flip_file(base: &[u8], records: &[ChangeRecord]) -> Result<(Vec<u8>, Vec<ChangeRecord>)> {
    invert::invert_forward_splices(base, records)
}

/// Deep-copy a parsed per-file entry (the convert output is a NEW archive).
fn clone_archive_file(af: &ArchiveFile) -> ArchiveFile {
    ArchiveFile {
        id: af.id.clone(),
        base: af.base.clone(),
        header: af.header.clone(),
        records: af.records.clone(),
    }
}

/// The new predominant variant after a whole-archive flip: the variant of the
/// direction most files now carry. On a tie (or empty), fall back to the
/// current manifest variant so the result stays deterministic.
fn predominant_variant(files: &[ArchiveFile], manifest: &crate::manifest::Manifest) -> Variant {
    let forward = files
        .iter()
        .filter(|f| f.header.direction == Direction::Forward)
        .count();
    let reverse = files.len() - forward;
    if forward > reverse {
        Variant::Wal
    } else if reverse > forward {
        Variant::Wbl
    } else {
        manifest.variant
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, Variant};
    use crate::{FileId, Version};

    /// Build a forward archive with a multi-splice history over "abcdef".
    fn rich_forward_archive() -> Archive {
        let mut a = Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new());
        a.add("doc.txt", b"abcdef", None, Some(Direction::Forward))
            .expect("add");
        a.update_with_splices("doc.txt", &[ChangeRecord::splice(6, 0, "GH")])
            .expect("save 1");
        a.update_with_splices("doc.txt", &[ChangeRecord::splice(0, 2, "XY")])
            .expect("save 2");
        a.update_with_splices("doc.txt", &[ChangeRecord::splice(4, 1, "ZZZ")])
            .expect("save 3");
        a
    }

    fn all_indices(archive: &Archive, id: &str) -> Vec<Vec<u8>> {
        let count = archive.file(id).expect("file").versions.count;
        (0..=count)
            .map(|k| {
                archive
                    .reconstruct(&FileId::from(id), Version::Index(k))
                    .expect("reconstruct")
            })
            .collect()
    }

    #[test]
    fn convert_forward_reverse_forward_reconstructs_every_index_identically() {
        let original = rich_forward_archive();
        let before = all_indices(&original, "doc_txt");

        let reversed = original.convert(None, None).expect("flip to reverse");
        assert_eq!(
            reversed.file("doc_txt").expect("file").direction,
            Direction::Reverse
        );
        let mid = all_indices(&reversed, "doc_txt");
        assert_eq!(before, mid, "every index identical after forward->reverse");

        let back = reversed.convert(None, None).expect("flip back to forward");
        assert_eq!(
            back.file("doc_txt").expect("file").direction,
            Direction::Forward
        );
        let after = all_indices(&back, "doc_txt");
        assert_eq!(before, after, "every index identical after the round-trip");
    }

    #[test]
    fn whole_archive_convert_sets_variant_and_descriptors_and_survives_to_bytes_read_roundtrip() {
        let original = rich_forward_archive();
        let reversed = original.convert(None, None).expect("flip");
        assert_eq!(reversed.manifest().variant, Variant::Wbl);
        assert_eq!(reversed.manifest().base_role, "current");
        assert_eq!(reversed.manifest().log_direction, "reverse");
        reversed
            .manifest()
            .validate()
            .expect("converted manifest validates");

        // Survives a to_bytes() -> read() re-validation.
        let bytes = reversed.to_bytes().expect("to_bytes");
        let reopened = Archive::read(&bytes).expect("read re-validates");
        assert_eq!(reopened.manifest().variant, Variant::Wbl);
    }

    #[test]
    fn masked_convert_flips_only_matched_and_leaves_variant_untouched() {
        let mut a = Archive::from_parts_for_test(Manifest::new(Variant::Wal), Vec::new());
        a.add("a.txt", b"aaa", None, Some(Direction::Forward))
            .expect("add a");
        a.add("b.txt", b"bbb", None, Some(Direction::Forward))
            .expect("add b");

        let converted = a.convert(Some("a.txt"), None).expect("masked convert");
        assert_eq!(
            converted.file("a_txt").expect("a").direction,
            Direction::Reverse,
            "matched file flipped"
        );
        assert_eq!(
            converted.file("b_txt").expect("b").direction,
            Direction::Forward,
            "unmatched file untouched"
        );
        assert_eq!(
            converted.manifest().variant,
            Variant::Wal,
            "a masked convert leaves variant untouched"
        );
    }

    #[test]
    fn convert_preserves_count_and_checkpoints() {
        let original = rich_forward_archive();
        let count_before = original.file("doc_txt").expect("file").versions.count;

        // Attach a checkpoint, then convert.
        let mut with_cp = original;
        {
            let tracked = with_cp
                .manifest_mut()
                .files
                .iter_mut()
                .find(|f| f.id == "doc_txt")
                .expect("file");
            tracked.checkpoints.push(crate::manifest::Checkpoint {
                index: 1,
                label: "milestone".to_string(),
            });
        }

        let reversed = with_cp.convert(None, None).expect("flip");
        let tracked = reversed.file("doc_txt").expect("file");
        assert_eq!(tracked.versions.count, count_before, "count invariant");
        assert_eq!(tracked.checkpoints.len(), 1, "checkpoints preserved");
        assert_eq!(tracked.checkpoints[0].index, 1, "checkpoint index kept");
    }

    #[test]
    fn convert_to_same_direction_is_a_noop() {
        let original = rich_forward_archive();
        let same = original
            .convert(None, Some(Direction::Forward))
            .expect("no-op convert");
        assert_eq!(
            same.file("doc_txt").expect("file").direction,
            Direction::Forward
        );
        assert_eq!(
            all_indices(&original, "doc_txt"),
            all_indices(&same, "doc_txt")
        );
    }
}
