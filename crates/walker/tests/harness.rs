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

use std::time::Duration;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

use walrus_fmt::{ChangeRecord, WalrusFile};

use walker::player::Player;
use walker::transport;

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

/// FR-K3 AC1: a "step older" action INCREASES the model index (index 0 is the
/// current version; a higher index is older), driven entirely headlessly.
#[test]
fn step_older_button_increases_version_index_headless() {
    let player = player_with_three_versions();
    assert_eq!(
        player.index(),
        0,
        "player starts at the current version (index 0)"
    );

    // The ui closure renders the smallest real transport slice: a step button
    // and a label showing the current index. The Player is the harness state so
    // the test can read back `index()` after frames run.
    let mut harness = Harness::new_ui_state(
        |ui, player: &mut Player| {
            if ui.button("< older").clicked() {
                player.step_older();
            }
            ui.label(format!("version {}", player.index()));
        },
        player,
    );

    // The initial label (built when the harness constructed the first frame)
    // shows the current index.
    assert!(
        harness.query_by_label("version 0").is_some(),
        "initial frame shows the current version label"
    );

    // Click the step button through the accessibility tree, then run a single
    // frame to process the queued click. One `run()` is a fixed, bounded step --
    // no loop-until-timeout.
    harness.get_by_label("< older").click();
    harness.run();

    // Model state moved to an OLDER version ...
    assert_eq!(
        harness.state().index(),
        1,
        "clicking 'older' INCREASED the index toward older versions"
    );
    // ... and the rendered label reflects the new index.
    assert!(
        harness.query_by_label("version 1").is_some(),
        "the version label re-rendered to the older index"
    );
}

/// FR-K3 AC1: a "step newer" action DECREASES the model index, driven headlessly.
#[test]
fn step_newer_button_decreases_version_index_headless() {
    let mut player = player_with_three_versions();
    player.seek(2); // start on an older version so there is room to go newer.
    assert_eq!(player.index(), 2, "player starts on an older version");

    let mut harness = Harness::new_ui_state(
        |ui, player: &mut Player| {
            if ui.button("newer >").clicked() {
                player.step_newer();
            }
            ui.label(format!("version {}", player.index()));
        },
        player,
    );

    assert!(
        harness.query_by_label("version 2").is_some(),
        "initial frame shows the older version label"
    );

    harness.get_by_label("newer >").click();
    harness.run();

    assert_eq!(
        harness.state().index(),
        1,
        "clicking 'newer' DECREASED the index toward newer versions"
    );
    assert!(
        harness.query_by_label("version 1").is_some(),
        "the version label re-rendered to the newer index"
    );
}

/// FR-K3 AC1: seeking to index 0 shows the CURRENT version, driven headlessly.
#[test]
fn seek_to_zero_shows_the_current_version_headless() {
    let mut player = player_with_three_versions();
    player.seek(3); // park on the oldest version first.
    assert_eq!(player.index(), 3, "player starts on the oldest version");

    let mut harness = Harness::new_ui_state(
        |ui, player: &mut Player| {
            if ui.button("current >|").clicked() {
                player.seek(0);
            }
            ui.label(format!("version {}", player.index()));
        },
        player,
    );

    assert!(
        harness.query_by_label("version 3").is_some(),
        "initial frame shows the oldest version label"
    );

    harness.get_by_label("current >|").click();
    harness.run();

    assert_eq!(
        harness.state().index(),
        0,
        "seeking to 0 shows the current version"
    );
    assert!(
        harness.query_by_label("version 0").is_some(),
        "the version label re-rendered to the current index 0"
    );
}

/// FR-K3 AC3: the real transport renders time-oriented labels that read
/// correctly under index-0-is-current. There must be NO leftover
/// "forward = toward head = newer" wording from the old single-file model: the
/// buttons read "oldest / older / newer / current", and the direction toggle
/// reads "older / newer" -- asserted on the accessibility tree, not pixels.
#[test]
fn transport_labels_read_correctly_under_index_zero_is_current() {
    let player = player_with_three_versions();
    let mut multiplier = 1.0_f64;
    let mut harness = Harness::new_ui_state(
        move |ui, player: &mut Player| {
            transport::controls(ui, player, &mut multiplier);
        },
        player,
    );
    harness.run();

    for label in [
        "|< oldest",
        "< older",
        "newer >",
        "current >|",
        "older",
        "newer",
    ] {
        assert!(
            harness.query_by_label(label).is_some(),
            "the transport renders the '{label}' control under index-0-is-current"
        );
    }

    // The old single-file labels MUST be gone -- their presence is the leftover
    // the acceptance criterion forbids.
    for stale in [
        "|< base", "head >|", "forward", "backward", "step >", "< step",
    ] {
        assert!(
            harness.query_by_label(stale).is_none(),
            "the stale single-file label '{stale}' must not appear"
        );
    }
}

/// Clicking a speed preset drives the player's model interval through the real
/// `transport::set_speed_multiplier`, asserted headlessly on model state (not
/// pixels). 4x over the 400 ms base is a 100 ms interval.
#[test]
fn speed_preset_button_drives_model_interval_headless() {
    let player = player_with_three_versions();
    assert_eq!(
        player.step_interval(),
        Duration::from_millis(400),
        "player starts at the 400 ms (1x) base interval"
    );

    // The ui closure renders a real "4x" speed preset button wired to the
    // transport speed setter, plus a label echoing the model interval so the
    // test can assert on rendered state too.
    let mut harness = Harness::new_ui_state(
        |ui, player: &mut Player| {
            if ui.button("4x").clicked() {
                transport::set_speed_multiplier(player, 4.0);
            }
            ui.label(format!("interval {}ms", player.step_interval().as_millis()));
        },
        player,
    );

    assert!(
        harness.query_by_label("interval 400ms").is_some(),
        "initial frame shows the 400 ms base interval"
    );

    harness.get_by_label("4x").click();
    harness.run();

    assert_eq!(
        harness.state().step_interval(),
        Duration::from_millis(100),
        "clicking '4x' set the model interval to 100 ms"
    );
    assert!(
        harness.query_by_label("interval 100ms").is_some(),
        "the interval label re-rendered to 100 ms"
    );
}
