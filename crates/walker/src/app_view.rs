//! App-shell view helpers: renderer selection for the navigator's active file,
//! per-file document metadata extraction, and the status-bar renderer label.
//!
//! Split out of `app.rs` (as a `mod app_view;` child) so that file stays under
//! the 400 non-test-line standard (NFR-1). These are pure helpers over the
//! format-agnostic seam and the per-file manifest; they hold no app state.

use crate::navigator::NavigatorState;
use crate::source::HistorySource;
use crate::view::{self, Renderer};

/// The renderer for the navigator's active file, chosen through the seam.
///
/// Falls back to [`Renderer::Text`] when there is no active selection.
pub(super) fn navigator_renderer(
    source: &dyn HistorySource,
    navigator: &NavigatorState,
) -> Renderer {
    match navigator.active() {
        Some(active) => view::present::select_renderer_for(source, active),
        None => Renderer::Text,
    }
}

/// A short label for a renderer, for the status bar.
pub(super) fn renderer_label(renderer: &Renderer) -> &'static str {
    match renderer {
        Renderer::Html => "HTML",
        Renderer::Markdown => "Markdown",
        Renderer::Csv => "CSV",
        Renderer::Code(_) => "Code",
        Renderer::Text => "Text",
    }
}
