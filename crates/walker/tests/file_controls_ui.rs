//! Headless `egui_kittest` tests for the FR-K6 per-file + batch controls
//! (AC1 per-file controls, AC2 batch-remove confirmation).
//!
//! These drive the REAL navigator panes over a multi-file `InMemorySource`
//! through `navigator::panels`, capturing the frame's
//! [`NavigatorAction::file_action`] via a shared `Rc<RefCell<..>>` sink (the
//! pattern `tests/navigator.rs` uses for `history_for`). Each test runs a
//! FIXED, bounded number of frames and asserts on labels / model state, never
//! on pixels or frame counts. The `rfd` save dialog an export intent later
//! drives is the one MANUAL carve-out and is not exercised here.

use std::cell::RefCell;
use std::rc::Rc;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walker::import::{FilePicker, ImportState};
use walker::navigator::controls::FileAction;
use walker::navigator::{self, NavigatorState};
use walker::source::testsupport::InMemorySource;

/// A no-op [`FilePicker`]: these tests never click Import.
struct NoPicker;

impl FilePicker for NoPicker {
    fn pick(&self) -> Option<std::path::PathBuf> {
        None
    }
}

type Sink = Rc<RefCell<Option<FileAction>>>;

/// Build a harness over the sample source with `state`, routing every frame's
/// `file_action` into `sink`.
fn harness_with(state: NavigatorState, sink: Sink) -> Harness<'static, NavigatorState> {
    let mut import = ImportState::default();
    Harness::new_ui_state(
        move |ui, state: &mut NavigatorState| {
            let source = InMemorySource::sample();
            let action = navigator::panels(ui, &source, &NoPicker, &mut import, state);
            if let Some(fa) = action.file_action {
                *sink.borrow_mut() = Some(fa);
            }
        },
        state,
    )
}

/// AC1: entering a cap in the per-file gen-cap field and clicking Apply emits
/// `SetGenerations` with the parsed cap for the active file.
#[test]
fn applying_a_generation_cap_emits_set_generations_for_the_active_file() {
    let source = InMemorySource::sample();
    let mut state = NavigatorState::default();
    state.select(&source.file_named("README.md"));

    let captured: Sink = Rc::new(RefCell::new(None));
    let mut harness = harness_with(state, Rc::clone(&captured));
    harness.run();

    // Focus the gen-cap field (found by its labelled_by caption "Generation
    // cap:", unique across the pane), then type "3". egui needs the field
    // focused before it accepts keystrokes.
    harness.get_by_label_contains("Generation cap:").click();
    harness.run();
    harness
        .get_by_label_contains("Generation cap:")
        .type_text("3");
    harness.run();
    harness.get_by_label("Apply cap").click();
    harness.run();

    let id = source.file_named("README.md").id();
    assert_eq!(
        *captured.borrow(),
        Some(FileAction::SetGenerations { id, n: Some(3) }),
        "Apply with '3' emits SetGenerations n=Some(3)"
    );
}

/// AC1: a blank gen-cap field applies as `None` (unlimited).
#[test]
fn applying_a_blank_generation_cap_emits_unlimited() {
    let source = InMemorySource::sample();
    let mut state = NavigatorState::default();
    state.select(&source.file_named("README.md"));

    let captured: Sink = Rc::new(RefCell::new(None));
    let mut harness = harness_with(state, Rc::clone(&captured));
    harness.run();

    // Do not type anything: the field is blank -> unlimited.
    harness.get_by_label("Apply cap").click();
    harness.run();

    let id = source.file_named("README.md").id();
    assert_eq!(
        *captured.borrow(),
        Some(FileAction::SetGenerations { id, n: None }),
        "a blank cap field applies as unlimited (None)"
    );
}

/// AC1: entering a keep value and clicking Condense now emits `Condense`.
#[test]
fn clicking_condense_now_emits_condense_with_the_keep_value() {
    let source = InMemorySource::sample();
    let mut state = NavigatorState::default();
    state.select(&source.file_named("README.md"));

    let captured: Sink = Rc::new(RefCell::new(None));
    let mut harness = harness_with(state, Rc::clone(&captured));
    harness.run();

    // Focus the keep field (labelled_by "Condense keep:") before typing.
    harness.get_by_label_contains("Condense keep:").click();
    harness.run();
    harness
        .get_by_label_contains("Condense keep:")
        .type_text("1");
    harness.run();
    harness.get_by_label("Condense now").click();
    harness.run();

    let id = source.file_named("README.md").id();
    assert_eq!(
        *captured.borrow(),
        Some(FileAction::Condense { id, keep: 1 }),
        "Condense now with keep '1' emits Condense keep=1"
    );
}

/// AC1: clicking the per-file Remove emits `Remove` for that id with NO pending
/// batch-remove confirmation (a single file needs no confirmation).
#[test]
fn clicking_remove_emits_remove_for_the_active_file_with_no_pending_confirmation() {
    let source = InMemorySource::sample();
    let mut state = NavigatorState::default();
    state.select(&source.file_named("README.md"));

    let captured: Sink = Rc::new(RefCell::new(None));
    let mut harness = harness_with(state, Rc::clone(&captured));
    harness.run();

    harness.get_by_label("Remove").click();
    harness.run();

    let id = source.file_named("README.md").id();
    assert_eq!(
        *captured.borrow(),
        Some(FileAction::Remove { id }),
        "the per-file Remove emits a single-file Remove"
    );
    assert!(
        !harness.state().pending_batch_remove().is_pending(),
        "a single-file remove never parks a batch-remove confirmation"
    );
}

/// AC2: with two files selected, clicking batch Remove parks the ids and emits
/// NO action; a confirmation row renders; Confirm then emits `BatchRemove` with
/// both ids and clears the gate.
#[test]
fn batch_remove_requires_an_explicit_confirmation_before_removing() {
    let source = InMemorySource::sample();
    let mut state = NavigatorState::default();
    // Toggle two files into the multi-select set (batch enabled).
    state.toggle(source.file_named("README.md").id());
    state.toggle(source.file_named("data.csv").id());
    assert!(state.batch_enabled(), "precondition: two selected");

    let captured: Sink = Rc::new(RefCell::new(None));
    let mut harness = harness_with(state, Rc::clone(&captured));
    harness.run();

    harness.get_by_label("Batch remove").click();
    harness.run();

    // No BatchRemove yet; the gate is pending with both ids.
    assert!(
        captured.borrow().is_none(),
        "clicking Batch remove does NOT remove immediately"
    );
    assert!(
        harness.state().pending_batch_remove().is_pending(),
        "the batch remove is parked awaiting confirmation"
    );
    assert_eq!(
        harness.state().pending_batch_remove().ids().len(),
        2,
        "both selected ids are parked"
    );
    assert!(
        harness.query_by_label_contains("Remove 2 files?").is_some(),
        "a confirmation row is present"
    );

    // Confirm: emits BatchRemove with the two ids and clears the gate.
    harness.get_by_label("Confirm remove").click();
    harness.run();

    let expected = {
        let mut ids = vec![
            source.file_named("README.md").id(),
            source.file_named("data.csv").id(),
        ];
        ids.sort_unstable();
        ids
    };
    assert_eq!(
        *captured.borrow(),
        Some(FileAction::BatchRemove { ids: expected }),
        "Confirm emits BatchRemove with the parked ids"
    );
    assert!(
        !harness.state().pending_batch_remove().is_pending(),
        "Confirm clears the pending gate"
    );
}

/// AC2: Cancel on the batch-remove confirmation clears the gate and emits NO
/// action.
#[test]
fn cancelling_a_batch_remove_clears_the_gate_and_emits_no_action() {
    let source = InMemorySource::sample();
    let mut state = NavigatorState::default();
    state.toggle(source.file_named("README.md").id());
    state.toggle(source.file_named("data.csv").id());

    let captured: Sink = Rc::new(RefCell::new(None));
    let mut harness = harness_with(state, Rc::clone(&captured));
    harness.run();

    harness.get_by_label("Batch remove").click();
    harness.run();
    assert!(harness.state().pending_batch_remove().is_pending());

    harness.get_by_label("Cancel remove").click();
    harness.run();

    assert!(
        captured.borrow().is_none(),
        "cancel emits no removal action"
    );
    assert!(
        !harness.state().pending_batch_remove().is_pending(),
        "cancel clears the pending gate"
    );
}
