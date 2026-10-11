//! Generate `examples/walrus-story.wal`: a narrated title-card animation.
//!
//! Run from the workspace root with:
//!
//! ```sh
//! cargo run -p walrus-fmt --example generate_story
//! ```
//!
//! Built on the multi-file [`Archive`] mutate API. The archive tracks a single
//! file, `walrus-story.txt`, authored as a FORWARD `.wal`: a fixed walrus header
//! above a caption region, and each forward save rewrites the caption line.
//! Scrubbing or auto-playing it in `walker` reads out a short story about what
//! Walrus does, like film credits. Playing it backward rewinds the story;
//! looping paces it.
//!
//! Index model (FORMAT.md sections 3/5): INDEX 0 IS THE CURRENT version, higher
//! indices are OLDER, and `count` is the ORIGINAL. So the opening caption
//! (`CAPTION_FRAMES[0]`) is the ORIGINAL at `Version::Original` (the far index)
//! and the closing bow (`CAPTION_FRAMES[last]`) is the CURRENT at
//! `Version::Current` (index 0). Playing the file FORWARD from original to
//! current reads the story in order.
//!
//! Each save REPLACES the entire caption region with the next line via a single
//! splice (delete the old caption bytes, insert the new ones), applied through
//! `update_with_splices` (default feature set, no diff backend). That keeps
//! every step a clean, self-inverting splice and makes each version byte-exact,
//! so forward then backward playback returns identical frames.

use std::path::PathBuf;

use walrus_fmt::{Archive, ChangeRecord, Direction, Manifest, Variant, Version};

/// The stable header shown above the caption on every frame.
const HEADER: &str = "\
        _____
     .-'     '-.
    /   o   o   \\
   |    .---.    |      (((
   |   ( ~~~ )   |     ~~~~~
    \\   '---'   /    the walrus
     '-._____.-'
       || ||
       || ||

   ----------------------------------------
   ";

/// The caption lines, in playback order. `CAPTION_FRAMES[0]` is the ORIGINAL
/// (far index); each subsequent line is reached by one forward save, and the
/// last line is the CURRENT (index 0).
const CAPTION_FRAMES: &[&str] = &[
    "a walrus clears his throat...",
    "\"Let me tell you what I carry.\"",
    "\"I am one file -- but I remember.\"",
    "\"Inside me: a base document,\"",
    "\"and an ordered log of every change.\"",
    "\"A .wal keeps the ORIGINAL as its base\"",
    "\"and plays its log FORWARD to now.\"",
    "\"A .wbl keeps the CURRENT as its base\"",
    "\"and plays its log BACKWARD to the start.\"",
    "\"Ask me for any version -- I rebuild it.\"",
    "\"Step forward. Step back. Watch it move.\"",
    "\"This very caption was an animation...\"",
    "\"...each line a splice in my change log.\"",
    "\"walrus-fmt reads me. walx inspects me.\"",
    "\"walker plays me, like these credits.\"",
    "\"One artefact. The whole history. Inside.\"",
    "the walrus takes a bow.   ~ fin ~",
];

fn main() -> anyhow::Result<()> {
    // Seed an empty archive carrying the deterministic archive-level metadata
    // (fixed timestamp, no system clock enters the written bytes).
    let mut manifest = Manifest::new(Variant::Wal);
    manifest.created = Some("2026-10-08T20:00:00Z".to_string());
    manifest.generator = Some(format!(
        "walrus-fmt {} example/generate_story",
        env!("CARGO_PKG_VERSION")
    ));
    let mut archive = Archive::new(manifest);

    // The ORIGINAL (base/far index) is the header plus the first caption.
    let mut current = screen(CAPTION_FRAMES[0]);
    archive.add("walrus-story.txt", &current, None, Some(Direction::Forward))?;

    // Each later save replaces the caption region. The caption begins at the
    // byte offset right after the header; everything from there to the end of
    // the current screen is the old caption to delete.
    let caption_at = HEADER.len();
    for next_caption in &CAPTION_FRAMES[1..] {
        let next = screen(next_caption);
        let delete_len = current.len() - caption_at;
        // The caption region is always valid UTF-8 (header is ASCII and ends on
        // a char boundary; the caption and trailing newline are UTF-8 text).
        let insert =
            std::str::from_utf8(&next[caption_at..]).expect("caption region is valid UTF-8");
        let splice = ChangeRecord::splice(caption_at, delete_len, insert);
        archive.update_with_splices("walrus-story.txt", &[splice])?;
        current = next;
    }

    let out = output_path();
    archive.write(&out)?;

    // Report, and self-verify the round-trip the same way a reader would, via
    // the NEW API endpoints (ORIGINAL = opening card, CURRENT = closing bow).
    let reopened = Archive::open(&out)?;
    let tracked = reopened
        .file("walrus-story.txt")
        .expect("walrus-story.txt is tracked after write");
    let id = tracked.id.clone().into();
    println!(
        "wrote {} ({} versions)",
        out.display(),
        tracked.versions.count
    );
    println!(
        "  verify: original is {} bytes, current is {} bytes",
        reopened.reconstruct(&id, Version::Original)?.len(),
        reopened.reconstruct(&id, Version::Current)?.len(),
    );
    Ok(())
}

/// Compose the full screen for a caption: the stable header then the caption.
fn screen(caption: &str) -> Vec<u8> {
    format!("{HEADER}{caption}\n").into_bytes()
}

/// Resolve `<workspace>/examples/walrus-story.wal` from this crate's manifest
/// dir (`crates/walrus`), so the output lands in the repo's top-level examples.
fn output_path() -> PathBuf {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .join("..")
        .join("..")
        .join("examples")
        .join("walrus-story.wal")
}
