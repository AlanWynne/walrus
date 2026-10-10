//! Per-document export UI (design section 7.4).
//!
//! The offered formats come from `walrus_fmt::export::offered_formats(mime)`
//! (the single source of truth, pure data). For a chosen format this module
//! produces the export bytes by calling the matching `walrus_fmt::export::to_*`
//! backend, opens an `rfd` save dialog with a matching extension (Original uses
//! `document.name`), and writes the bytes. Backend/`ExportUnavailable` errors
//! and write failures are surfaced to the caller as a status string.
//!
//! walker builds `walrus-fmt` with `features = ["export-all"]`, so every backend
//! is present in the shipped binary.

use anyhow::{Context, Result};
use eframe::egui;
use walrus_fmt::export::{self, ExportFormat};

/// The default file extension for an export format (Original is handled by the
/// caller, which uses `document.name`).
fn extension_for(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Original => "",
        ExportFormat::PlainText => "txt",
        ExportFormat::Markdown => "md",
        ExportFormat::Html => "html",
        ExportFormat::Csv => "csv",
        ExportFormat::Json => "json",
        ExportFormat::Pdf => "pdf",
        ExportFormat::Docx => "docx",
        // `ExportFormat` is #[non_exhaustive]; a future variant falls back to a
        // generic extension until this match is extended.
        _ => "bin",
    }
}

/// A short label for the format in the export menu.
fn label_for(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Original => "Original",
        ExportFormat::PlainText => "Plain text",
        ExportFormat::Markdown => "Markdown",
        ExportFormat::Html => "HTML",
        ExportFormat::Csv => "CSV",
        ExportFormat::Json => "JSON",
        ExportFormat::Pdf => "PDF",
        ExportFormat::Docx => "DOCX",
        // `ExportFormat` is #[non_exhaustive]; label an unknown future variant.
        _ => "Export",
    }
}

/// Produce the export bytes for a reconstructed document in the chosen format.
///
/// `doc` is the already-reconstructed version bytes; `mime` is the document MIME
/// type. Errors (backend failure, unavailable feature) are returned as
/// `anyhow::Error` for the caller to show in the status bar.
pub fn export_bytes(format: ExportFormat, doc: &[u8], mime: &str) -> Result<Vec<u8>> {
    match format {
        ExportFormat::Original => Ok(export::to_original(doc)),
        ExportFormat::PlainText => Ok(String::from_utf8_lossy(doc).into_owned().into_bytes()),
        ExportFormat::Markdown => export::to_markdown(doc).context("markdown export failed"),
        ExportFormat::Html => export::to_html(doc, mime).context("HTML export failed"),
        ExportFormat::Csv => export::to_csv(doc).context("CSV export failed"),
        ExportFormat::Json => export::to_json(doc).context("JSON export failed"),
        ExportFormat::Pdf => export::to_pdf(doc, mime).context("PDF export failed"),
        ExportFormat::Docx => export::to_docx(doc, mime).context("DOCX export failed"),
        // `ExportFormat` is #[non_exhaustive]; reject an unknown future variant
        // rather than silently producing nothing.
        other => Err(anyhow::anyhow!("unsupported export format {other:?}")),
    }
}

/// The suggested save file name for a format, given the document's stored name.
///
/// Original keeps the stored name verbatim; others swap the extension.
pub fn suggested_name(format: ExportFormat, document_name: &str) -> String {
    if format == ExportFormat::Original {
        return document_name.to_string();
    }
    let stem = document_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(document_name);
    format!("{stem}.{}", extension_for(format))
}

/// Draw the export menu as a set of menu buttons; on a click, run the save
/// dialog and write the bytes. Returns a status message to show (success or
/// error) if an export was performed, else `None`.
///
/// The `rfd` native save dialog is MANUAL to verify (a real OS dialog that a
/// headless harness cannot drive); the byte production above is pure and tested.
pub fn menu(
    ui: &mut egui::Ui,
    offered: &[ExportFormat],
    doc: &[u8],
    mime: &str,
    document_name: &str,
) -> Option<String> {
    let mut status = None;
    for &format in offered {
        if ui.button(label_for(format)).clicked() {
            ui.close();
            status = Some(run_export(format, doc, mime, document_name));
        }
    }
    status
}

/// Produce bytes, open the save dialog, write the file; return a status string.
fn run_export(format: ExportFormat, doc: &[u8], mime: &str, document_name: &str) -> String {
    let bytes = match export_bytes(format, doc, mime) {
        Ok(bytes) => bytes,
        Err(e) => return format!("Export failed: {e:#}"),
    };
    let suggested = suggested_name(format, document_name);
    let Some(path) = rfd::FileDialog::new().set_file_name(&suggested).save_file() else {
        return "Export cancelled.".to_string();
    };
    match std::fs::write(&path, &bytes) {
        Ok(()) => format!("Exported {} to {}", label_for(format), path.display()),
        Err(e) => format!("Export write failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_keeps_the_stored_name_verbatim() {
        assert_eq!(
            suggested_name(ExportFormat::Original, "report.html"),
            "report.html"
        );
    }

    #[test]
    fn non_original_swaps_the_extension() {
        assert_eq!(suggested_name(ExportFormat::Pdf, "notes.txt"), "notes.pdf");
        assert_eq!(
            suggested_name(ExportFormat::Html, "notes.txt"),
            "notes.html"
        );
    }

    #[test]
    fn original_export_is_byte_identical() {
        let doc = b"the time has come";
        let out = export_bytes(ExportFormat::Original, doc, "text/plain").expect("original ok");
        assert_eq!(out, doc.to_vec());
    }

    #[test]
    fn plain_text_export_round_trips_utf8() {
        let doc = b"walrus";
        let out = export_bytes(ExportFormat::PlainText, doc, "text/plain").expect("plain ok");
        assert_eq!(out, doc.to_vec());
    }
}
