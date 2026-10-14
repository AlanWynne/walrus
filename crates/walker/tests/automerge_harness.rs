//! Headless egui_kittest harness for the Automerge open path (FR-K11 FEAT-004).
//!
//! The whole file is `#![cfg(feature = "automerge")]`-gated: with the feature
//! off there is no `AutomergeSource` to drive and the crate must build without
//! the `automerge` dev-dependency path, so this test tree simply compiles out.
//!
//! It proves AC4 end to end: an Automerge document, opened THROUGH the shared
//! magic-byte sniff + `AutomergeSource::open` path, drives the EXACT Phase-1
//! `view::render` + `transport::controls` + `Player` seam a walrus file does.
//! Stepping to an older version changes the rendered bytes, and a structured
//! document's status note reaches the status bar -- all through the trait, with
//! no transport/player/viewer code made format-aware.
//!
//! The native `rfd` open/save dialog stays the ONLY MANUAL carve-out: it is an
//! OS-native dialog a headless accesskit harness cannot drive. Everything up to
//! the dialog -- the sniff, the open, reconstruct, render, transport -- is
//! driven here headlessly.
#![cfg(feature = "automerge")]

use std::io::Write;

use automerge::transaction::Transactable;
use automerge::{AutoCommit, ObjType, ROOT};

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walker::player::Player;
use walker::source::{sniff_backend, AutomergeSource, Backend, HistorySource};
use walker::transport;
use walker::view;

// === fixtures ===========================================================

/// Build a single-text-root Automerge document whose text is grown by one
/// edit per change (so there are `edits.len()` text changes plus the object
/// create), and save it to bytes.
fn text_doc_bytes(key: &str, edits: &[&str]) -> Vec<u8> {
    let mut doc = AutoCommit::new();
    let text = doc
        .put_object(ROOT, key, ObjType::Text)
        .expect("put text object");
    doc.commit();
    let mut at = 0usize;
    for edit in edits {
        doc.splice_text(&text, at, 0, edit).expect("splice text");
        at += edit.chars().count();
        doc.commit();
    }
    doc.save()
}

/// Build a structured (multi-key) Automerge document and save it to bytes.
fn structured_doc_bytes() -> Vec<u8> {
    let mut doc = AutoCommit::new();
    doc.put(ROOT, "title", "walrus").expect("put title");
    doc.put(ROOT, "count", 2_i64).expect("put count");
    doc.commit();
    doc.save()
}

/// Write `bytes` to a `name` file in a fresh temp dir and return both so the
/// dir outlives the open. Uses `tempfile::TempDir` per the testing steering (no
/// files outside tests/fixtures/).
fn write_temp(bytes: &[u8], name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("make temp dir");
    let path = dir.path().join(name);
    let mut f = std::fs::File::create(&path).expect("create fixture");
    f.write_all(bytes).expect("write fixture");
    (dir, path)
}

/// Open `path` THROUGH the shared sniff: read the leading bytes, assert they
/// classify as Automerge, then build the source. This mirrors the app-shell
/// open path (`app.rs::open_file`) exactly: the sniff decides, not the
/// extension.
fn open_through_sniff(path: &std::path::Path) -> AutomergeSource {
    let head = std::fs::read(path).expect("read fixture head");
    assert_eq!(
        sniff_backend(&head),
        Backend::Automerge,
        "the fixture's leading bytes must classify as Automerge"
    );
    AutomergeSource::open(path).expect("open Automerge document through the sniff path")
}

// === AC4: same transport + viewer, past version changes rendered bytes ==

/// FR-K11 AC4: an Automerge document opened through the sniff path drives the
/// SAME `view::render` + `transport::controls` + `Player` seam as a walrus
/// file. Stepping to an older version changes the rendered text, proving no
/// transport/player/viewer code is format-aware.
#[test]
fn stepping_older_changes_the_rendered_automerge_bytes_headless() {
    // "hello " then "world": current text is "hello world"; one step older is
    // "hello " (before the final splice).
    let (_dir, path) = write_temp(
        &text_doc_bytes("doc.txt", &["hello ", "world"]),
        "note.amrg",
    );
    let source = open_through_sniff(&path);
    let file = source.files()[0].clone();

    // The player is sized from the trait exactly as app.rs::open_file does:
    // len = versions(active).len() - 1 (the inclusive far index).
    let len = source.versions(&file).len().saturating_sub(1);
    let player = Player::new(len);

    // Drive the EXACT Phase-1 central-panel seam: a real Player + the real
    // transport controls, with the viewer reconstructing the active file at
    // `source.version_at(file, player.index())` via `view::render`. The harness
    // only ever names the trait; nothing here is automerge-aware.
    let mut harness = Harness::new_ui_state(
        move |ui, player: &mut Player| {
            let mut speed = 1.0_f64;
            transport::controls(ui, player, &mut speed);
            ui.separator();
            let version = source.version_at(&file, player.index());
            view::render(ui, &source, &file, &version);
        },
        player,
    );
    harness.run();

    // Current version (index 0) renders "hello world". The text/plain renderer
    // paints the reconstructed bytes into a read-only `TextEdit` (view::text),
    // so the content is the node's accessibility VALUE, not its label. egui's
    // multiline edit exposes the text as BOTH a `MultilineTextInput` and a
    // child `TextRun` carrying the same value, so query by value and assert at
    // least one match (`query_all_by_value`) rather than the exactly-one
    // `query_by_value`.
    assert!(
        harness.query_all_by_value("hello world").next().is_some(),
        "the current Automerge version renders its current text"
    );

    // Step to an older version through the real transport "< older" control.
    harness.get_by_label("< older").click();
    harness.run();

    assert_eq!(
        harness.state().index(),
        1,
        "clicking 'older' advanced the shared player to the older index"
    );
    // The viewer re-rendered the OLDER version's bytes through the same seam.
    assert!(
        harness.query_all_by_value("hello ").next().is_some(),
        "the viewer re-rendered the older Automerge version's text"
    );
    assert!(
        harness.query_all_by_value("hello world").next().is_none(),
        "the current text is no longer shown after stepping older"
    );
}

// === AC7 wiring: structured status note reaches the status bar ==========

/// FR-K11 AC7 (wiring): a structured Automerge document's `status_note` reaches
/// the status bar through the format-agnostic seam. The harness renders the
/// real `chrome::status_bar` with the note sourced from `status_note(active)`,
/// exactly as `app.rs` wires it, and asserts the structured label appears.
#[test]
fn a_structured_document_status_note_reaches_the_status_bar_headless() {
    let (_dir, path) = write_temp(&structured_doc_bytes(), "data.amrg");
    let source = open_through_sniff(&path);
    let file = source.files()[0].clone();

    // The note comes from the trait, not from a concrete downcast.
    let note = source.status_note(&file);
    assert_eq!(
        note.as_deref(),
        Some("Automerge (structured JSON view)"),
        "a structured document reports the JSON-view note through the trait"
    );

    let (mime, name) = source.content_hint(&file);
    let mut harness = Harness::new_ui(move |ui| {
        let info = walker::chrome::StatusInfo {
            file_name: &name,
            variant: "amrg",
            index: 0,
            total: source.versions(&file).len(),
            renderer: "Code",
            mime: &mime,
            note: note.as_deref(),
        };
        walker::chrome::status_bar(ui, Some(&info), "ready");
    });
    harness.run();

    assert!(
        harness
            .query_by_label("Automerge (structured JSON view)")
            .is_some(),
        "the structured-view note reaches the status bar through StatusInfo.note"
    );
}
