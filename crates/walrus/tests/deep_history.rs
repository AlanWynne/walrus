//! Deep in-memory history reconstruct test for the walrus-fmt engine.
//!
//! Build a single-document `.wal` in memory with a FEW HUNDRED mixed-size
//! splices applied deterministically, then assert that reconstruction at EVERY
//! version index equals independently-computed expected bytes (FORMAT.md
//! section 5). A second pass round-trips through the compressed container
//! (`to_bytes` + `read`) and re-checks every index for byte-identity across the
//! container boundary.
//!
//! The single-document `WalrusFile::reconstruct(Version::Index(i))` keeps the
//! OLD index convention: index 0 is the stored base (version 0) and index
//! `len()` is the fully-replayed far end. The expected bytes are computed in
//! that same forward order, so index `i` corresponds to `expected[i]`.

use walrus_fmt::{ChangeRecord, Version, WalrusFile};

/// Number of change records (versions beyond the base) to build.
const VERSION_COUNT: usize = 300;

/// Apply one splice to `current` in place: at `at`, delete `delete_len` bytes
/// and insert `insert`. The caller guarantees the splice is in-bounds.
fn apply_splice(current: &mut Vec<u8>, at: usize, delete_len: usize, insert: &[u8]) {
    let tail = current.split_off(at + delete_len);
    current.truncate(at);
    current.extend_from_slice(insert);
    current.extend_from_slice(&tail);
}

/// Build the base, the forward log, and the independently-computed expected
/// bytes for every version index `0..=VERSION_COUNT`.
///
/// `expected[0]` is the base; `expected[i]` is the base with the first `i`
/// records applied. Every record is constructed to be in-bounds for the version
/// it applies to, mixing single-character edits, tiny single-line edits, and
/// periodic multi-kilobyte whole-content rewrites, deterministically from the
/// version number.
fn build_history() -> (Vec<u8>, Vec<ChangeRecord>, Vec<Vec<u8>>) {
    let base: Vec<u8> = b"one\nseed line\n".to_vec();
    let mut current = base.clone();
    let mut log: Vec<ChangeRecord> = Vec::with_capacity(VERSION_COUNT);
    let mut expected: Vec<Vec<u8>> = Vec::with_capacity(VERSION_COUNT + 1);
    expected.push(current.clone());

    for version in 1..=VERSION_COUNT {
        let phase = version % 7;
        let (at, delete_len, insert): (usize, usize, String) = if phase == 0 {
            // Large whole-content rewrite: replace EVERYTHING with a multi-KB
            // block keyed on the version number (a big splice).
            let block = format!("block-{version:04}-").repeat(400); // ~5 KB
            let whole = format!("BIG\n{block}\nend-{version:04}\n");
            (0, current.len(), whole)
        } else if phase == 3 {
            // Tiny single-line edit: append a short line at the end (small
            // splice, zero deletion).
            (current.len(), 0, format!("line edit {version}\n"))
        } else {
            // Single-character / short marker edit: insert a couple of bytes at
            // a deterministic in-bounds offset (one small splice).
            let at = version % (current.len() + 1);
            (at, 0, format!("v{}", version % 10))
        };

        let insert_bytes = insert.clone().into_bytes();
        log.push(ChangeRecord::splice(at, delete_len, insert));
        apply_splice(&mut current, at, delete_len, &insert_bytes);
        expected.push(current.clone());
    }

    (base, log, expected)
}

#[test]
fn reconstruct_at_every_index_of_a_deep_history_matches_expected_bytes() {
    let (base, log, expected) = build_history();
    let file = WalrusFile::new_wal(base, log).expect("new_wal builds a valid manifest");

    assert_eq!(
        file.len(),
        VERSION_COUNT,
        "log length must equal the version count"
    );
    // expected has one entry per index in 0..=len, so enumerate covers them all.
    for (index, want) in expected.iter().enumerate() {
        let got = file
            .reconstruct(Version::Index(index))
            .expect("reconstruct must succeed for an in-range index");
        assert_eq!(&got, want, "version {index} reconstructs to the wrong bytes");
    }
}

#[test]
fn deep_history_round_trips_byte_identical_through_the_container_at_every_index() {
    let (base, log, expected) = build_history();
    let original = WalrusFile::new_wal(base, log).expect("new_wal builds a valid manifest");

    let bytes = original.to_bytes().expect("serialise to container bytes");
    let reopened = WalrusFile::read(&bytes).expect("read the container back");

    assert_eq!(
        reopened.len(),
        VERSION_COUNT,
        "round-trip must preserve the version count"
    );
    // expected has one entry per index in 0..=len, so enumerate covers them all.
    for (index, want) in expected.iter().enumerate() {
        let got = reopened
            .reconstruct(Version::Index(index))
            .expect("reconstruct after round-trip must succeed");
        assert_eq!(
            &got, want,
            "version {index} differs after a write/read round-trip"
        );
    }
}
