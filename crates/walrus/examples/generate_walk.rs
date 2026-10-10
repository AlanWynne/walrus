//! Generate `examples/walrus.wal`: the mascot "walking walrus" demo.
//!
//! Run from the workspace root with:
//!
//! ```sh
//! cargo run -p walrus-fmt --example generate_walk
//! ```
//!
//! The base (version 0) is the ASCII walrus at the far left (zero indent). Each
//! forward change record shifts him one column to the right by widening the
//! leading indentation of every art line. Playing the file forward in `walker`
//! walks the walrus across the screen; backward returns him; loop makes him
//! pace. Frame `N` places the walrus at column `N`, so a reader can assert the
//! leading-space count at each version index -- the file doubles as a
//! deterministic playback test.
//!
//! Each record is a single splice that replaces the whole frame with the next
//! one. That keeps every step self-inverting and every version byte-exact, so
//! forward-then-backward playback returns identical frames.

use std::path::PathBuf;

use walrus_fmt::{ChangeRecord, WalrusFile};

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

/// How many columns to walk across (number of forward records). Frame 0 is the
/// base at column 0; the head is the walrus at column `STEPS`.
const STEPS: usize = 24;

fn main() -> anyhow::Result<()> {
    // Version 0: the walrus at column 0.
    let base = frame(0);

    // Each later frame is the walrus one column further right. A record deletes
    // the whole current frame and inserts the next; `at` is 0 and `delete_len`
    // is the current frame length.
    let mut log: Vec<ChangeRecord> = Vec::new();
    let mut current = base.clone();
    for col in 1..=STEPS {
        let next = frame(col);
        let insert = std::str::from_utf8(&next).expect("frame is valid UTF-8");
        let mut record = ChangeRecord::splice(0, current.len(), insert);
        record.note = Some(format!("walrus at column {col}"));
        log.push(record);
        current = next;
    }

    let mut doc = WalrusFile::new_wal(base, log)?;
    let m = doc.manifest_mut();
    if let Some(file) = m.files.first_mut() {
        // Per-file document metadata now lives on the tracked file (the old
        // top-level Manifest::document was removed by the multi-file reshape).
        file.name = "walrus.txt".to_string();
        file.encoding = "utf-8".to_string();
        file.mime = "text/plain".to_string();
    }
    m.created = Some("2026-10-08T20:30:00Z".to_string());
    m.generator = Some(format!(
        "walrus-fmt {} example/generate_walk",
        env!("CARGO_PKG_VERSION")
    ));

    let out = output_path();
    doc.write(&out)?;

    // Self-verify the round-trip the way a reader would, and confirm the walrus
    // moved exactly `STEPS` columns between frame 0 and the head. Measure the
    // SHIFT (head indent minus base indent) rather than an absolute column, so
    // the art's own built-in leading spaces do not skew the check.
    let reopened = WalrusFile::open(&out)?;
    let base_frame = reopened.reconstruct(walrus_fmt::Version::Index(0))?;
    let head_frame = reopened.reconstruct(walrus_fmt::Version::Head)?;
    let shift = leading_spaces(&head_frame) - leading_spaces(&base_frame);
    println!("wrote {} ({} versions)", out.display(), reopened.len());
    println!("  verify: walrus shifted {shift} columns base->head (expected {STEPS})");
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
