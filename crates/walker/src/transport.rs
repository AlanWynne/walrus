//! Transport controls row and the playback speed control (design section 4).
//!
//! Speed is expressed as a MULTIPLIER against a 400 ms base interval. The
//! conversion is a pure free function so it is unit-tested without egui; the
//! player clamps the resulting interval to its 16 ms minimum, so even extreme
//! multipliers are safe.

use std::time::Duration;

use eframe::egui;

use crate::player::{Direction, Player};

/// The base per-step interval at 1x speed.
const BASE_INTERVAL_SECS: f64 = 0.400;

/// Preset speed multipliers offered as a segmented stepper.
const PRESETS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// The per-step interval for a speed multiplier. Pure and testable.
///
/// Higher multiplier -> shorter interval -> faster playback. At 1x this is the
/// 400 ms base; 4x is 100 ms; 0.25x is 1600 ms.
pub fn interval_for_multiplier(mult: f64) -> Duration {
    Duration::from_secs_f64(BASE_INTERVAL_SECS / mult)
}

/// Apply a speed multiplier to the player (the player clamps the minimum).
pub fn set_speed_multiplier(player: &mut Player, mult: f64) {
    player.set_step_interval(interval_for_multiplier(mult));
}

/// Draw the transport controls and speed control. Returns true if the version
/// changed (so the caller refreshes the displayed document). `multiplier` holds
/// the current speed across frames.
pub fn controls(ui: &mut egui::Ui, player: &mut Player, multiplier: &mut f64) -> bool {
    let mut changed = false;

    // The timeline reads left-to-right as oldest -> current: index 0 (current)
    // is the right-hand end and the highest index (oldest) is the left-hand
    // end. "older" steps back in time (increasing the index); "newer" steps
    // forward in time (decreasing the index).
    ui.horizontal(|ui| {
        if ui.button("|< oldest").clicked() {
            player.seek(player.len());
            changed = true;
        }
        if ui.button("< older").clicked() {
            changed |= player.step_older();
        }
        let play_label = if player.is_playing() { "pause" } else { "play" };
        if ui.button(play_label).clicked() {
            if player.is_playing() {
                player.pause();
            } else {
                player.play();
            }
        }
        if ui.button("newer >").clicked() {
            changed |= player.step_newer();
        }
        if ui.button("current >|").clicked() {
            player.seek(0);
            changed = true;
        }
    });

    ui.horizontal(|ui| {
        let mut older = player.direction() == Direction::Older;
        if ui.selectable_label(older, "older").clicked() {
            older = true;
            player.set_direction(Direction::Older);
        }
        if ui.selectable_label(!older, "newer").clicked() {
            player.set_direction(Direction::Newer);
        }
        let mut looping = player.looping();
        if ui.checkbox(&mut looping, "loop").changed() {
            player.set_looping(looping);
        }
    });

    speed_control(ui, player, multiplier);

    // Seek slider across the version range.
    let len = player.len();
    if len > 0 {
        let mut idx = player.index();
        if ui
            .add(egui::Slider::new(&mut idx, 0..=len).text("version"))
            .changed()
        {
            player.seek(idx);
            changed = true;
        }
    }

    changed
}

/// Draw the speed presets and the fine slider; apply changes to the player.
fn speed_control(ui: &mut egui::Ui, player: &mut Player, multiplier: &mut f64) {
    ui.horizontal(|ui| {
        ui.label("speed:");
        for preset in PRESETS {
            let selected = (*multiplier - preset).abs() < f64::EPSILON;
            if ui
                .selectable_label(selected, format!("{preset}x"))
                .clicked()
            {
                *multiplier = preset;
                set_speed_multiplier(player, *multiplier);
            }
        }
        if ui
            .add(
                egui::Slider::new(multiplier, 0.25..=4.0)
                    .logarithmic(true)
                    .text("x"),
            )
            .changed()
        {
            set_speed_multiplier(player, *multiplier);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_x_is_the_four_hundred_millisecond_base() {
        assert_eq!(interval_for_multiplier(1.0), Duration::from_millis(400));
    }

    #[test]
    fn four_x_is_one_hundred_milliseconds() {
        assert_eq!(interval_for_multiplier(4.0), Duration::from_millis(100));
    }

    #[test]
    fn quarter_x_is_sixteen_hundred_milliseconds() {
        assert_eq!(interval_for_multiplier(0.25), Duration::from_millis(1600));
    }
}
