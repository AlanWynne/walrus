//! walker -- an egui viewer and auto-player for .wal / .wbl files.
//!
//! Browse versions, auto-play forward or backward with loop and speed, export a
//! selected version, and view CSV files as columns. The format and playback
//! logic live in `walrus_fmt` and the `player` module respectively.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use walker::app::WalkerApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([900.0, 640.0])
            .with_title("Walker"),
        ..Default::default()
    };
    eframe::run_native(
        "Walker",
        options,
        Box::new(|cc| Ok(Box::new(WalkerApp::new(cc)))),
    )
}
