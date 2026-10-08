//! Export a reconstructed version to another format.
//!
//! Each target is behind an opt-in cargo feature so a playback-only embedder
//! does not pull in heavy dependencies (PDF/DOCX in particular). The functions
//! here take the already-reconstructed document bytes from
//! [`crate::WalrusFile::reconstruct`].
//!
//! Status: text/markdown/html/csv/json are straightforward and stubbed to the
//! intended shape; pdf/docx are placeholders pending a chosen backend crate.

/// The set of export targets Walrus intends to support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportFormat {
    /// Plain UTF-8 text (passthrough).
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

#[cfg(feature = "export-md")]
/// Export document bytes as Markdown. For a plain-text base this is a fenced or
/// passthrough rendering; richer mapping is a TODO.
pub fn to_markdown(doc: &[u8]) -> Vec<u8> {
    doc.to_vec()
}

#[cfg(feature = "export-html")]
/// Export document bytes as a minimal HTML document.
pub fn to_html(doc: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(doc);
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<!doctype html>\n<html><body><pre>{escaped}</pre></body></html>\n").into_bytes()
}

#[cfg(feature = "export-json")]
/// Export document bytes wrapped in a JSON object.
pub fn to_json(doc: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(doc);
    serde_json::to_vec_pretty(&serde_json::json!({ "text": text }))
        .unwrap_or_else(|_| b"{}".to_vec())
}
