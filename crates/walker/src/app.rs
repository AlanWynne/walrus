//! The egui application shell for Walker.
//!
//! Responsibilities (scaffold):
//!   - open a `.wal` / `.wbl` file via a native dialog,
//!   - show the current version text,
//!   - transport controls: step back/forward, play/pause, direction, loop, speed,
//!   - a seek slider across the version range,
//!   - export the selected version (wired to walrus-fmt export features),
//!   - a CSV column viewer tab for `.csv` inputs.
//!
//! The heavy lifting (reconstruct/playback) lives in `walrus_fmt` and `player`.

use std::time::Instant;

use eframe::egui;
use walrus_fmt::WalrusFile;

use crate::player::{Direction, Player};

/// Which view the app is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// WAL/WBL playback view.
    Player,
    /// CSV column viewer.
    Csv,
}

/// The Walker application state.
pub struct WalkerApp {
    view: View,
    player: Option<Player>,
    /// Cached text of the current version, refreshed on change.
    current_text: String,
    status: String,
    last_frame: Instant,
    /// Loaded CSV rows for the column viewer.
    csv_rows: Vec<Vec<String>>,
}

impl Default for WalkerApp {
    fn default() -> Self {
        Self {
            view: View::Player,
            player: None,
            current_text: String::new(),
            status: "Open a .wal or .wbl file to begin.".to_string(),
            last_frame: Instant::now(),
            csv_rows: Vec::new(),
        }
    }
}

impl WalkerApp {
    /// Construct the app for `eframe`.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// Open a Walrus file and position at the base.
    fn open_file(&mut self, path: std::path::PathBuf) {
        match WalrusFile::open(&path) {
            Ok(file) => {
                let mut player = Player::new(file);
                player.seek(0);
                self.refresh_text(&player);
                self.status = format!("Opened {} ({} versions)", path.display(), player.len());
                self.player = Some(player);
                self.view = View::Player;
            }
            Err(e) => {
                self.status = format!("Failed to open {}: {e}", path.display());
            }
        }
    }

    /// Refresh the cached current-version text from the player.
    fn refresh_text(&mut self, player: &Player) {
        match player.current_bytes() {
            Ok(bytes) => self.current_text = String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => self.current_text = format!("<reconstruction error: {e}>"),
        }
    }

    /// Draw the transport controls; returns true if the version changed.
    fn transport_controls(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(player) = self.player.as_mut() else {
            ui.label("No file loaded.");
            return false;
        };
        let mut changed = false;

        ui.horizontal(|ui| {
            if ui.button("|< base").clicked() {
                player.seek(0);
                changed = true;
            }
            if ui.button("< step").clicked() {
                changed |= player.step_backward();
            }
            let play_label = if player.is_playing() { "pause" } else { "play" };
            if ui.button(play_label).clicked() {
                if player.is_playing() {
                    player.pause();
                } else {
                    player.play();
                }
            }
            if ui.button("step >").clicked() {
                changed |= player.step_forward();
            }
            if ui.button("head >|").clicked() {
                player.seek(player.len());
                changed = true;
            }
        });

        ui.horizontal(|ui| {
            let mut forward = player.direction() == Direction::Forward;
            if ui.selectable_label(forward, "forward").clicked() {
                forward = true;
                player.set_direction(Direction::Forward);
            }
            if ui.selectable_label(!forward, "backward").clicked() {
                player.set_direction(Direction::Backward);
            }
            let mut looping = player.looping();
            if ui.checkbox(&mut looping, "loop").changed() {
                player.set_looping(looping);
            }
        });

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
}

impl eframe::App for WalkerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Advance playback by real elapsed time.
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_frame);
        self.last_frame = now;

        let mut version_changed = false;
        if let Some(player) = self.player.as_mut() {
            version_changed = player.tick(elapsed);
            if player.is_playing() {
                // Keep repainting while playing so the animation runs.
                ctx.request_repaint();
            }
        }
        if version_changed {
            if let Some(player) = self.player.as_ref() {
                // Avoid a double mutable borrow by reconstructing from a clone-free ref.
                match player.current_bytes() {
                    Ok(bytes) => self.current_text = String::from_utf8_lossy(&bytes).into_owned(),
                    Err(e) => self.current_text = format!("<reconstruction error: {e}>"),
                }
            }
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Open...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Walrus", &["wal", "wbl"])
                        .add_filter("CSV", &["csv"])
                        .pick_file()
                    {
                        self.open_file(path);
                    }
                }
                ui.separator();
                ui.selectable_value(&mut self.view, View::Player, "Player");
                ui.selectable_value(&mut self.view, View::Csv, "CSV viewer");
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(&self.status);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.view {
            View::Player => {
                let changed = self.transport_controls(ui);
                if changed {
                    if let Some(player) = self.player.as_ref() {
                        match player.current_bytes() {
                            Ok(bytes) => {
                                self.current_text = String::from_utf8_lossy(&bytes).into_owned()
                            }
                            Err(e) => self.current_text = format!("<reconstruction error: {e}>"),
                        }
                    }
                }
                ui.separator();
                egui::ScrollArea::both().show(ui, |ui| {
                    // Read-only display of the current version.
                    let mut text: &str = self.current_text.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .code_editor()
                            .desired_width(f32::INFINITY),
                    );
                });
            }
            View::Csv => {
                // TODO: render self.csv_rows as an aligned, sortable column grid.
                ui.heading("CSV column viewer");
                if self.csv_rows.is_empty() {
                    ui.label("Open a .csv file to view it as columns. (viewer TODO)");
                }
            }
        });
    }
}
