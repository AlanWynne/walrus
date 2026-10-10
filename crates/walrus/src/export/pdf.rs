//! PDF export backend (feature `export-pdf`).
//!
//! Uses `printpdf` with a standard-14 built-in font (Helvetica), so no external
//! font file is needed and the whole document is built in memory. CSS-perfect
//! layout is explicitly not a goal: the extracted text is laid out line by line
//! with simple flow pagination.

use std::io::BufWriter;

use printpdf::{BuiltinFont, Mm, PdfDocument};

/// A4 page size in millimetres (printpdf measures in `f32`).
const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;
/// Layout constants (millimetres / points).
const MARGIN_MM: f32 = 20.0;
const FONT_SIZE_PT: f32 = 11.0;
const LINE_HEIGHT_MM: f32 = 6.0;

/// Export document bytes as a PDF beginning with the `%PDF-` magic.
///
/// HTML sources (`mime` starts with `text/html`) have their text extracted via
/// the shared `export-html` helper when that feature is enabled; otherwise the
/// bytes are decoded lossily. The document is built fully in memory.
///
/// # Errors
/// Returns [`crate::WalrusError::Export`] if the PDF writer fails. Never panics.
pub fn to_pdf(doc: &[u8], mime: &str) -> crate::Result<Vec<u8>> {
    let text = extract_text(doc, mime);

    let (pdf, page1, layer1) = PdfDocument::new(
        "Walrus export",
        Mm(PAGE_WIDTH_MM),
        Mm(PAGE_HEIGHT_MM),
        "Layer 1",
    );
    let font =
        pdf.add_builtin_font(BuiltinFont::Helvetica)
            .map_err(|e| crate::WalrusError::Export {
                backend: "pdf",
                reason: e.to_string(),
            })?;

    let mut layer = pdf.get_page(page1).get_layer(layer1);
    let mut y = PAGE_HEIGHT_MM - MARGIN_MM;

    for line in text.lines() {
        if y < MARGIN_MM {
            let (page, new_layer) = pdf.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "Layer 1");
            layer = pdf.get_page(page).get_layer(new_layer);
            y = PAGE_HEIGHT_MM - MARGIN_MM;
        }
        layer.use_text(line, FONT_SIZE_PT, Mm(MARGIN_MM), Mm(y), &font);
        y -= LINE_HEIGHT_MM;
    }

    let mut buffer = Vec::new();
    {
        let mut writer = BufWriter::new(&mut buffer);
        pdf.save(&mut writer)
            .map_err(|e| crate::WalrusError::Export {
                backend: "pdf",
                reason: e.to_string(),
            })?;
    }
    Ok(buffer)
}

/// Extract display text from the document bytes for a given MIME type.
#[cfg(feature = "export-html")]
fn extract_text(doc: &[u8], mime: &str) -> String {
    if mime.trim().to_ascii_lowercase().starts_with("text/html") {
        super::html::extract_text(doc)
    } else {
        String::from_utf8_lossy(doc).into_owned()
    }
}

/// Fallback when `export-html` is not enabled: no HTML parser is available, so
/// decode lossily regardless of MIME.
#[cfg(not(feature = "export-html"))]
fn extract_text(doc: &[u8], _mime: &str) -> String {
    String::from_utf8_lossy(doc).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_nonempty_and_starts_with_pdf_magic() {
        let out = to_pdf(b"the walrus and the carpenter\nwere walking", "text/plain")
            .expect("pdf export");
        assert!(!out.is_empty());
        assert!(out.starts_with(b"%PDF-"), "missing %PDF- magic");
    }
}
