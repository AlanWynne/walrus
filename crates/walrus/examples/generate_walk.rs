//! Generate `examples/walrus.wal`: the mascot "walking walrus" demo.
//!
//! Run from the workspace root with:
//!
//! ```sh
//! cargo run -p walrus-fmt --example generate_walk
//! ```
//!
//! Built on the multi-file [`Archive`] mutate API. The archive tracks a single
//! file, `walrus.txt`, authored as a FORWARD `.wal`: its base is the ORIGINAL
//! (the walrus at column 0) and each forward save shifts him one column to the
//! right by widening the leading indentation of every art line. Playing the
//! file forward in `walker` walks the walrus across the screen; backward
//! returns him; loop makes him pace.
//!
//! Index model (FORMAT.md sections 3/5): INDEX 0 IS THE CURRENT version, higher
//! indices are OLDER, and `count` (== STEPS) is the ORIGINAL. The art's own
//! leftmost frame -- "frame 0", column 0 -- is therefore the ORIGINAL at
//! `Version::Original` (index STEPS), while the rightmost "frame STEPS" is the
//! CURRENT at `Version::Current` (index 0). (This flips the OLD single-file
//! shim's "base = index 0 = leftmost" wording: the base is still the leftmost
//! frame, but the base is now the ORIGINAL at the FAR index, not index 0.)
//!
//! Each save is a single whole-frame splice that replaces the entire current
//! frame with the next one (via `update_with_splices`, which stays in the
//! default feature set and needs no diff backend). That keeps every step
//! self-inverting and every version byte-exact, so forward-then-backward
//! playback returns identical frames -- the file doubles as a deterministic
//! playback test (see `tests/walrus_demo.rs`).

use std::path::PathBuf;

use walrus_fmt::{Archive, ChangeRecord, Direction, Manifest, Variant, Version};

/// The walrus art lines at zero indentation (the leftmost frame). Each frame
/// prepends `col` spaces to every line to place him at that column.
const WALRUS: &[&str] = &[
    "    _____",
    " .-'     '-.",
    "/   o   o   \\",
    "|    .---.    |      (((",
    "|   ( ~~~ )   |     ~~~~~",
    " \\   '---'   /    the walrus",
    "  '-._____.-'",
    "    || ||",
    "    || ||     <- tusks",
];

/// How many columns to walk across (number of forward saves). The ORIGINAL
/// (far index == STEPS) is the walrus at column 0; the CURRENT (index 0) is the
/// walrus at column `STEPS`. Mirrored by `STEPS` in `tests/walrus_demo.rs`.
const STEPS: usize = 24;

fn main() -> anyhow::Result<()> {
    // Seed an empty archive carrying the deterministic archive-level metadata
    // (fixed timestamp, no system clock enters the written bytes), then add the
    // ORIGINAL frame and walk forward one save per column.
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.created = Some("2026-10-08T20:30:00Z".to_string());
    manifest.generator = Some(format!(
        "walrus-fmt {} example/generate_walk",
        env!("CARGO_PKG_VERSION")
    ));
    let mut archive = Archive::new(manifest);

    // The ORIGINAL (base/far index) is the walrus at column 0. `add` sets the
    // per-file name/encoding/mime (walrus.txt, utf-8, text/plain) and marks the
    // file FORWARD so its base stays fixed as saves append.
    let mut current = frame(0);
    archive.add("walrus.txt", &current, None, Some(Direction::Forward))?;

    // Each later save replaces the whole current frame with the walrus one
    // column further right -- a single splice at offset 0 deleting the old frame
    // and inserting the next. One save == count advances by one.
    for col in 1..=STEPS {
        let next = frame(col);
        let insert = std::str::from_utf8(&next).expect("frame is valid UTF-8");
        let splice = ChangeRecord::splice(0, current.len(), insert);
        archive.update_with_splices("walrus.txt", &[splice])?;
        current = next;
    }

    let out = output_path();
    archive.write(&out)?;

    // Self-verify the round-trip through the NEW API: reopen, confirm the walrus
    // moved exactly `STEPS` columns between the ORIGINAL and the CURRENT. Measure
    // the SHIFT (current indent minus original indent) rather than an absolute
    // column, so the art's own built-in leading spaces do not skew the check.
    let reopened = Archive::open(&out)?;
    let tracked = reopened
        .file("walrus.txt")
        .expect("walrus.txt is tracked after write");
    let id = tracked.id.clone().into();
    let original = reopened.reconstruct(&id, Version::Original)?;
    let current = reopened.reconstruct(&id, Version::Current)?;
    let shift = leading_spaces(&current) - leading_spaces(&original);
    println!(
        "wrote {} ({} versions)",
        out.display(),
        tracked.versions.count
    );
    println!("  verify: walrus shifted {shift} columns original->current (expected {STEPS})");
    assert_eq!(
        shift, STEPS,
        "walrus did not shift the expected number of columns"
    );
    Ok(())
}

/// Count the leading spaces on the first line of a frame.
fn leading_spaces(frame: &[u8]) -> usize {
    frame.iter().take_while(|&&b| b == b' ').count()
}

/// Build the full walrus frame at the given column (that many leading spaces on
/// every art line). Lines are newline-terminated.
fn frame(col: usize) -> Vec<u8> {
    let indent = " ".repeat(col);
    let mut s = String::new();
    for line in WALRUS {
        s.push_str(&indent);
        s.push_str(line);
        s.push('\n');
    }
    s.into_bytes()
}

/// Resolve `<workspace>/examples/walrus.wal` from this crate's manifest dir.
fn output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join("walrus.wal")
}
