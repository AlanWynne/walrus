//! Headless egui_kittest tests for the FR-K9 per-file history view.
//!
//! These drive the REAL `history_view::history_panel` against an in-memory
//! `HistorySource` (no `walrus-fmt` archive), mirroring `navigator.rs`: each
//! test runs a FIXED, bounded number of frames and asserts on the accessibility
//! tree and on the returned export intent -- never on pixels or frame counts.
//!
//! The panel is coded to the format-agnostic seam, so the tests need only
//! `InMemorySource`. The live `rfd` save is MANUAL and not exercised here; the
//! export-intent STRING the button yields is pure and asserted directly.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walker::history_view::{self, history_report_markdown};
use walker::source::testsupport::InMemorySource;
use walker::source::HistorySource;

/// The captured export intent is threaded as the harness state so a click can
/// be read back after a frame, mirroring how `navigator.rs` captures actions.
type ExportSink = Option<String>;

/// AC1: the panel lists one row per synthesized change record for the selected
/// file, each row carrying its index.
#[test]
fn history_panel_lists_an_entry_row_for_each_change_record() {
    let source = InMemorySource::sample();
    let readme = source.file_named("README.md"); // two versions -> one entry.
    let entries = source.history(&readme);
    assert_eq!(entries.len(), 1, "README.md synthesizes exactly one entry");

    let mut harness = Harness::new_ui_state(
        move |ui, _sink: &mut ExportSink| {
            history_view::history_panel(ui, &source, &readme);
        },
        ExportSink::None,
    );
    harness.run();

    // The one entry's row label carries its index (`#0`).
    assert!(
        harness.query_by_label_contains("#0").is_some(),
        "a row is present for the entry at index 0"
    );
}

/// AC1: each row shows the splice summary in the `@<at> -<del> +<ins>` form.
#[test]
fn history_panel_shows_the_splice_summary_in_each_row() {
    let source = InMemorySource::sample();
    let readme = source.file_named("README.md");
    let entries = source.history(&readme);
    let splice = &entries[0].splice;
    let fragment = format!(
        "@{} -{} +{}",
        splice.at, splice.deleted_len, splice.inserted_len
    );

    let mut harness = Harness::new_ui_state(
        move |ui, _sink: &mut ExportSink| {
            history_view::history_panel(ui, &source, &readme);
        },
        ExportSink::None,
    );
    harness.run();

    assert!(
        harness.query_by_label_contains(&fragment).is_some(),
        "the row shows the splice summary fragment {fragment:?}"
    );
}

/// AC2: clicking "Export report" yields the deterministic Markdown report for
/// the selected file (matching the pure formatter for the same entries).
#[test]
fn history_panel_export_button_yields_deterministic_markdown() {
    let source = InMemorySource::sample();
    let readme = source.file_named("README.md");
    let expected = history_report_markdown("README.md", &source.history(&readme));

    let mut harness = Harness::new_ui_state(
        move |ui, sink: &mut ExportSink| {
            let action = history_view::history_panel(ui, &source, &readme);
            if let Some(md) = action.export_markdown {
                *sink = Some(md);
            }
        },
        ExportSink::None,
    );
    harness.run();

    // Click the export button and run one frame to process the click.
    harness.get_by_label("Export report").click();
    harness.run();

    assert_eq!(
        harness.state().as_deref(),
        Some(expected.as_str()),
        "the export intent equals the pure formatter output"
    );
}
