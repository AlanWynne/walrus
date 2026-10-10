//! Plain-text fallback view (design 2.2): read-only monospace. This is the
//! universal fallback for `text/plain` and any unrecognised document.

use eframe::egui;

/// Paint the document bytes as read-only monospace text.
pub fn paint(ui: &mut egui::Ui, bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut text_ref: &str = text.as_str();
    ui.add(
        egui::TextEdit::multiline(&mut text_ref)
            .code_editor()
            .desired_width(f32::INFINITY),
    );
}
