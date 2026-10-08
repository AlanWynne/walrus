//! Generate `examples/walrus-story.wal`: a narrated title-card animation.
//!
//! Run from the workspace root with:
//!
//! ```sh
//! cargo run -p walrus-fmt --example generate_story
//! ```
//!
//! The file is a `.wal` whose base (version 0) is an opening title card and
//! whose forward log rewrites a caption line one frame at a time. Scrubbing or
//! auto-playing it in `walker` reads out a short story about what Walrus does,
//! like film credits. Playing it backward rewinds the story; looping paces it.
//!
//! How the animation is built: the "screen" is a fixed walrus header followed by
//! a caption region. Each frame REPLACES the entire caption region with the next
//! line of narration via a single splice (delete the old caption bytes, insert
//! the new ones). That keeps every step a clean, self-inverting splice and makes
//! each version byte-exact, so forward then backward playback returns identical
//! frames.

use std::path::PathBuf;

use walrus_fmt::{ChangeRecord, DocumentInfo, WalrusFile};

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

/// The caption lines, in playback order. Frame 0 is the base; each subsequent
/// line is reached by one forward change record.
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
    // Version 0 (the base, the original) is the header plus the first caption.
    let base = screen(CAPTION_FRAMES[0]);

    // Each later frame replaces the caption region. The caption begins at the
    // byte offset right after the header; everything from there to the end of
    // the current screen is the old caption to delete.
    let caption_at = HEADER.len();
    let mut log: Vec<ChangeRecord> = Vec::new();
    let mut current = base.clone();
    for next_caption in &CAPTION_FRAMES[1..] {
        let next = screen(next_caption);
        let delete_len = current.len() - caption_at;
        // The caption region is always valid UTF-8 (header is ASCII and ends on
        // a char boundary; the caption and trailing newline are UTF-8 text).
        let insert =
            std::str::from_utf8(&next[caption_at..]).expect("caption region is valid UTF-8");
        let mut record = ChangeRecord::splice(caption_at, delete_len, insert);
        record.note = Some(format!("caption: {next_caption}"));
        log.push(record);
        current = next;
    }

    let mut doc = WalrusFile::new_wal(base, log)?;
    let m = doc.manifest_mut();
    m.document = DocumentInfo {
        encoding: "utf-8".to_string(),
        mime: "text/plain".to_string(),
        name: "walrus-story.txt".to_string(),
    };
    m.created = Some("2026-10-08T20:00:00Z".to_string());
    m.generator = Some(format!(
        "walrus-fmt {} example/generate_story",
        env!("CARGO_PKG_VERSION")
    ));

    let out = output_path();
    doc.write(&out)?;

    // Report, and self-verify the round-trip the same way a reader would.
    let reopened = WalrusFile::open(&out)?;
    println!("wrote {} ({} versions)", out.display(), reopened.len());
    println!(
        "  verify: version 0 is {} bytes, head is {} bytes",
        reopened.reconstruct(walrus_fmt::Version::Index(0))?.len(),
        reopened.reconstruct(walrus_fmt::Version::Head)?.len(),
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
