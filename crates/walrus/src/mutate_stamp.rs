//! Timestamp-stamping save seam (FR-W11 AC8): `add`/`update` variants that
//! stamp each recorded change with a caller-supplied UTC instant.
//!
//! The library is deliberately CLOCK-FREE (the same determinism rule FR-W5
//! followed by taking the rebase `last` timestamp as a parameter): the CLI
//! reads the wall clock and passes the formatted string in; this module only
//! validates its shape and threads it onto the recorded splice via the shared
//! `store_save` path in `mutate_add`. The on-disk format is unchanged -- the
//! record `ts` field already exists (FORMAT.md 4.1); these methods only set it.

use crate::archive::Archive;
use crate::wchg::Direction;
use crate::{Result, WalrusError};

impl Archive {
    /// Like [`Archive::add`], but STAMP the recorded change with `ts`, a UTC
    /// instant in the fixed shape `YYYY-MM-DDThh:mm:ssZ`. On a CREATE (first
    /// version) nothing is recorded, so no stamp is applied; the first real save
    /// that records a splice carries `ts`. A no-change save records nothing (ts
    /// unused) and is a success.
    ///
    /// # Errors
    /// Returns [`WalrusError::InvalidTimestamp`] when `ts` is not the fixed UTC
    /// shape, [`WalrusError::BinaryInput`] when `bytes` is not valid UTF-8, and
    /// otherwise as [`Archive::add`].
    pub fn add_at(
        &mut self,
        name: &str,
        bytes: &[u8],
        generations: Option<u32>,
        dir: Option<Direction>,
        ts: &str,
    ) -> Result<()> {
        validate_ts(ts)?;
        let text = crate::mutate_add::utf8_or_binary(bytes)?;

        if let Some(id) = self.id_for_selector(name) {
            self.store_save_from_text(&id, text, Some(ts))?;
            if let Some(n) = generations {
                self.set_generations_by_id(&id, n)?;
            }
            return Ok(());
        }
        // No existing file: a create records nothing, so there is no record to
        // stamp. Delegate to the plain create path.
        self.add(name, bytes, generations, dir)
    }

    /// Like [`Archive::update`], but STAMP the recorded change with `ts`. A
    /// no-change save records nothing (ts unused) and is a success. Gated behind
    /// the `diff` feature.
    ///
    /// # Errors
    /// Returns [`WalrusError::InvalidTimestamp`] when `ts` is not the fixed UTC
    /// shape, [`WalrusError::NotTracked`] when `name` is absent, and
    /// [`WalrusError::BinaryInput`] when `bytes` is not valid UTF-8.
    #[cfg(feature = "diff")]
    pub fn update_at(
        &mut self,
        name: &str,
        bytes: &[u8],
        generations: Option<u32>,
        ts: &str,
    ) -> Result<()> {
        validate_ts(ts)?;
        self.update_inner(name, bytes, generations, Some(ts))
    }

    /// Record a tracked file's source-file last-modified stamp into its manifest
    /// `attributes.mtime` (FORMAT.md section 3). Metadata only: the library stays
    /// clock-free and filesystem-free -- `mtime` is a pre-formatted UTC string the
    /// CLI supplies, exactly like the `ts` seam. Sets ONLY `attributes.mtime`; no
    /// other attribute, record, base, or version is touched, so it never perturbs
    /// reconstruction (FORMAT.md section 5 / INV-3).
    ///
    /// # Errors
    /// Returns [`WalrusError::NotTracked`] when `name` matches no tracked file.
    pub fn set_attributes_mtime(&mut self, name: &str, mtime: &str) -> Result<()> {
        let id = self
            .id_for_selector(name)
            .ok_or_else(|| WalrusError::NotTracked {
                name: name.to_string(),
            })?;
        let tracked = self
            .manifest_mut()
            .files
            .iter_mut()
            .find(|f| f.id == id)
            .ok_or_else(|| WalrusError::NotTracked {
                name: name.to_string(),
            })?;
        tracked.attributes.mtime = Some(mtime.to_string());
        Ok(())
    }
}

/// Validate a caller-supplied record timestamp against the fixed UTC shape
/// `YYYY-MM-DDThh:mm:ssZ` the writer and the `--at` resolver require. Reuses the
/// single library parser so there is ONE shape definition.
///
/// # Errors
/// Returns [`WalrusError::InvalidTimestamp`] for any other shape.
fn validate_ts(ts: &str) -> Result<()> {
    if crate::timestamp::parse_ts_epoch(ts).is_some() {
        Ok(())
    } else {
        Err(WalrusError::InvalidTimestamp {
            value: ts.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, Variant};
    use crate::record::Op;

    fn empty_archive(variant: Variant) -> Archive {
        Archive::from_parts_for_test(Manifest::new(variant), Vec::new())
    }

    /// The `ts` of the first (newest) record of a tracked file, if any.
    fn first_record_ts(archive: &Archive, sel: &str) -> Option<String> {
        archive
            .records(sel)
            .and_then(|r| r.first())
            .and_then(|rec| rec.ts.clone())
    }

    #[test]
    fn add_at_stamps_the_recorded_change_with_the_supplied_utc_timestamp() {
        // A fixed UTC string (never the wall clock) so the test is deterministic.
        let ts = "2026-10-11T16:18:44Z";
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("create");
        // A create records nothing; the first save that records a splice must
        // carry the supplied timestamp on that record.
        a.add_at("doc.txt", b"hello world", None, None, ts)
            .expect("add_at");
        assert_eq!(first_record_ts(&a, "doc.txt").as_deref(), Some(ts));
    }

    #[cfg(feature = "diff")]
    #[test]
    fn update_at_stamps_the_recorded_change_with_the_supplied_utc_timestamp() {
        let ts = "2026-10-11T16:18:44Z";
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("create");
        a.update_at("doc.txt", b"hello!", None, ts)
            .expect("update_at");
        assert_eq!(first_record_ts(&a, "doc.txt").as_deref(), Some(ts));
    }

    #[test]
    fn stamping_a_reverse_file_also_carries_the_timestamp() {
        let ts = "2026-10-11T16:18:44Z";
        let mut a = empty_archive(Variant::Wbl);
        a.add("doc.txt", b"abc", None, Some(Direction::Reverse))
            .expect("create reverse");
        a.add_at("doc.txt", b"abcX", None, None, ts)
            .expect("add_at reverse");
        // The inserted reverse record(s) representing this save carry the ts.
        assert_eq!(first_record_ts(&a, "doc.txt").as_deref(), Some(ts));
    }

    #[test]
    fn an_ill_shaped_timestamp_is_rejected_and_the_file_is_unmutated() {
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("create");
        let err = a
            .add_at("doc.txt", b"hello world", None, None, "11/10/2026 16:18")
            .expect_err("a non-fixed-shape ts must be rejected");
        assert!(matches!(err, WalrusError::InvalidTimestamp { .. }));
        // Nothing was recorded: the file still has no splice records.
        assert!(a.records("doc.txt").expect("records").is_empty());
    }

    #[test]
    fn a_no_change_stamped_save_records_nothing_and_is_ok() {
        let ts = "2026-10-11T16:18:44Z";
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("create");
        a.add_at("doc.txt", b"hello", None, None, ts)
            .expect("no-op stamped save is ok");
        assert!(a.records("doc.txt").expect("records").is_empty());
    }

    #[test]
    fn set_attributes_mtime_sets_only_the_mtime_attribute() {
        let ts = "2026-10-11T16:18:44Z";
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("create");
        a.set_attributes_mtime("doc.txt", ts).expect("set mtime");
        let tracked = a.file("doc.txt").expect("tracked");
        assert_eq!(tracked.attributes.mtime.as_deref(), Some(ts));
        // Only mtime is touched: size and mode stay as the default (None).
        assert!(tracked.attributes.size.is_none(), "size untouched");
        assert!(tracked.attributes.mode.is_none(), "mode untouched");
    }

    #[test]
    fn set_attributes_mtime_on_untracked_returns_not_tracked() {
        let mut a = empty_archive(Variant::Wal);
        let err = a
            .set_attributes_mtime("nope", "2026-10-11T16:18:44Z")
            .expect_err("absent name must error");
        assert!(matches!(err, WalrusError::NotTracked { .. }));
    }

    #[test]
    fn the_stamped_record_resolves_under_at_timestamp_addressing() {
        // The whole point of AC8: a stamped record is findable by --at.
        let ts = "2026-10-11T16:18:44Z";
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("create");
        a.add_at("doc.txt", b"hello world", None, None, ts)
            .expect("add_at");
        // The record is a splice carrying the ts, so resolve_timestamp can see it.
        let records = a.records("doc.txt").expect("records");
        assert_eq!(records.len(), 1);
        assert!(matches!(records[0].op, Op::Splice { .. }));
        assert_eq!(records[0].ts.as_deref(), Some(ts));
    }
}
