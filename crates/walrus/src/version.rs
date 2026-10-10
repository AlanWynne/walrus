//! Version addressing: resolve a [`Version`] selector to a concrete per-file
//! index (FORMAT.md section 5).
//!
//! Addressing is PER TRACKED FILE. Each file has its own index range
//! `[0, count]` where `count` is the number of SPLICE records in its `.wchg`.
//! INDEX 0 IS ALWAYS THE CURRENT version for both directions; higher indices
//! are older, and `count` is the far (oldest) end = the ORIGINAL. Resolution is
//! done against a single file's `count`, its checkpoints, and its records'
//! timestamps; the archive variant is never consulted.

use crate::manifest::TrackedFile;
use crate::record::ChangeRecord;
use crate::timestamp::parse_ts_epoch;
use crate::{Result, WalrusError};

/// A way of addressing one version within a single tracked file.
///
/// All selectors resolve to an integer index in that file's `[0, count]`, where
/// index 0 is the CURRENT version and `count` is the far (oldest) end = the
/// ORIGINAL. Higher indices are older.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Version {
    /// An explicit index (canonical form). Index 0 is current, `count` is the
    /// original; an index greater than `count` is a typed range error.
    Index(usize),
    /// The current version (index 0).
    Current,
    /// The original (far) version (index `count`).
    Original,
    /// A signed offset relative to the CURRENT version (index 0). A POSITIVE
    /// `n` moves OLDER, toward the original (because a higher index is older);
    /// a NEGATIVE `n` moves toward the current and is clamped at 0. The result
    /// is clamped into `[0, count]` at BOTH ends. So `Relative(0) == Current`,
    /// `Relative(count) == Original`, `Relative(count + k)` clamps to `count`,
    /// and `Relative(-k)` clamps to 0.
    Relative(i64),
    /// A named checkpoint, resolved against this file's own checkpoint list to
    /// an index in the file's own `[0, count]` space.
    Label(String),
    /// A timestamp (epoch seconds). Resolves to the index whose record best
    /// matches it; a timestamp before the earliest record resolves to index 0
    /// (current). See [`resolve_file`] for the full rule.
    Timestamp(i64),
}

/// Resolve a selector against a single tracked file.
///
/// `file` provides the checkpoints (for [`Version::Label`]); `records` provides
/// the timestamps (for [`Version::Timestamp`]); `count` is the number of SPLICE
/// records (the far index). Resolution rules per selector are documented on the
/// [`Version`] variants. The result is always in `[0, count]`.
///
/// # Errors
/// Returns [`WalrusError::VersionRange`] for an out-of-range [`Version::Index`]
/// and [`WalrusError::UnknownCheckpoint`] for an unknown [`Version::Label`].
pub(crate) fn resolve_file(
    file: &TrackedFile,
    records: &[ChangeRecord],
    count: usize,
    version: Version,
) -> Result<usize> {
    match version {
        Version::Current => Ok(0),
        Version::Original => Ok(count),
        Version::Index(i) => {
            if i > count {
                return Err(WalrusError::VersionRange {
                    requested: i,
                    max: count,
                });
            }
            Ok(i)
        }
        // Relative to current (index 0): positive moves older (higher index),
        // negative clamps to 0; the sum is clamped into [0, count] at both ends.
        Version::Relative(n) => Ok(clamp_i64(n, count)),
        Version::Label(label) => file
            .checkpoints
            .iter()
            .find(|c| c.label == label)
            .map(|c| c.index.min(count))
            .ok_or(WalrusError::UnknownCheckpoint(label)),
        Version::Timestamp(t) => Ok(resolve_timestamp(records, count, t)),
    }
}

/// Clamp a signed offset-from-current into the valid index range `[0, count]`.
/// A negative value clamps to 0 (current); a value above `count` clamps to the
/// original end.
fn clamp_i64(n: i64, count: usize) -> usize {
    if n <= 0 {
        0
    } else if (n as u128) >= count as u128 {
        count
    } else {
        n as usize
    }
}

/// Resolve a [`Version::Timestamp`] against a file's records.
///
/// The index/record correspondence (FORMAT.md section 4.1): record `i` (1-based
/// from the first record line) transforms version `i-1` into version `i` ALONG
/// THE LOG'S DECLARED DIRECTION. For index addressing the resolver works in the
/// file's canonical `[0, count]` space where index 0 is current and `count` is
/// the original, so the matching walks records in reverse-chronological order:
/// index `k` corresponds to the (`count - k`)-th most recent splice. Records
/// carry an optional ISO-8601 `ts`; those that do not parse are skipped. We
/// pick the index whose record timestamp is the newest `<= t`. A `t` before the
/// earliest usable timestamp (or no usable timestamp at all) resolves to index
/// 0 (current). The result is clamped into `[0, count]`.
fn resolve_timestamp(records: &[ChangeRecord], count: usize, t: i64) -> usize {
    // Collect (splice-index-in-[1..=count], epoch) for every splice record that
    // carries a parseable timestamp, in splice order (oldest-applied first is
    // record order along the log). We only need relative ordering of the epoch
    // values to pick the newest at-or-before `t`.
    let mut best_splice: Option<usize> = None;
    let mut splice_ordinal = 0usize; // 1-based position among splices
    for record in records {
        if !record.is_splice() {
            continue;
        }
        splice_ordinal += 1;
        let epoch = match record.ts.as_deref().and_then(parse_ts_epoch) {
            Some(e) => e,
            None => continue,
        };
        if epoch <= t {
            best_splice = Some(splice_ordinal);
        }
    }

    // `best_splice` (1-based ordinal among splices, oldest step first) is the
    // newest forward step whose timestamp is at-or-before `t`. FORMAT.md 4.1:
    // record `i` yields forward-version `i`, whose canonical index (0 = current,
    // `count` = original) is `count - i`. So the matched index is `count - i`.
    // No match (t before the earliest usable timestamp, or no usable timestamp)
    // resolves to index 0 (current) per FORMAT.md section 5.
    match best_splice {
        Some(ordinal) => count.saturating_sub(ordinal).min(count),
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Checkpoint, Variant};
    use crate::record::ChangeRecord;

    fn file_with_checkpoints() -> TrackedFile {
        let mut f = TrackedFile::new("f0", "doc.txt", Variant::Wal, 10);
        f.checkpoints = vec![Checkpoint {
            index: 4,
            label: "milestone".to_string(),
        }];
        f
    }

    #[test]
    fn resolve_file_maps_current_and_original_to_the_range_ends() {
        let f = TrackedFile::new("f0", "doc.txt", Variant::Wal, 10);
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Current).expect("current"),
            0
        );
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Original).expect("original"),
            10
        );
    }

    #[test]
    fn resolve_file_relative_clamps_into_range() {
        let f = TrackedFile::new("f0", "doc.txt", Variant::Wal, 10);
        // Relative(0) is current; a mid positive value moves older; both ends clamp.
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Relative(0)).expect("zero"),
            0
        );
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Relative(3)).expect("mid"),
            3
        );
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Relative(10)).expect("count"),
            10
        );
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Relative(99)).expect("over"),
            10
        );
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Relative(-5)).expect("under"),
            0
        );
    }

    #[test]
    fn resolve_file_index_out_of_range_is_a_typed_error() {
        let f = TrackedFile::new("f0", "doc.txt", Variant::Wal, 3);
        assert!(matches!(
            resolve_file(&f, &[], 3, Version::Index(4)),
            Err(WalrusError::VersionRange {
                requested: 4,
                max: 3
            })
        ));
    }

    #[test]
    fn resolve_file_label_resolves_and_rejects_unknown() {
        let f = file_with_checkpoints();
        assert_eq!(
            resolve_file(&f, &[], 10, Version::Label("milestone".into())).expect("known"),
            4
        );
        assert!(matches!(
            resolve_file(&f, &[], 10, Version::Label("nope".into())),
            Err(WalrusError::UnknownCheckpoint(_))
        ));
    }

    #[test]
    fn resolve_file_timestamp_before_earliest_resolves_to_current() {
        // Three splices with ascending timestamps; a `t` before all of them
        // resolves to index 0 (current).
        let records = vec![
            ts_splice("2026-01-01T00:00:03Z"), // record 1 (oldest step)
            ts_splice("2026-01-01T00:00:05Z"),
            ts_splice("2026-01-01T00:00:07Z"),
        ];
        let f = TrackedFile::new("f0", "doc.txt", Variant::Wal, 3);
        let before = parse_ts_epoch("2026-01-01T00:00:00Z").expect("parse");
        assert_eq!(
            resolve_file(&f, &records, 3, Version::Timestamp(before)).expect("ts"),
            0
        );
    }

    fn ts_splice(ts: &str) -> ChangeRecord {
        let mut r = ChangeRecord::splice(0, 0, "x");
        r.ts = Some(ts.to_string());
        r
    }
}
