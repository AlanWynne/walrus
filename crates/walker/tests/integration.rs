//! Section 8 integration round-trip: open the narrated demo `.wal`, confirm the
//! viewer picks the text/code path, reconstruct a chosen version, and export it
//! to EACH offered format, asserting each backend's well-formed-output check.
//!
//! walker builds `walrus-fmt` with `features = ["export-all"]`, so every backend
//! is present here. On-disk assertions use `tempfile::TempDir` (testing
//! steering: no writes outside a temp dir).

use std::path::PathBuf;

use walrus_fmt::export::{self, ExportFormat};
use walrus_fmt::{Archive, FileId, Version};

use walker::export_ui;
use walker::view::{select_renderer, Renderer};

/// Absolute path to `examples/walrus-story.wal` (two levels up from the walker
/// crate manifest dir to the workspace root).
fn story_path() -> PathBuf {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the walker crate")
        .join("examples")
        .join("walrus-story.wal")
}

#[test]
fn story_wal_reconstructs_selects_text_and_exports_every_offered_format() {
    let path = story_path();
    let archive = Archive::open(&path).expect("the narrated demo .wal opens");

    // The demo's document mime is text/plain -> the Text (or Code) path.
    // Metadata is per-file in the multi-file layout; the narrated demo tracks a
    // single document, so read it from the first tracked file.
    let doc = archive
        .files()
        .next()
        .expect("the demo manifest tracks one file")
        .clone();
    assert_eq!(
        doc.mime, "text/plain",
        "the demo stores a text/plain document"
    );
    let renderer = select_renderer(&doc.mime, &doc.name);
    assert!(
        matches!(renderer, Renderer::Text | Renderer::Code(_)),
        "a text/plain document selects the Text or Code path, got {renderer:?}"
    );

    // Reconstruct the CURRENT version (index 0 in the direction-aware model) to
    // export. The file is addressed by its stable FileId.
    let file_id = FileId::from(doc.id.as_str());
    let bytes = archive
        .reconstruct(&file_id, Version::Index(0))
        .expect("the current version reconstructs");
    assert!(!bytes.is_empty(), "the reconstructed version is non-empty");

    // Every offered format must export with a well-formed result.
    let offered = export::offered_formats(&doc.mime);
    for format in [
        ExportFormat::Original,
        ExportFormat::PlainText,
        ExportFormat::Html,
        ExportFormat::Markdown,
        ExportFormat::Pdf,
        ExportFormat::Docx,
    ] {
        assert!(
            offered.contains(&format),
            "{format:?} is offered for a text/plain document"
        );
        let out = export_ui::export_bytes(format, &bytes, &doc.mime)
            .unwrap_or_else(|e| panic!("{format:?} export failed: {e:#}"));
        assert_well_formed(format, &out, &bytes);
    }

    // Original, written under document.name into a temp dir, is byte-identical.
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let name = export_ui::suggested_name(ExportFormat::Original, &doc.name);
    assert_eq!(name, doc.name, "Original keeps the stored name verbatim");
    let original = export_ui::export_bytes(ExportFormat::Original, &bytes, &doc.mime)
        .expect("original export");
    let out_path = tmp.path().join(&name);
    std::fs::write(&out_path, &original).expect("write original export");
    let read_back = std::fs::read(&out_path).expect("read original export");
    assert_eq!(
        read_back, bytes,
        "Original reproduces the stored bytes exactly"
    );
}

/// Assert the per-format well-formed-output check from design section 8.
fn assert_well_formed(format: ExportFormat, out: &[u8], source: &[u8]) {
    match format {
        ExportFormat::Original => {
            assert_eq!(out, source, "Original is byte-identical to the source");
        }
        ExportFormat::PlainText => {
            assert!(!out.is_empty(), "plain text is non-empty");
        }
        ExportFormat::Html => {
            let text = String::from_utf8_lossy(out).to_ascii_lowercase();
            assert!(text.contains("<!doctype html>"), "HTML has a doctype");
            assert!(
                text.contains("<html") && text.contains("</html>"),
                "HTML has a balanced html wrapper"
            );
        }
        ExportFormat::Markdown => {
            assert!(!out.is_empty(), "markdown is non-empty");
        }
        ExportFormat::Pdf => {
            assert!(out.starts_with(b"%PDF-"), "PDF starts with the %PDF- magic");
        }
        ExportFormat::Docx => {
            // A .docx is a zip (starts with PK) containing word/document.xml.
            assert!(out.starts_with(b"PK"), "DOCX is a zip archive");
            assert!(
                zip_contains(out, "word/document.xml"),
                "DOCX zip contains word/document.xml"
            );
        }
        other => panic!("unexpected format in round-trip: {other:?}"),
    }
}

/// Scan a zip archive's bytes for a stored file name (central-directory names
/// appear verbatim in the uncompressed structure, which is enough to assert the
/// expected part is present without a zip-reader dependency).
fn zip_contains(zip: &[u8], name: &str) -> bool {
    let needle = name.as_bytes();
    zip.windows(needle.len()).any(|w| w == needle)
}
