//! Export a reconstructed version to another format.
//!
//! The metadata surface here -- [`ExportFormat`], [`ExportUnavailable`],
//! [`offered_formats`], and [`to_original`] -- is ALWAYS compiled, with no
//! dependency beyond the standard library, so a playback-only embedder can ask
//! which formats a document could be exported to and perform a verbatim
//! "Original" export without enabling any heavy backend.
//!
//! The heavy backends (`to_html`, `to_markdown`, `to_csv`, `to_json`,
//! `to_pdf`, `to_docx`) are each behind an opt-in cargo feature so the
//! embeddable library stays lean (project-conventions invariant 2). The
//! functions take the already-reconstructed document bytes from
//! [`crate::Archive::reconstruct`].

// === Always-compiled modules ========================================
mod original;

pub use original::to_original;

// === Feature-gated backend modules ==================================
#[cfg(feature = "export-html")]
mod html;
#[cfg(feature = "export-html")]
pub use html::to_html;

#[cfg(feature = "export-md")]
mod markdown;
#[cfg(feature = "export-md")]
pub use markdown::to_markdown;

#[cfg(feature = "export-csv")]
mod csv;
#[cfg(feature = "export-csv")]
pub use csv::to_csv;

#[cfg(feature = "export-json")]
mod json;
#[cfg(feature = "export-json")]
pub use json::to_json;

#[cfg(feature = "export-pdf")]
mod pdf;
#[cfg(feature = "export-pdf")]
pub use pdf::to_pdf;

#[cfg(feature = "export-docx")]
mod docx;
#[cfg(feature = "export-docx")]
pub use docx::to_docx;

// === Metadata surface (always compiled) =============================

/// The set of export targets Walrus can offer for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportFormat {
    /// The stored document, written back verbatim under its original name.
    Original,
    /// Plain UTF-8 text (lossy passthrough).
    PlainText,
    /// Markdown.
    Markdown,
    /// HTML document.
    Html,
    /// Comma-separated values (for tabular documents).
    Csv,
    /// JSON wrapper carrying the text and metadata.
    Json,
    /// PDF (feature `export-pdf`).
    Pdf,
    /// DOCX (feature `export-docx`).
    Docx,
}

/// Error returned when an export target is not available in this build.
#[derive(Debug)]
pub struct ExportUnavailable {
    /// The requested format.
    pub format: ExportFormat,
}

impl std::fmt::Display for ExportUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "export format {:?} is not enabled in this build (enable its cargo feature)",
            self.format
        )
    }
}

impl std::error::Error for ExportUnavailable {}

/// Normalise a MIME type: lowercase and strip any `;` parameters.
fn normalise_mime(mime: &str) -> String {
    let base = mime.split(';').next().unwrap_or("");
    base.trim().to_ascii_lowercase()
}

/// The export formats appropriate to a document of the given MIME type.
///
/// This is pure data and needs NO export feature compiled: it is the single
/// source of truth both the GUI and any other embedder consult so they agree on
/// which formats a document type offers. `Original`, `PlainText`, `Html`,
/// `Json`, `Pdf`, and `Docx` are always offered; `Markdown` is added for
/// markdown and plain-text sources; `Csv` is offered ONLY for `text/csv`.
pub fn offered_formats(mime: &str) -> Vec<ExportFormat> {
    let normalised = normalise_mime(mime);
    let mut formats = vec![
        ExportFormat::Original,
        ExportFormat::PlainText,
        ExportFormat::Html,
        ExportFormat::Json,
        ExportFormat::Pdf,
        ExportFormat::Docx,
    ];

    let is_markdown = matches!(normalised.as_str(), "text/markdown" | "text/x-markdown");
    let is_plain = normalised == "text/plain" || normalised.is_empty();
    if is_markdown || is_plain {
        formats.push(ExportFormat::Markdown);
    }

    if normalised == "text/csv" {
        formats.push(ExportFormat::Csv);
    }

    formats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offered_formats_for_csv_includes_csv() {
        let formats = offered_formats("text/csv");
        assert!(formats.contains(&ExportFormat::Csv));
    }

    #[test]
    fn offered_formats_for_prose_omits_csv() {
        let formats = offered_formats("text/plain");
        assert!(!formats.contains(&ExportFormat::Csv));
    }

    #[test]
    fn offered_formats_always_includes_original_and_plaintext() {
        for mime in [
            "text/plain",
            "text/csv",
            "text/markdown",
            "application/json",
        ] {
            let formats = offered_formats(mime);
            assert!(formats.contains(&ExportFormat::Original), "mime {mime}");
            assert!(formats.contains(&ExportFormat::PlainText), "mime {mime}");
        }
    }

    #[test]
    fn offered_formats_for_markdown_source_adds_markdown() {
        assert!(offered_formats("text/markdown").contains(&ExportFormat::Markdown));
    }

    #[test]
    fn offered_formats_for_plain_source_adds_markdown() {
        assert!(offered_formats("text/plain").contains(&ExportFormat::Markdown));
    }

    #[test]
    fn offered_formats_ignores_mime_parameters_and_case() {
        let formats = offered_formats("TEXT/CSV; charset=utf-8");
        assert!(formats.contains(&ExportFormat::Csv));
    }

    #[test]
    fn to_original_returns_input_unchanged() {
        let doc = b"the walrus and the carpenter";
        assert_eq!(to_original(doc), doc.to_vec());
    }
}
