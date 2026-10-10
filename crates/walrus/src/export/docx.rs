//! DOCX export backend (feature `export-docx`).
//!
//! Builds a `.docx` (an OOXML zip) fully in memory via `docx-rs`. There is no
//! lightweight alternative and no hand-rolled OOXML: the only archive crate
//! otherwise in the graph is `tar`, which cannot produce the zip container a
//! `.docx` requires.

use std::io::Cursor;

use docx_rs::{Docx, Paragraph, Run};

/// Export document bytes as a `.docx` document.
///
/// HTML sources have their text extracted via the shared `export-html` helper
/// when that feature is enabled; otherwise bytes are decoded lossily. Each text
/// line becomes a paragraph. The output is a zip whose directory contains
/// `word/document.xml` and `[Content_Types].xml`.
///
/// # Errors
/// Returns [`crate::WalrusError::Export`] if the DOCX packer fails. Never
/// panics.
pub fn to_docx(doc: &[u8], mime: &str) -> crate::Result<Vec<u8>> {
    let text = extract_text(doc, mime);

    let mut document = Docx::new();
    for line in text.lines() {
        document = document.add_paragraph(Paragraph::new().add_run(Run::new().add_text(line)));
    }

    let mut cursor = Cursor::new(Vec::new());
    document
        .build()
        .pack(&mut cursor)
        .map_err(|e| crate::WalrusError::Export {
            backend: "docx",
            reason: e.to_string(),
        })?;
    Ok(cursor.into_inner())
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

/// Fallback when `export-html` is not enabled.
#[cfg(not(feature = "export-html"))]
fn extract_text(doc: &[u8], _mime: &str) -> String {
    String::from_utf8_lossy(doc).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read the central-directory file names from a zip archive in memory.
    fn zip_entry_names(bytes: &[u8]) -> Vec<String> {
        // Minimal central-directory scan: find each central file header
        // signature (PK\x01\x02) and read the name after the fixed 46-byte
        // header. Avoids pulling a zip crate into the test.
        let mut names = Vec::new();
        let sig = [0x50, 0x4B, 0x01, 0x02];
        let mut i = 0;
        while i + 46 <= bytes.len() {
            if bytes[i..i + 4] == sig {
                let name_len =
                    u16::from_le_bytes([bytes[i + 28], bytes[i + 29]]) as usize;
                let start = i + 46;
                let end = start + name_len;
                if end <= bytes.len() {
                    if let Ok(name) = std::str::from_utf8(&bytes[start..end]) {
                        names.push(name.to_string());
                    }
                }
                i = end;
            } else {
                i += 1;
            }
        }
        names
    }

    #[test]
    fn output_is_a_zip_containing_document_xml_and_content_types() {
        let out = to_docx(b"hello walrus", "text/plain").expect("docx export");
        assert!(out.starts_with(b"PK"), "not a zip archive");
        let names = zip_entry_names(&out);
        assert!(
            names.iter().any(|n| n == "word/document.xml"),
            "missing word/document.xml, got {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "[Content_Types].xml"),
            "missing [Content_Types].xml, got {names:?}"
        );
    }
}
