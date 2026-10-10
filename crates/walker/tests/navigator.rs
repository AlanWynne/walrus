//! Headless egui_kittest tests for the FR-K4 two-pane navigator.
//!
//! These drive the REAL navigator panes against a multi-file in-memory
//! `HistorySource` (no `walrus-fmt` archive), mirroring `harness.rs`: each test
//! runs a FIXED, bounded number of frames and asserts on the accessibility tree
//! (labels / enabled state) and on the `NavigatorState` model -- never on pixels
//! or frame counts.
//!
//! The navigator is coded to the format-agnostic seam, so the tests need only
//! `InMemorySource` and `NavigatorState`; no archive and no `rfd` dialog are
//! involved (the external pane's import flow is FR-K5 and stays MANUAL).

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walker::navigator::{self, NavigatorState};
use walker::source::testsupport::InMemorySource;
use walker::source::{HistorySource, VersionId};

/// AC1: the internal pane lists every tracked file with its index size and
/// generation cap (plus current size), driven headlessly from a multi-file
/// in-memory source.
#[test]
fn internal_pane_lists_every_tracked_file_with_index_size_and_generation_cap() {
    let source = InMemorySource::sample();
    let state = NavigatorState::default();

    let mut harness = Harness::new_ui_state(
        move |ui, state: &mut NavigatorState| {
            navigator::panels(ui, &source, state);
        },
        state,
    );
    harness.run();

    // Every sample file name appears as a row label.
    for name in [
        "README.md",
        "data.csv",
        "page.html",
        "data.json",
        "notes.txt",
    ] {
        assert!(
            harness.query_by_label_contains(name).is_some(),
            "the internal pane lists a row for {name}"
        );
    }

    // README.md carries two versions; its row shows index size 2, an
    // "unlimited" generation cap, and a byte count (current size). The full row
    // label asserts all three facets at once (and ends in "bytes").
    let readme_info = source_info("README.md");
    assert_eq!(readme_info.index_size, 2);
    assert!(
        harness
            .query_by_label_contains("README.md  --  2 versions, gen unlimited,")
            .is_some(),
        "README.md's row shows index size 2 and an unlimited generation cap"
    );
    assert!(
        harness
            .query_by_label_contains("gen unlimited, 25 bytes")
            .is_some(),
        "README.md's row shows its current size in bytes"
    );
}

/// The `FileInfo` for a named file in a fresh sample source (test helper so the
/// harness closure can own its own source).
fn source_info(name: &str) -> walker::source::FileInfo {
    let source = InMemorySource::sample();
    source.file_info(&source.file_named(name))
}

/// AC2: selecting a file binds the active ref and routes reconstruct to that
/// file's timeline (model-level binding), driven headlessly.
#[test]
fn selecting_a_file_binds_the_active_ref_and_routes_reconstruct() {
    let source = InMemorySource::sample();
    let csv = source.file_named("data.csv");
    let readme = source.file_named("README.md");
    let csv_bytes = source
        .reconstruct(&csv, &VersionId::Index(0))
        .expect("csv current bytes");

    let state = NavigatorState::default();
    let mut harness = Harness::new_ui_state(
        move |ui, state: &mut NavigatorState| {
            navigator::panels(ui, &source, state);
        },
        state,
    );
    harness.run();

    // Click data.csv's row and run one frame to process the click.
    harness.get_by_label_contains("data.csv").click();
    harness.run();
    assert_eq!(
        harness.state().active().map(|f| f.name().to_string()),
        Some("data.csv".to_string()),
        "clicking data.csv binds it as the active file"
    );

    // The active ref routes reconstruct to data.csv's canned current bytes.
    let active_source = InMemorySource::sample();
    let active = harness.state().active().expect("an active file").clone();
    assert_eq!(
        active_source
            .reconstruct(&active, &VersionId::Index(0))
            .expect("reconstruct via the active ref"),
        csv_bytes,
        "versions()/reconstruct route to the selected file"
    );

    // Selecting a different file changes the active ref (AC2).
    harness.get_by_label_contains("README.md").click();
    harness.run();
    assert_eq!(
        harness.state().active().map(|f| f.name().to_string()),
        Some(readme.name().to_string()),
        "selecting README.md changes the active ref"
    );
}

/// AC3: multi-select of two files enables the batch-action button; one selected
/// leaves it disabled. Driven headlessly on model state and the enabled widget.
#[test]
fn multi_select_of_two_files_enables_the_batch_action_button() {
    let source = InMemorySource::sample();

    // Start with a single selected file: the batch button is DISABLED.
    let mut state = NavigatorState::default();
    state.select(&source.file_named("README.md"));
    assert!(
        !state.batch_enabled(),
        "one selected file does not enable batch actions"
    );

    let mut harness = Harness::new_ui_state(
        move |ui, state: &mut NavigatorState| {
            navigator::panels(ui, &source, state);
        },
        state,
    );
    harness.run();
    assert!(
        harness.query_by_label("Batch actions (1)").is_some(),
        "the batch button reflects a single selection"
    );

    // Toggle a second file into the multi-select set via ctrl-click bookkeeping
    // on the model, then re-render.
    harness.state_mut().toggle(1); // data.csv
    harness.run();

    assert_eq!(harness.state().selected_count(), 2);
    assert!(
        harness.state().batch_enabled(),
        "two selected files enable batch actions"
    );
    assert!(
        harness.query_by_label("Batch actions (2)").is_some(),
        "the batch button label reflects two selected files"
    );
}
