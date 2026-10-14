//! Headless egui_kittest tests for the trait-driven presentation path (FR-K8).
//!
//! These prove that a renderer is chosen from `HistorySource::content_hint` and
//! that the bytes painted come from `HistorySource::reconstruct`, driven end to
//! end against the in-memory fixture source with NO `walrus-fmt` archive. The
//! harness runs a FIXED, bounded number of frames and asserts on the
//! accessibility tree / model state -- never on pixels or frame counts.
//!
//! The native `rfd` save dialog (export) stays MANUAL: it is a real OS dialog a
//! headless accesskit harness cannot drive. Everything up to it -- format
//! offering and byte production -- is asserted in `export_ui`'s unit tests.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walker::source::testsupport::InMemorySource;
use walker::source::{HistorySource, VersionId};
use walker::view::{self, Renderer};

/// Rendering a text/csv file's reconstructed CURRENT bytes through
/// `view::render` paints the CSV grid, and the header cell text from the
/// reconstructed bytes appears in the accessibility tree.
#[test]
fn render_paints_reconstructed_csv_bytes_through_the_source() {
    let source = InMemorySource::single("data.csv", "text/csv", b"name,age\nalice,30");

    let mut harness = Harness::new_ui(move |ui| {
        let file = source.files()[0].clone();
        view::render(ui, &source, &file, &VersionId::Index(0));
    });
    harness.run();

    // The CSV grid renders the first row as a header; its cells come straight
    // from the reconstructed bytes.
    assert!(
        harness.query_by_label("name").is_some(),
        "the csv header cell 'name' from the reconstructed bytes is rendered"
    );
    assert!(
        harness.query_by_label("age").is_some(),
        "the csv header cell 'age' from the reconstructed bytes is rendered"
    );
}

/// Rendering an OLDER version (index 1) through the source paints that older
/// version's bytes, proving the byte path is version-addressed via the source.
#[test]
fn render_paints_the_requested_older_version_bytes() {
    let source = InMemorySource::sample();

    let mut harness = Harness::new_ui(move |ui| {
        let file = source.file_named("README.md");
        // Index 1 is the OLDER readme version in the sample source.
        view::render(ui, &source, &file, &VersionId::Index(1));
    });
    harness.run();

    assert!(
        harness.query_by_label("Older").is_some(),
        "the older version heading text is rendered from the reconstructed bytes"
    );
}

/// An out-of-range version does not panic the viewer: `view::render` shows an
/// inline reconstruction error instead.
#[test]
fn render_shows_an_inline_error_for_an_out_of_range_version() {
    let source = InMemorySource::single("notes.txt", "text/plain", b"hello");

    let mut harness = Harness::new_ui(move |ui| {
        let file = source.files()[0].clone();
        view::render(ui, &source, &file, &VersionId::Index(99));
    });
    harness.run();

    // The exact inline label is the formatted reconstruction error for the
    // out-of-range index produced by the in-memory source.
    assert!(
        harness
            .query_by_label("Reconstruction error: version index 99 out of range")
            .is_some(),
        "an out-of-range version renders an inline error, not a panic"
    );
}

/// The renderer chosen for a file is driven by the source's content hint:
/// a text/markdown hint selects the markdown renderer.
#[test]
fn select_renderer_for_markdown_file_is_markdown() {
    let source = InMemorySource::sample();
    let file = source.file_named("README.md");
    assert_eq!(
        view::select_renderer_for(&source, &file),
        Renderer::Markdown
    );
}
