//! The walrus demo as a deterministic playback fixture (FR-W12).
//!
//! This integration test treats the COMMITTED `examples/walrus.wal` as a real
//! multi-file [`Archive`]: it opens the committed binary (never regenerating at
//! test time, never using a tempfile), reconstructs every version index, and
//! asserts the forward/backward walk is byte-identical at every index (INV-3).
//!
//! Index model (FORMAT.md sections 3/5): index 0 is the CURRENT version, higher
//! indices are OLDER, and `count` is the ORIGINAL. The generator authors the
//! walk as a FORWARD `.wal` whose ORIGINAL (`Version::Original`, index `count`)
//! is the walrus at column 0 and whose CURRENT (`Version::Current`, index 0) is
//! the walrus at column `STEPS`. So the column shift CURRENT - ORIGINAL == STEPS.

use std::path::PathBuf;

use walrus_fmt::{Archive, Version};

/// Column count the generator walks across. Mirrors `STEPS` in
/// `examples/generate_walk.rs`; the two MUST stay in step (the generator applies
/// one whole-frame save per column, so the file has exactly `STEPS` records).
const STEPS: usize = 24;

/// Resolve the committed fixture relative to this crate's manifest dir.
fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("walrus.wal")
}

/// Count the leading spaces on the first line of a frame.
fn leading_spaces(frame: &[u8]) -> usize {
    frame.iter().take_while(|&&b| b == b' ').count()
}

#[test]
fn walrus_demo_opens_as_a_multi_file_archive_tracking_walrus_txt() {
    let archive = Archive::open(fixture_path()).expect("open committed walrus.wal");
    let tracked = archive
        .file("walrus.txt")
        .expect("archive tracks a file named walrus.txt");
    assert_eq!(
        tracked.versions.count, STEPS,
        "the walk has exactly STEPS records (one whole-frame save per column)"
    );
}

#[test]
fn walrus_demo_reconstructs_every_version_index_validly() {
    let archive = Archive::open(fixture_path()).expect("open committed walrus.wal");
    let tracked = archive.file("walrus.txt").expect("walrus.txt tracked");
    let id = tracked.id.clone().into();
    let count = tracked.versions.count;
    for k in 0..=count {
        archive
            .reconstruct(&id, Version::Index(k))
            .unwrap_or_else(|e| panic!("index {k} must reconstruct validly: {e}"));
    }
}

#[test]
fn walrus_demo_forward_then_backward_walk_is_byte_identical() {
    let archive = Archive::open(fixture_path()).expect("open committed walrus.wal");
    let tracked = archive.file("walrus.txt").expect("walrus.txt tracked");
    let id = tracked.id.clone().into();
    let count = tracked.versions.count;

    // Walk CURRENT -> ORIGINAL (index 0 up to count), then back. Each reversed
    // frame must equal its forward counterpart byte-for-byte (INV-3).
    let forward: Vec<Vec<u8>> = (0..=count)
        .map(|k| {
            archive
                .reconstruct(&id, Version::Index(k))
                .expect("forward reconstruct")
        })
        .collect();
    let backward: Vec<Vec<u8>> = (0..=count)
        .rev()
        .map(|k| {
            archive
                .reconstruct(&id, Version::Index(k))
                .expect("backward reconstruct")
        })
        .collect();
    for k in 0..=count {
        assert_eq!(
            forward[k],
            backward[count - k],
            "forward/backward frames differ at index {k}"
        );
    }
}

#[test]
fn walrus_demo_column_shift_between_original_and_current_equals_steps() {
    let archive = Archive::open(fixture_path()).expect("open committed walrus.wal");
    let tracked = archive.file("walrus.txt").expect("walrus.txt tracked");
    let id = tracked.id.clone().into();

    let current = archive
        .reconstruct(&id, Version::Current)
        .expect("reconstruct current");
    let original = archive
        .reconstruct(&id, Version::Original)
        .expect("reconstruct original");
    let shift = leading_spaces(&current) - leading_spaces(&original);
    assert_eq!(
        shift, STEPS,
        "walrus shifted {shift} columns ORIGINAL->CURRENT (expected {STEPS})"
    );
}

#[test]
fn walrus_demo_index_endpoints_match_current_and_original() {
    let archive = Archive::open(fixture_path()).expect("open committed walrus.wal");
    let tracked = archive.file("walrus.txt").expect("walrus.txt tracked");
    let id = tracked.id.clone().into();
    let count = tracked.versions.count;

    assert_eq!(
        archive
            .reconstruct(&id, Version::Index(0))
            .expect("index 0"),
        archive.reconstruct(&id, Version::Current).expect("current"),
        "index 0 is the current version"
    );
    assert_eq!(
        archive
            .reconstruct(&id, Version::Index(count))
            .expect("index count"),
        archive
            .reconstruct(&id, Version::Original)
            .expect("original"),
        "index count is the original version"
    );
}
