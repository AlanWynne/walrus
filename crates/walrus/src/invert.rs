//! Splice-list inversion: turn a FORWARD-sense splice list applied to some
//! version into the REVERSE splice list that walks the post-apply bytes back to
//! that version (FR-W7 design section 4).
//!
//! This is the single production home of the inversion math that decisions (b)
//! (forward-vs-stored-direction storage) and (c) (`convert`) both depend on.
//! The two former `#[cfg(test)]` fixtures in `archive.rs`/`mutate.rs` are
//! collapsed onto it so there is ONE inversion implementation, test and
//! production alike. It never introduces a second apply path: it advances its
//! scratch copy through [`crate::reconstruct::apply_record`] (INV-4).

use crate::record::{ChangeRecord, Op};
use crate::{reconstruct, Result, WalrusError};

// === inversion helper ===================================================

/// Invert a forward-sense splice list applied to `version` into the reverse
/// splice list, returning `(forward_applied_bytes, reverse_records)`: the first
/// element is `version` with every forward splice applied (the reverse base),
/// and the second is the inverse records ordered newest-first, so applying them
/// to the forward-applied bytes walks back to `version`.
///
/// Only [`Op::Splice`] records drive the walk; [`Op::Checkpoint`] records are
/// SKIPPED (they carry no mutation and do not advance the version).
///
/// Given boundary-aligned forward splices (the `update_with_splices` boundary
/// rejects any splice that would split a multi-byte UTF-8 character), the
/// captured deleted bytes are valid UTF-8 by construction.
///
/// # Errors
/// Returns [`WalrusError::SpliceOutOfBounds`] when a forward splice references a
/// byte range outside the version it applies to (surfaced from
/// [`crate::reconstruct::apply_record`]), or when the captured deleted bytes are
/// not valid UTF-8 (an internal invariant breach the boundary check should have
/// caught: never a panic).
// FR-W7 FEAT-001 foundation: the production callers are the reverse-file
// storage path (FEAT-002) and `convert` (FEAT-003); until they land it is
// exercised only by this module's and archive.rs/mutate.rs test fixtures.
#[allow(dead_code)]
pub(crate) fn invert_forward_splices(
    version: &[u8],
    forward: &[ChangeRecord],
) -> Result<(Vec<u8>, Vec<ChangeRecord>)> {
    let mut scratch = version.to_vec();
    let mut inverses = Vec::new();

    for record in forward {
        let Op::Splice {
            at,
            delete_len,
            insert,
        } = &record.op
        else {
            // Checkpoints are not splices and do not advance the version.
            continue;
        };

        let end = at
            .checked_add(*delete_len)
            .ok_or(WalrusError::SpliceOutOfBounds {
                at: *at,
                len: scratch.len(),
            })?;
        if end > scratch.len() {
            return Err(WalrusError::SpliceOutOfBounds {
                at: end,
                len: scratch.len(),
            });
        }

        // The bytes about to be deleted become the inverse splice's insert.
        let captured = &scratch[*at..end];
        let captured_str =
            std::str::from_utf8(captured).map_err(|_| WalrusError::SpliceOutOfBounds {
                at: *at,
                len: scratch.len(),
            })?;
        inverses.push(ChangeRecord::splice(*at, insert.len(), captured_str));

        // Advance the scratch copy through the single apply path (INV-4).
        reconstruct::apply_record(&mut scratch, record)?;
    }

    // `scratch` is now the forward-applied bytes (the reverse base). The reverse
    // log undoes the last forward step first, so read the inverses newest-first.
    inverses.reverse();
    Ok((scratch, inverses))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconstruct::reconstruct_directional;
    use crate::wchg::Direction;

    /// A forward history over "abcdef" inverts to a reverse log that
    /// reconstructs byte-identically to the forward reconstruct at EVERY index.
    #[test]
    fn invert_forward_history_reconstructs_byte_identically_at_every_index() {
        let original = b"abcdef".to_vec();
        let forward = vec![
            ChangeRecord::splice(6, 0, "GH"),  // "abcdef" -> "abcdefGH"
            ChangeRecord::splice(0, 2, "XY"),  // -> "XYcdefGH"
            ChangeRecord::splice(4, 1, "ZZZ"), // -> "XYcdZZZfGH"
        ];
        let count = forward.iter().filter(|r| r.is_splice()).count();

        let (reverse_base, reverse_records) =
            invert_forward_splices(&original, &forward).expect("invert");

        for k in 0..=count {
            let forward_bytes =
                reconstruct_directional(&original, &forward, Direction::Forward, k, count)
                    .expect("forward reconstruct");
            let reverse_bytes = reconstruct_directional(
                &reverse_base,
                &reverse_records,
                Direction::Reverse,
                k,
                count,
            )
            .expect("reverse reconstruct");
            assert_eq!(
                forward_bytes, reverse_bytes,
                "forward and reverse reconstruct differ at index {k}"
            );
        }
    }

    /// A forward splice referencing a range past the end of the version it
    /// applies to surfaces a typed [`WalrusError::SpliceOutOfBounds`].
    #[test]
    fn invert_bad_offset_surfaces_splice_out_of_bounds() {
        let original = b"abc".to_vec();
        let forward = vec![ChangeRecord::splice(5, 0, "x")];
        let err = invert_forward_splices(&original, &forward)
            .expect_err("an out-of-range offset must error");
        assert!(matches!(err, WalrusError::SpliceOutOfBounds { .. }));
    }
}
