//! Theming via egui's built-in `Visuals` / `ThemePreference` (design section 6).
//!
//! `catppuccin-egui` is dropped (no third-party theme crate). The picker is a
//! small mode selector; the mode -> preference mapping is pure data so it is
//! unit-tested without pixels. The resulting concrete colours stay a manual
//! check (egui resolves them from the preference).

use eframe::egui;

/// The theme mode the user selects. Default is Dark (FFWB default-dark posture).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Follow the OS preference.
    System,
    /// Force light.
    Light,
    /// Force dark (default).
    #[default]
    Dark,
}

impl ThemeMode {
    /// All modes, in picker order.
    pub const ALL: [ThemeMode; 3] = [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark];

    /// A human label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::System => "System",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        }
    }

    /// Map the mode to an egui theme preference (pure data).
    pub fn to_preference(self) -> egui::ThemePreference {
        match self {
            ThemeMode::System => egui::ThemePreference::System,
            ThemeMode::Light => egui::ThemePreference::Light,
            ThemeMode::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// Apply the selected mode to the egui context.
pub fn apply(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_theme(mode.to_preference());
}

/// Draw the theme picker; applies the change on selection. Returns true if the
/// mode changed.
pub fn picker(ui: &mut egui::Ui, mode: &mut ThemeMode) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label("Theme")
        .selected_text(mode.label())
        .show_ui(ui, |ui| {
            for candidate in ThemeMode::ALL {
                if ui
                    .selectable_value(mode, candidate, candidate.label())
                    .clicked()
                {
                    changed = true;
                }
            }
        });
    if changed {
        apply(ui.ctx(), *mode);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_dark() {
        assert_eq!(ThemeMode::default(), ThemeMode::Dark);
    }

    #[test]
    fn system_mode_maps_to_system_preference() {
        assert_eq!(
            ThemeMode::System.to_preference(),
            egui::ThemePreference::System
        );
    }

    #[test]
    fn light_mode_maps_to_light_preference() {
        assert_eq!(
            ThemeMode::Light.to_preference(),
            egui::ThemePreference::Light
        );
    }

    #[test]
    fn dark_mode_maps_to_dark_preference() {
        assert_eq!(ThemeMode::Dark.to_preference(), egui::ThemePreference::Dark);
    }
}
