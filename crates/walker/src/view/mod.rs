//! The content-aware viewer tree.
//!
//! `selector` owns the pure renderer-selection logic (no egui); the other
//! modules each own one renderer. This module re-exports the public surface and
//! provides a thin dispatch that paints the chosen renderer -- it holds no
//! rendering logic of its own, only the match that delegates to the submodules.

pub mod code;
pub mod csv;
pub mod html;
mod html_token;
pub mod markdown;
pub mod richtext;
pub mod selector;
pub mod text;

pub use selector::{select_renderer, CodeLang, Renderer};

use eframe::egui;

/// Paint `bytes` with the chosen `renderer`. Thin dispatch: each arm delegates
/// to the owning submodule.
pub fn paint(ui: &mut egui::Ui, renderer: &Renderer, bytes: &[u8]) {
    match renderer {
        Renderer::Html => richtext::paint(ui, &html::lower(bytes)),
        Renderer::Markdown => richtext::paint(ui, &markdown::lower(bytes)),
        Renderer::Csv => match csv::parse_rows(bytes) {
            Ok(rows) => csv::paint(ui, &rows),
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, format!("CSV parse error: {e}"));
            }
        },
        Renderer::Code(lang) => code::paint(ui, *lang, bytes),
        Renderer::Text => text::paint(ui, bytes),
    }
}
