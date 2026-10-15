//! The `manifest.json` container member (multi-file layout, FORMAT.md section 3).
//!
//! The manifest describes the archive as a whole and carries a `files` array
//! with one [`TrackedFile`] entry per tracked document. The `files` array is the
//! authoritative directory of tracked files; the tar member layout
//! (`files/<id>/base`, `files/<id>/<name>.wchg`) is derived from it.

use serde::{Deserialize, Serialize};

use crate::wchg::Direction;

/// Which variant of the format an archive is. Always a concrete `wal` or `wbl`
/// (never a third "mixed" value): the variant only seeds the DEFAULT direction
/// for newly added files and is never consulted for reconstruction. A single
/// archive may legally hold a mix of forward and reverse files; "mixedness" is
/// emergent, not stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    /// `.wal`: the DEFAULT for new files is an original base with a forward log.
    Wal,
    /// `.wbl`: the DEFAULT for new files is a current base with a reverse log.
    Wbl,
}

impl Variant {
    /// The DEFAULT replay direction this variant seeds for newly added files.
    /// It is not a per-file reconstruction rule: a file carries its own
    /// authoritative direction, which may differ from the variant.
    pub fn direction(self) -> Direction {
        match self {
            Variant::Wal => Direction::Forward,
            Variant::Wbl => Direction::Reverse,
        }
    }

    /// The archive-level `base_role` string this variant records
    /// (`"original"` / `"current"`); a default descriptor, not a per-file rule.
    pub fn base_role(self) -> &'static str {
        match self {
            Variant::Wal => "original",
            Variant::Wbl => "current",
        }
    }

    /// The archive-level `log_direction` string this variant records
    /// (`"forward"` / `"reverse"`); a default descriptor, not a per-file rule.
    pub fn log_direction(self) -> &'static str {
        match self {
            Variant::Wal => "forward",
            Variant::Wbl => "reverse",
        }
    }
}

/// Recorded original attributes of a tracked file (FORMAT.md section 3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileAttributes {
    /// Byte size of the original file, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Original modification time (ISO-8601), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtime: Option<String>,
    /// Original mode/permission string (e.g. `"0644"`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Per-file version-count bookkeeping.
///
/// Index 0 is ALWAYS the current version; higher indices are older. Base/head
/// follow the FILE's OWN direction, not the archive variant. For a forward file
/// the base is the original, so `base_index = count` and `head_index = 0`. For
/// a reverse file the base is the current, so `base_index = 0` and
/// `head_index = count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    /// Number of change records in this file's `.wchg` log.
    pub count: usize,
    /// The index the stored base represents.
    pub base_index: usize,
    /// The opposite end of the index range.
    pub head_index: usize,
}

impl Versions {
    /// Build the version bookkeeping for a file with `count` records replaying
    /// in the given direction, applying the spec's base/head convention. A
    /// forward file has `base_index = count` and `head_index = 0`; a reverse
    /// file has `base_index = 0` and `head_index = count`. Base/head follow the
    /// FILE's OWN direction, never the archive variant.
    pub fn for_direction(direction: Direction, count: usize) -> Self {
        match direction {
            Direction::Forward => Versions {
                count,
                base_index: count,
                head_index: 0,
            },
            Direction::Reverse => Versions {
                count,
                base_index: 0,
                head_index: count,
            },
        }
    }

    /// Build the version bookkeeping for the variant's DEFAULT direction. This
    /// seeds the default for newly added files; it does not constrain a file,
    /// which carries its own direction. Delegates to [`Versions::for_direction`].
    pub fn for_variant(variant: Variant, count: usize) -> Self {
        Self::for_direction(variant.direction(), count)
    }
}

/// A named, human-addressable version index within a file's own index space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The version index this checkpoint names (in `0..=count`).
    pub index: usize,
    /// A human label, e.g. "before refactor".
    pub label: String,
}

/// Optional audit metadata for history dropped by `condense` or the generations
/// rolling-window trim (FORMAT.md section 3). Re-base leaves no log marker; this
/// records the fact instead. Informational only; does not affect reconstruction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebaseInfo {
    /// How many re-base operations have occurred for this file.
    pub count: u32,
    /// ISO timestamp of the most recent re-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
    /// Total versions discarded by re-basing.
    pub dropped: u32,
}

/// One entry of the manifest `files` array: a tracked document and its history
/// bookkeeping (FORMAT.md section 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedFile {
    /// Stable, archive-unique, filesystem-safe id matching `files/<id>/`.
    pub id: String,
    /// The original filename and relative path as supplied (may contain `/`).
    pub name: String,
    /// Declared text encoding of this file's base and reconstructed versions.
    pub encoding: String,
    /// MIME type (e.g. `text/plain`, `text/csv`, `text/markdown`).
    pub mime: String,
    /// Recorded original file attributes.
    #[serde(default)]
    pub attributes: FileAttributes,
    /// This file's OWN replay direction, authoritative for how it reconstructs
    /// and for its base/head indices. It may differ from the archive variant;
    /// the variant only seeds the default when a new file is added.
    pub direction: Direction,
    /// Version counts for this file.
    pub versions: Versions,
    /// Retention cap mirrored from the `.wchg` header: `0` = unlimited.
    #[serde(default)]
    pub generations: u32,
    /// Optional named indices for human addressing (indices in `[0, count]`).
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    /// Optional re-base audit metadata; absent means never re-based.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebase: Option<RebaseInfo>,
}

impl TrackedFile {
    /// Build a minimal tracked-file entry whose direction defaults to the
    /// archive variant's direction (the seed for a newly added file), defaulting
    /// encoding/mime and leaving attributes/checkpoints empty.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        variant: Variant,
        count: usize,
    ) -> Self {
        Self::new_with_direction(id, name, variant.direction(), count)
    }

    /// Build a minimal tracked-file entry with an explicit per-file direction,
    /// deriving base/head from that direction. Use this when a file's direction
    /// differs from the archive variant's default.
    pub fn new_with_direction(
        id: impl Into<String>,
        name: impl Into<String>,
        direction: Direction,
        count: usize,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            encoding: "utf-8".to_string(),
            mime: "text/plain".to_string(),
            attributes: FileAttributes::default(),
            direction,
            versions: Versions::for_direction(direction, count),
            generations: 0,
            checkpoints: Vec::new(),
            rebase: None,
        }
    }
}

/// The parsed `manifest.json` (archive-level fields plus the `files` array).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always `"walrus"`.
    pub format: String,
    /// Spec version string, e.g. `"0.1"`.
    pub spec_version: String,
    /// `wal` or `wbl`.
    pub variant: Variant,
    /// `"original"` for `.wal`, `"current"` for `.wbl`.
    pub base_role: String,
    /// `"forward"` for `.wal`, `"reverse"` for `.wbl`.
    pub log_direction: String,
    /// ISO-8601 creation timestamp (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Generator identifier (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<String>,
    /// The tracked files in this archive (authoritative directory). May be empty.
    #[serde(default)]
    pub files: Vec<TrackedFile>,
}

impl Manifest {
    /// Current spec version this crate reads and writes.
    pub const SPEC_VERSION: &'static str = "0.1";

    /// Build an empty manifest for the given variant (zero tracked files).
    pub fn new(variant: Variant) -> Self {
        Self {
            format: "walrus".to_string(),
            spec_version: Self::SPEC_VERSION.to_string(),
            variant,
            base_role: variant.base_role().to_string(),
            log_direction: variant.log_direction().to_string(),
            created: None,
            generator: None,
            files: Vec::new(),
        }
    }

    /// The checkpoints of the first tracked file, or an empty slice when the
    /// archive has no files. Checkpoints are a per-file property on
    /// [`TrackedFile`]; this is a convenience accessor for a single-file
    /// archive.
    pub fn checkpoints(&self) -> &[Checkpoint] {
        self.files
            .first()
            .map(|f| f.checkpoints.as_slice())
            .unwrap_or(&[])
    }

    /// Look up a tracked file by its id.
    pub fn file(&self, id: &str) -> Option<&TrackedFile> {
        self.files.iter().find(|f| f.id == id)
    }

    /// Validate structural invariants across the archive and every tracked file.
    ///
    /// Checks the format tag and spec compatibility, and that the archive-level
    /// `base_role` and `log_direction` descriptors match the variant. For each
    /// file it checks that the version bookkeeping and checkpoints are
    /// consistent with the FILE's OWN direction. A file's direction is NOT
    /// required to match the archive variant: a mixed archive is legal.
    pub fn validate(&self) -> crate::Result<()> {
        if self.format != "walrus" {
            return Err(crate::WalrusError::Manifest(format!(
                "unexpected format tag '{}'",
                self.format
            )));
        }
        if self.spec_version != Self::SPEC_VERSION {
            return Err(crate::WalrusError::Manifest(format!(
                "incompatible spec_version '{}' (this build reads {})",
                self.spec_version,
                Self::SPEC_VERSION
            )));
        }
        if self.base_role != self.variant.base_role() {
            return Err(crate::WalrusError::Manifest(format!(
                "base_role '{}' disagrees with variant {:?}",
                self.base_role, self.variant
            )));
        }
        if self.log_direction != self.variant.log_direction() {
            return Err(crate::WalrusError::Manifest(format!(
                "log_direction '{}' disagrees with variant {:?}",
                self.log_direction, self.variant
            )));
        }
        for file in &self.files {
            self.validate_file(file)?;
        }
        Ok(())
    }

    /// Validate a single tracked-file entry against its OWN direction.
    ///
    /// Base/head must follow the file's own direction, never the archive
    /// variant: a reverse file in a wal archive is legal.
    fn validate_file(&self, file: &TrackedFile) -> crate::Result<()> {
        let expected = Versions::for_direction(file.direction, file.versions.count);
        if file.versions.base_index != expected.base_index
            || file.versions.head_index != expected.head_index
        {
            return Err(crate::WalrusError::Manifest(format!(
                "file '{}' base/head index {}/{} disagree with its direction {:?} (expected {}/{})",
                file.name,
                file.versions.base_index,
                file.versions.head_index,
                file.direction,
                expected.base_index,
                expected.head_index
            )));
        }
        for cp in &file.checkpoints {
            if cp.index > file.versions.count {
                return Err(crate::WalrusError::Manifest(format!(
                    "file '{}' checkpoint '{}' index {} exceeds count {}",
                    file.name, cp.label, cp.index, file.versions.count
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file(variant: Variant, count: usize) -> TrackedFile {
        TrackedFile::new("a1b2c3", "src/document.txt", variant, count)
    }

    #[test]
    fn manifest_round_trips_with_zero_files() {
        let m = Manifest::new(Variant::Wal);
        let json = serde_json::to_string(&m).expect("serialise");
        let back: Manifest = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, m);
        assert!(back.files.is_empty());
    }

    #[test]
    fn manifest_round_trips_with_one_file() {
        let mut m = Manifest::new(Variant::Wal);
        m.files.push(sample_file(Variant::Wal, 42));
        let json = serde_json::to_string(&m).expect("serialise");
        let back: Manifest = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, m);
    }

    #[test]
    fn manifest_round_trips_with_many_files() {
        let mut m = Manifest::new(Variant::Wbl);
        m.files
            .push(TrackedFile::new("f1", "a.txt", Variant::Wbl, 3));
        m.files
            .push(TrackedFile::new("f2", "b/c.md", Variant::Wbl, 0));
        m.files
            .push(TrackedFile::new("f3", "d.csv", Variant::Wbl, 100));
        let json = serde_json::to_string(&m).expect("serialise");
        let back: Manifest = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, m);
        assert_eq!(back.files.len(), 3);
    }

    #[test]
    fn wal_base_and_head_indices_follow_the_spec() {
        let v = Versions::for_variant(Variant::Wal, 42);
        assert_eq!(v.base_index, 42, "wal base is the original at count");
        assert_eq!(v.head_index, 0, "wal head (current) is index 0");
    }

    #[test]
    fn wbl_base_and_head_indices_follow_the_spec() {
        let v = Versions::for_variant(Variant::Wbl, 42);
        assert_eq!(v.base_index, 0, "wbl base is the current at index 0");
        assert_eq!(v.head_index, 42, "wbl head (original) is at count");
    }

    #[test]
    fn validate_accepts_a_consistent_wal_manifest() {
        let mut m = Manifest::new(Variant::Wal);
        m.files.push(sample_file(Variant::Wal, 42));
        m.validate().expect("consistent manifest validates");
    }

    #[test]
    fn validate_rejects_a_checkpoint_index_past_count() {
        let mut m = Manifest::new(Variant::Wal);
        let mut file = sample_file(Variant::Wal, 5);
        file.checkpoints.push(Checkpoint {
            index: 6,
            label: "too far".to_string(),
        });
        m.files.push(file);
        let err = m
            .validate()
            .expect_err("checkpoint past count must be rejected");
        assert!(matches!(err, crate::WalrusError::Manifest(_)));
    }

    #[test]
    fn a_wal_archive_may_mix_forward_and_reverse_files() {
        // The archive variant only seeds the default; a single archive may hold
        // a mix of forward and reverse files. Each file's base/head follows its
        // OWN direction, and the mixed archive validates with no error.
        let mut m = Manifest::new(Variant::Wal);
        m.files.push(TrackedFile::new_with_direction(
            "fwd",
            "forward.txt",
            Direction::Forward,
            10,
        ));
        m.files.push(TrackedFile::new_with_direction(
            "rev",
            "reverse.txt",
            Direction::Reverse,
            7,
        ));
        m.validate()
            .expect("a mixed forward/reverse archive is legal and validates");
    }

    #[test]
    fn base_head_contradicting_the_files_own_direction_is_rejected() {
        // A FORWARD file given wbl-style indices (base 0 / head count) must be
        // rejected: base/head must agree with the file's OWN direction.
        let mut m = Manifest::new(Variant::Wal);
        let mut file =
            TrackedFile::new_with_direction("fwd", "forward.txt", Direction::Forward, 10);
        file.versions = Versions {
            count: 10,
            base_index: 0,
            head_index: 10,
        };
        m.files.push(file);
        let err = m
            .validate()
            .expect_err("base/head contradicting the file's own direction must be rejected");
        assert!(matches!(err, crate::WalrusError::Manifest(_)));
    }

    #[test]
    fn a_reverse_header_is_legal_in_a_wal_archive() {
        // The removed agreement rule rejected a reverse header in a wal archive.
        // A file's direction is now authoritative and independent of the
        // variant: a reverse-direction file (and its reverse .wchg header) is
        // legal in a wal archive, and the archive validates with no error.
        let header = crate::WchgHeader::new(Direction::Reverse, 0, "reverse.txt", "utf-8", 7);
        assert_eq!(header.direction, Direction::Reverse);

        let mut m = Manifest::new(Variant::Wal);
        m.files.push(TrackedFile::new_with_direction(
            "rev",
            "reverse.txt",
            header.direction,
            header.count,
        ));
        m.validate()
            .expect("a reverse file is legal in a wal archive");
    }

    #[test]
    fn base_role_and_log_direction_match_the_variant() {
        let wal = Manifest::new(Variant::Wal);
        assert_eq!(wal.base_role, "original");
        assert_eq!(wal.log_direction, "forward");
        let wbl = Manifest::new(Variant::Wbl);
        assert_eq!(wbl.base_role, "current");
        assert_eq!(wbl.log_direction, "reverse");
    }
}
