//! HTML export backend (feature `export-html`).
//!
//! Output is always a complete, minimal document beginning with
//! `<!doctype html>`. A Markdown source is rendered through `pulldown-cmark`
//! into the acceptable subset (no scripts, no remote fetches); any other source
//! has its text escaped and wrapped in a `<pre>` block so untrusted bytes can
//! never inject markup.

use pulldown_cmark::{html, Event, Options, Parser};
use scraper::Html;

/// Export document bytes as a minimal, self-contained HTML document.
///
/// When `mime` is `text/markdown` (or `text/x-markdown`) the bytes are parsed
/// as Markdown and rendered to an HTML fragment wrapped in a document shell.
/// Otherwise the text is HTML-escaped and placed in a `<pre>` block. Bytes are
/// decoded lossily, so this never panics.
///
/// # Errors
/// Currently infallible, but returns [`crate::Result`] to match the backend
/// contract.
pub fn to_html(doc: &[u8], mime: &str) -> crate::Result<Vec<u8>> {
    let text = String::from_utf8_lossy(doc);
    let normalised = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();

    let body = if matches!(normalised.as_str(), "text/markdown" | "text/x-markdown") {
        render_markdown(&text)
    } else {
        format!("<pre>{}</pre>", escape(&text))
    };

    let document =
        format!("<!doctype html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n<body>\n{body}\n</body>\n</html>\n");
    Ok(document.into_bytes())
}

/// Render Markdown to an HTML fragment using the acceptable subset.
///
/// Raw HTML in the source (block or inline) is DROPPED rather than passed
/// through, so stored `<script>` or other markup from an untrusted document can
/// never survive into the exported HTML (untrusted-content rule).
fn render_markdown(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, Options::empty())
        .filter(|event| !matches!(event, Event::Html(_) | Event::InlineHtml(_)));
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// HTML-escape the three characters that can break out of text content.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Extract plain text from an HTML document, used by the PDF/DOCX backends.
///
/// Tags are dropped and their text content concatenated with single spaces.
/// This lives behind `export-html` because it needs an HTML parser; the heavy
/// backends fall back to lossy UTF-8 when `export-html` is not enabled.
pub(crate) fn extract_text(html_bytes: &[u8]) -> String {
    let source = String::from_utf8_lossy(html_bytes);
    let document = Html::parse_document(&source);
    let pieces: Vec<String> = document
        .root_element()
        .text()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    pieces.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_escaped_and_wrapped() {
        let out = to_html(b"a < b && c > d", "text/plain").expect("html export");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.starts_with("<!doctype html>"));
        assert!(text.contains("&lt;"));
        assert!(text.contains("&gt;"));
        assert!(text.contains("&amp;"));
        assert!(text.contains("</html>"));
        // The raw unescaped sequence must not survive.
        assert!(!text.contains("a < b"));
    }

    #[test]
    fn markdown_source_renders_to_subset_html() {
        let out = to_html(b"# Title\n\nsome **bold** text", "text/markdown").expect("html export");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.starts_with("<!doctype html>"));
        assert!(text.contains("<h1>"));
        assert!(text.contains("<strong>bold</strong>"));
    }

    #[test]
    fn markdown_raw_script_is_not_executed() {
        let out = to_html(b"text <script>alert(1)</script>", "text/markdown").expect("html export");
        let text = String::from_utf8(out).expect("utf-8");
        // Default pulldown-cmark escapes raw HTML rather than passing it through.
        assert!(!text.contains("<script>"));
    }

    #[test]
    fn extract_text_strips_tags() {
        let extracted = extract_text(b"<p>hello <b>walrus</b></p>");
        assert!(extracted.contains("hello"));
        assert!(extracted.contains("walrus"));
        assert!(!extracted.contains('<'));
    }
}
