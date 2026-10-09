//! Headless egui_kittest harness test for walker transport UI.
//!
//! This proves the egui_kittest harness is wired in and can drive real walker
//! transport behaviour without a window, a GPU, or pixel snapshots. It renders a
//! minimal slice of the transport (a "step >" button plus a version label)
//! backed by a real `walker::player::Player`, clicks the button through the
//! accessibility tree, runs a FIXED, bounded number of frames, and asserts on
//! model state and label text -- never on pixels or frame counts.
//!
//! The native file-open path (`rfd::FileDialog`) stays MANUAL: it opens an
//! OS-native dialog that cannot be driven by a headless accesskit harness.

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walrus_fmt::{ChangeRecord, WalrusFile};

use walker::player::Player;

/// Build a deterministic in-memory player with a known number of versions.
///
/// Uses `WalrusFile::new_wal` so the test touches no disk and no fixture file
/// (testing steering: deterministic, no external files outside tests/fixtures).
/// Three splice records give `len() == 3`, so the index can advance 0 -> 1.
fn player_with_three_versions() -> Player {
    let base = b"walrus".to_vec();
    let log = vec![
        ChangeRecord::splice(0, 0, "a"),
        ChangeRecord::splice(0, 0, "b"),
        ChangeRecord::splice(0, 0, "c"),
    ];
    let file = WalrusFile::new_wal(base, log).expect("new_wal builds a valid in-memory file");
    Player::new(file)
}

/// The transport "step >" button advances the player's version index, and the
/// version label reflects the new index, driven entirely headlessly.
#[test]
fn step_forward_button_advances_version_index_headless() {
    let player = player_with_three_versions();
    assert_eq!(player.index(), 0, "player starts positioned at the base");

    // The ui closure renders the smallest real transport slice: a step button
    // and a label showing the current index. The Player is the harness state so
    // the test can read back `index()` after frames run.
    let mut harness = Harness::new_ui_state(
        |ui, player: &mut Player| {
            if ui.button("step >").clicked() {
                player.step_forward();
            }
            ui.label(format!("version {}", player.index()));
        },
        player,
    );

    // The initial label (built when the harness constructed the first frame)
    // shows the base index.
    assert!(
        harness.query_by_label("version 0").is_some(),
        "initial frame shows the base version label"
    );

    // Click the step button through the accessibility tree, then run a single
    // frame to process the queued click. One `run()` is a fixed, bounded step --
    // no loop-until-timeout.
    harness.get_by_label("step >").click();
    harness.run();

    // Model state advanced ...
    assert_eq!(
        harness.state().index(),
        1,
        "clicking 'step >' advanced the player one version"
    );
    // ... and the rendered label reflects the new index.
    assert!(
        harness.query_by_label("version 1").is_some(),
        "the version label re-rendered to the advanced index"
    );
}
