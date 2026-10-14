//! Trait-driven presentation glue (FR-K8).
//!
//! These functions are the single place that connects a [`HistorySource`] to
//! the content viewer tree: the renderer is chosen from the source's
//! `content_hint`, and the bytes painted come from the source's `reconstruct`.
//! Keeping this glue separate from the pure `selector` logic and the per-format
//! renderers means `app.rs` (and, later, any `HistorySource` adapter) wires the
//! viewer through one byte path rather than reaching into a concrete format.
//!
//! This module takes `&dyn HistorySource` only -- it never names a concrete
//! backend, so it is exercised end-to-end against the in-memory fixture source
//! with no `walrus-fmt` archive involved.

use eframe::egui;

use crate::source::{FileRef, HistorySource, VersionId};
use crate::view::{self, Renderer};

/// Choose the renderer for `file` from the source's content hint.
///
/// The `(mime, name)` pair returned by [`HistorySource::content_hint`] is the
/// sole input to [`view::select_renderer`], so renderer choice is driven by the
/// source, not by any format-specific knowledge in the viewer.
pub fn select_renderer_for(source: &dyn HistorySource, file: &FileRef) -> Renderer {
    let (mime, name) = source.content_hint(file);
    view::select_renderer(&mime, &name)
}

/// Reconstruct `file` at `version` through the source and paint it.
///
/// The renderer is chosen via [`select_renderer_for`] and the bytes come from
/// [`HistorySource::reconstruct`]; a reconstruction error is shown inline
/// (the viewer never panics on a bad version). The untrusted-content
/// guarantees are entirely owned by the renderers this delegates to -- this
/// glue adds no fetch, script, or image handling of its own.
pub fn render(ui: &mut egui::Ui, source: &dyn HistorySource, file: &FileRef, version: &VersionId) {
    let renderer = select_renderer_for(source, file);
    match source.reconstruct(file, version) {
        Ok(bytes) => view::paint(ui, &renderer, &bytes),
        Err(e) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Reconstruction error: {e}"),
            );
        }
    }
}

// === Tests ==========================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::testsupport::InMemorySource;
    use crate::view::CodeLang;

    #[test]
    fn select_renderer_picks_markdown_for_text_markdown_hint() {
        let source = InMemorySource::sample();
        let file = source.file_named("README.md");
        assert_eq!(select_renderer_for(&source, &file), Renderer::Markdown);
    }

    #[test]
    fn select_renderer_picks_csv_for_text_csv_hint() {
        let source = InMemorySource::sample();
        let file = source.file_named("data.csv");
        assert_eq!(select_renderer_for(&source, &file), Renderer::Csv);
    }

    #[test]
    fn select_renderer_picks_html_for_text_html_hint() {
        let source = InMemorySource::sample();
        let file = source.file_named("page.html");
        assert_eq!(select_renderer_for(&source, &file), Renderer::Html);
    }

    #[test]
    fn select_renderer_picks_code_for_application_json_hint() {
        let source = InMemorySource::sample();
        let file = source.file_named("data.json");
        assert_eq!(
            select_renderer_for(&source, &file),
            Renderer::Code(CodeLang::Json)
        );
    }

    #[test]
    fn select_renderer_picks_text_for_plain_hint() {
        let source = InMemorySource::sample();
        let file = source.file_named("notes.txt");
        assert_eq!(select_renderer_for(&source, &file), Renderer::Text);
    }
}
