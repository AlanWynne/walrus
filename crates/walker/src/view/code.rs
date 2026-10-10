//! Code / structured-text view (design 2.2) via `egui_extras::syntax_highlighting`.
//!
//! JSON is pretty-printed with `serde_json` before highlighting (raw bytes on a
//! parse failure, keeping the forgiving contract). TOML and source languages are
//! highlighted as-is. An unknown language hint degrades to monospace inside the
//! highlighter, so no explicit fallback is needed.
//!
//! CONFIRMED against the pinned egui_extras 0.36 crate (design finding 2 flags
//! this as version-sensitive):
//!
//! ```ignore
//! egui_extras::syntax_highlighting::highlight(
//!     ctx: &egui::Context,
//!     style: &egui::Style,
//!     theme: &CodeTheme,
//!     code: &str,
//!     language: &str,
//! ) -> egui::text::LayoutJob
//! ```
//!
//! egui_extras 0.36 has NO `syntax_highlighting` cargo feature (the design named
//! one that does not exist); the module and `CodeTheme` are unconditional. The
//! optional `syntect` feature only upgrades quality, so the built-in fallback is
//! used here (design 2.5 defers the syntect upgrade).

use eframe::egui;
use egui_extras::syntax_highlighting::{highlight, CodeTheme};

use super::selector::CodeLang;

/// Prepare the source text for display: pretty-print JSON, pass others through.
///
/// Pure and testable: on a JSON parse failure the original text is returned
/// unchanged (forgiving contract).
pub fn prepare_source(lang: CodeLang, bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes).into_owned();
    match lang {
        CodeLang::Json => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value) => serde_json::to_string_pretty(&value).unwrap_or(text),
            Err(_) => text,
        },
        _ => text,
    }
}

/// Paint highlighted code for the given language.
pub fn paint(ui: &mut egui::Ui, lang: CodeLang, bytes: &[u8]) {
    let source = prepare_source(lang, bytes);
    let theme = CodeTheme::from_style(ui.style());
    let job = highlight(ui.ctx(), ui.style(), &theme, &source, lang.token());
    ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_pretty_printed() {
        let out = prepare_source(CodeLang::Json, br#"{"b":1,"a":2}"#);
        assert!(out.contains('\n'), "pretty JSON spans multiple lines");
        assert!(out.contains("\"b\": 1"), "keys are spaced after the colon");
    }

    #[test]
    fn invalid_json_is_returned_raw() {
        let raw = b"not json at all";
        let out = prepare_source(CodeLang::Json, raw);
        assert_eq!(out, "not json at all");
    }

    #[test]
    fn non_json_language_passes_through_unchanged() {
        let src = b"fn main() {}";
        let out = prepare_source(CodeLang::Rust, src);
        assert_eq!(out, "fn main() {}");
    }
}
