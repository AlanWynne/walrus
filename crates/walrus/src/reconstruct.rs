//! Reconstruct a document version by replaying splice records over the base.

use crate::record::{ChangeRecord, Op};
use crate::{Result, WalrusError};

/// Rebuild the document at `target_index` by applying the first `target_index`
/// splice records to `base`, in order.
///
/// Checkpoint records are skipped (they carry integrity hashes, not mutations).
/// The caller has already validated that `target_index <= number of records`.
pub(crate) fn reconstruct(
    base: &[u8],
    log: &[ChangeRecord],
    target_index: usize,
) -> Result<Vec<u8>> {
    let mut doc = base.to_vec();
    let mut applied = 0usize;

    for record in log {
        if applied == target_index {
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
}
