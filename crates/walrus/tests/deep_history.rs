//! Deep in-memory history reconstruct test for the walrus-fmt engine.
//!
//! Build a single-document `.wal` in memory with a FEW HUNDRED mixed-size
//! splices applied deterministically, then assert that reconstruction at EVERY
//! version index equals independently-computed expected bytes (FORMAT.md
//! section 5). A second pass round-trips through the compressed container
//! (`to_bytes` + `read`) and re-checks every index for byte-identity across the
//! container boundary.
//!
//! This exercises the live `Archive` engine. `Archive` uses the direction-aware
//! convention (FORMAT.md section 5): index 0 is the CURRENT version and the
//! file's splice `count` is the ORIGINAL. The history is replayed FORWARD as a
//! sequence of saves (`add` then one `update_with_splices` per record), so the
//! stored base is the original and each save moves the new current to index 0.
//!
//! The expected bytes are computed in forward order, where `expected[i]` is the
//! base with the first `i` records applied. The mapping onto the Archive index
//! space is therefore: Archive index `j` <-> `expected[VERSION_COUNT - j]`
//! (index 0 = final current = `expected[VERSION_COUNT]`, index `VERSION_COUNT` =
//! original = `expected[0]`). The assertion messages name the forward-order
//! `expected` subscript alongside `j` to make a mismatch easy to localise.

use walrus_fmt::{Archive, ChangeRecord, Direction, FileId, Manifest, Variant, Version};

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

/// Build the equivalent archive through the public mutate API: seed the
/// ORIGINAL as a forward file's first version, then replay each forward splice
/// as a save. A forward save APPENDS the net splice and moves the new current to
/// index 0, so after all saves the file holds `VERSION_COUNT` splices with the
/// final current at index 0 and the original at index `VERSION_COUNT`.
fn build_archive(base: &[u8], log: &[ChangeRecord]) -> Archive {
    let mut archive = Archive::new(Manifest::new(Variant::Wal));
    archive
        .add("document", base, None, Some(Direction::Forward))
        .expect("seed the original as the file's first version");
    for record in log {
        archive
            .update_with_splices("document", std::slice::from_ref(record))
            .expect("apply one forward splice as a save");
    }
    archive
}

#[test]
fn reconstruct_at_every_index_of_a_deep_history_matches_expected_bytes() {
    let (base, log, expected) = build_history();
    let archive = build_archive(&base, &log);

    let count = archive
        .file("document")
        .expect("the tracked file exists")
        .versions
        .count;
    assert_eq!(
        count, VERSION_COUNT,
        "every forward save appends a net splice, so count must equal the version count"
    );

    // Archive index j corresponds to expected[VERSION_COUNT - j]: index 0 is the
    // final current and index VERSION_COUNT is the original.
    let id = FileId::from("f0");
    for j in 0..=VERSION_COUNT {
        let want = &expected[VERSION_COUNT - j];
        let got = archive
            .reconstruct(&id, Version::Index(j))
            .expect("reconstruct must succeed for an in-range index");
        assert_eq!(
            &got,
            want,
            "archive index {j} (expected[{}]) reconstructs to the wrong bytes",
            VERSION_COUNT - j
        );
    }
}

#[test]
fn deep_history_round_trips_byte_identical_through_the_container_at_every_index() {
    let (base, log, expected) = build_history();
    let original = build_archive(&base, &log);

    let bytes = original.to_bytes().expect("serialise to container bytes");
    let reopened = Archive::read(&bytes).expect("read the container back");

    let count = reopened
        .file("document")
        .expect("the tracked file survives the round-trip")
        .versions
        .count;
    assert_eq!(
        count, VERSION_COUNT,
        "round-trip must preserve the version count"
    );

    // Same mapping as above, now asserted on the reopened archive.
    let id = FileId::from("f0");
    for j in 0..=VERSION_COUNT {
        let want = &expected[VERSION_COUNT - j];
        let got = reopened
            .reconstruct(&id, Version::Index(j))
            .expect("reconstruct after round-trip must succeed");
        assert_eq!(
            &got,
            want,
            "archive index {j} (expected[{}]) differs after a write/read round-trip",
            VERSION_COUNT - j
        );
    }
}
