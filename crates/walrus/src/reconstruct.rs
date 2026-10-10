//! Reconstruct a document version by replaying splice records over the base.

use crate::record::{ChangeRecord, Op};
use crate::wchg::Direction;
use crate::{Result, WalrusError};

/// Rebuild the document at `target_index` by applying the first `target_index`
/// splice records to `base`, in order (the OLD single-document, forward-only
/// convention where index 0 is the stored base and `len` is the far end). The
/// compatibility [`crate::WalrusFile`] path keeps this convention; the new
/// per-file, direction-aware engine is [`reconstruct_directional`].
///
/// Checkpoint records are skipped (they carry integrity hashes, not mutations).
/// The caller has already validated that `target_index <= number of records`.
pub(crate) fn reconstruct(
    base: &[u8],
    log: &[ChangeRecord],
    target_index: usize,
) -> Result<Vec<u8>> {
    // The old forward-only "apply first `target_index`" shares the apply loop
    // with the directional engine; only the index convention differs.
    apply_first_splices(base, log, target_index)
}

/// Reconstruct a version directionally: apply the first `splices_to_apply`
/// SPLICE records of `log` to `base`, in order, where the count depends on the
/// file's own `direction` and the resolved canonical `index` (0 = current,
/// `count` = original).
///
/// - FORWARD file: the base is the ORIGINAL (index `count`); to reach index
///   `index`, apply the first `count - index` splices.
/// - REVERSE file: the base is the CURRENT (index 0), records already inverted;
///   to reach index `index`, apply the first `index` splices.
///
/// Checkpoint records are skipped and do NOT advance the applied counter. The
/// caller has already validated `index` into `0..=count` via the resolver.
pub(crate) fn reconstruct_directional(
    base: &[u8],
    log: &[ChangeRecord],
    direction: Direction,
    index: usize,
    count: usize,
) -> Result<Vec<u8>> {
    let splices_to_apply = match direction {
        Direction::Forward => count.saturating_sub(index),
        Direction::Reverse => index,
    };
    apply_first_splices(base, log, splices_to_apply)
}

/// Apply the first `n` SPLICE records of `log` to `base` in order, skipping
/// checkpoint records (which do not advance the applied counter).
fn apply_first_splices(base: &[u8], log: &[ChangeRecord], n: usize) -> Result<Vec<u8>> {
    let mut doc = base.to_vec();
    let mut applied = 0usize;

    for record in log {
        if applied == n {
            break;
        }
        match &record.op {
            Op::Splice {
                at,
                delete_len,
                insert,
            } => {
                apply_splice(&mut doc, *at, *delete_len, insert.as_bytes())?;
                applied += 1;
            }
            // Checkpoint markers do not advance the version index.
            Op::Checkpoint { .. } => {}
        }
    }

    Ok(doc)
}

/// Apply one splice in place: at `at`, delete `delete_len` bytes, insert `ins`.
fn apply_splice(doc: &mut Vec<u8>, at: usize, delete_len: usize, ins: &[u8]) -> Result<()> {
    let end = at
        .checked_add(delete_len)
        .ok_or(WalrusError::SpliceOutOfBounds { at, len: doc.len() })?;
    if end > doc.len() {
        return Err(WalrusError::SpliceOutOfBounds {
            at: end,
            len: doc.len(),
        });
    }
    doc.splice(at..end, ins.iter().copied());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_in_order() {
        // "a" -> insert "b" at 1 -> "ab" -> insert "c" at 2 -> "abc"
        let base = b"a".to_vec();
        let log = vec![
            ChangeRecord::splice(1, 0, "b"),
            ChangeRecord::splice(2, 0, "c"),
        ];
        assert_eq!(reconstruct(&base, &log, 0).unwrap(), b"a");
        assert_eq!(reconstruct(&base, &log, 1).unwrap(), b"ab");
        assert_eq!(reconstruct(&base, &log, 2).unwrap(), b"abc");
    }

    #[test]
    fn delete_then_insert() {
        // "walrus" -> at 0 delete 6 insert "seal" -> "seal"
        let base = b"walrus".to_vec();
        let log = vec![ChangeRecord::splice(0, 6, "seal")];
        assert_eq!(reconstruct(&base, &log, 1).unwrap(), b"seal");
    }

    #[test]
    fn out_of_bounds_is_rejected() {
        let base = b"hi".to_vec();
        let log = vec![ChangeRecord::splice(5, 0, "x")];
        assert!(reconstruct(&base, &log, 1).is_err());
    }

    // === reconstruct_directional (per-file, direction-aware) ============

    #[test]
    fn directional_forward_index_zero_is_current_and_count_is_original() {
        // Forward file: base is the ORIGINAL "cat"; forward log appends to reach
        // the current "the cats". count = 2.
        let original = b"cat".to_vec();
        let log = vec![
            ChangeRecord::splice(3, 0, "s"),    // "cat" -> "cats"
            ChangeRecord::splice(0, 0, "the "), // "cats" -> "the cats"
        ];
        // index 0 (current) applies all count=2 splices.
        assert_eq!(
            reconstruct_directional(&original, &log, Direction::Forward, 0, 2).unwrap(),
            b"the cats"
        );
        // index count=2 (original) applies 0 splices = the base.
        assert_eq!(
            reconstruct_directional(&original, &log, Direction::Forward, 2, 2).unwrap(),
            b"cat"
        );
    }

    #[test]
    fn directional_reverse_index_zero_is_base_and_count_applies_all() {
        // Reverse file: base is the CURRENT "the cats"; reverse log walks back to
        // the original "cat". These reverse records are the inverse of the
        // forward ones, applied current -> original. count = 2.
        let current = b"the cats".to_vec();
        let log = vec![
            ChangeRecord::splice(0, 4, ""), // "the cats" -> "cats"
            ChangeRecord::splice(3, 1, ""), // "cats" -> "cat"
        ];
        // index 0 (current) applies 0 reverse splices = the base.
        assert_eq!(
            reconstruct_directional(&current, &log, Direction::Reverse, 0, 2).unwrap(),
            b"the cats"
        );
        // index count=2 (original) applies all reverse splices.
        assert_eq!(
            reconstruct_directional(&current, &log, Direction::Reverse, 2, 2).unwrap(),
            b"cat"
        );
    }
}
