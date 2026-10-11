//! FR-W9: replay a standalone `.wchg` (parsed header + records) over a base,
//! WITHOUT a parent manifest (FORMAT.md section 6, standalone-reader conformance).
//!
//! A bare `.wchg` is self-describing: its header carries the replay `direction`,
//! so a reader can reconstruct any version from `(header, records, base)` alone.
//! This reuses the single apply path ([`crate::reconstruct::reconstruct_directional`],
//! INV-4) and the single addressing path ([`crate::version::resolve_standalone`],
//! here with an EMPTY checkpoints slice). It is `pub(crate)`, not a new public
//! surface item.

use crate::record::ChangeRecord;
use crate::wchg::WchgHeader;
use crate::{reconstruct, version, Result};

/// Replay a standalone `.wchg` (parsed `header` + `records`) over `base` to the
/// requested `version`, using the header's own `direction`.
///
/// A standalone log has NO manifest and therefore NO checkpoints, so
/// [`crate::Version::Label`] is UNSUPPORTED here and resolves to
/// [`crate::WalrusError::UnknownCheckpoint`] for any label. All other selectors
/// ([`crate::Version::Index`], `Current`, `Original`, `Relative`, `Timestamp`)
/// resolve normally against the splice `count` and the records' own timestamps.
///
/// # Errors
/// Returns [`crate::WalrusError::VersionRange`] for an out-of-range index,
/// [`crate::WalrusError::UnknownCheckpoint`] for any [`crate::Version::Label`]
/// (no checkpoint source standalone), and
/// [`crate::WalrusError::SpliceOutOfBounds`] if a record is inconsistent with
/// the base it applies to.
// FR-W9 exposes the standalone replay path as crate-internal API, exercised by
// the tests below; no non-test caller consumes it yet (a future standalone
// `.wchg` reader / an FFWB embedder will), so the non-test build would flag it
// as dead. The allow keeps the deliberate seam without widening the public API.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn reconstruct_standalone(
    header: &WchgHeader,
    records: &[ChangeRecord],
    base: &[u8],
    version: crate::Version,
) -> Result<Vec<u8>> {
    let count = records.iter().filter(|r| r.is_splice()).count();
    // EMPTY checkpoints: there is no checkpoint source without a manifest, so a
    // Label selector resolves to UnknownCheckpoint (correct, FR-W9 / M-2).
    let index = version::resolve_standalone(count, &[], records, version)?;
    reconstruct::reconstruct_directional(base, records, header.direction, index, count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::ChangeRecord;
    use crate::wchg::{Direction, WchgHeader};
    use crate::{Version, WalrusError};

    /// A forward standalone history over the ORIGINAL base "cat": the forward
    /// log appends toward the current "the cats". count = 2.
    fn forward_header_and_log() -> (WchgHeader, Vec<ChangeRecord>, Vec<u8>) {
        let log = vec![
            ChangeRecord::splice(3, 0, "s"),    // "cat" -> "cats"
            ChangeRecord::splice(0, 0, "the "), // "cats" -> "the cats"
        ];
        let header = WchgHeader::new(Direction::Forward, 0, "doc.txt", "utf-8", log.len());
        (header, log, b"cat".to_vec())
    }

    /// The reverse standalone equivalent of the SAME history: base = current
    /// "the cats"; records walk current -> original. count = 2.
    fn reverse_header_and_log() -> (WchgHeader, Vec<ChangeRecord>, Vec<u8>) {
        let log = vec![
            ChangeRecord::splice(0, 4, ""), // "the cats" -> "cats"
            ChangeRecord::splice(3, 1, ""), // "cats" -> "cat"
        ];
        let header = WchgHeader::new(Direction::Reverse, 0, "doc.txt", "utf-8", log.len());
        (header, log, b"the cats".to_vec())
    }

    #[test]
    fn standalone_forward_log_index0_current_indexcount_original() {
        // A forward standalone log reconstructs with NO manifest: index 0 is the
        // current, index count is the original base.
        let (header, log, base) = forward_header_and_log();
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Index(0)).expect("current"),
            b"the cats"
        );
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Index(2)).expect("original"),
            b"cat"
        );
        // Current/Original selectors agree with the explicit indices.
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Current).expect("current sel"),
            b"the cats"
        );
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Original).expect("original sel"),
            b"cat"
        );
    }

    #[test]
    fn standalone_reverse_log_index0_current_indexcount_original() {
        // The reverse standalone twin reconstructs byte-identically (INV-3):
        // index 0 == current, index count == original, same bytes as forward.
        let (header, log, base) = reverse_header_and_log();
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Index(0)).expect("current"),
            b"the cats"
        );
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Index(2)).expect("original"),
            b"cat"
        );
    }

    #[test]
    fn standalone_label_without_manifest_returns_unknown_checkpoint() {
        // There is no checkpoint source standalone, so ANY Label resolves to
        // UnknownCheckpoint (M-2).
        let (header, log, base) = forward_header_and_log();
        let err = reconstruct_standalone(&header, &log, &base, Version::Label("milestone".into()))
            .expect_err("a label is unsupported standalone");
        assert!(matches!(err, WalrusError::UnknownCheckpoint(_)));
    }

    #[test]
    fn standalone_index_relative_timestamp_resolve_without_manifest() {
        // Index / Relative / Timestamp all resolve standalone (they need only
        // count and the records' own timestamps).
        let (mut header, mut log, base) = forward_header_and_log();
        // Attach ascending timestamps so Timestamp resolution has data.
        log[0].ts = Some("2026-01-01T00:00:05Z".to_string());
        log[1].ts = Some("2026-01-01T00:00:09Z".to_string());
        header.count = log.iter().filter(|r| r.is_splice()).count();

        // Relative(0) == current, Relative(count) == original.
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Relative(0)).expect("rel 0"),
            b"the cats"
        );
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Relative(2)).expect("rel count"),
            b"cat"
        );
        // A mid index resolves normally.
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Index(1)).expect("index 1"),
            b"cats"
        );
        // A timestamp before the earliest usable record resolves to current.
        let before = crate::timestamp::parse_ts_epoch("2026-01-01T00:00:00Z").expect("parse");
        assert_eq!(
            reconstruct_standalone(&header, &log, &base, Version::Timestamp(before))
                .expect("timestamp before earliest = current"),
            b"the cats"
        );
    }
}
