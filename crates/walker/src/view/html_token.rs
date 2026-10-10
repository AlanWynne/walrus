//! The HTML tokeniser for the viewer's safe HTML subset (split out of
//! `html.rs` to keep each file under the 400 non-test-line budget; this is the
//! lexical half, `html.rs` keeps the parser half that builds the block model).
//!
//! The input is UNTRUSTED: `script` / `style` / `iframe` / `head` content is
//! dropped entirely, an `img` is turned into a placeholder carrier token, and
//! an `a` href is carried as a sentinel text token for the parser. No HTML
//! parsing dependency is taken.

/// A lexical token: a tag or a text run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    /// An opening tag with its lowercased name, e.g. `<p>`.
    Open(String),
    /// A closing tag, e.g. `</p>`.
    Close(String),
    /// A self-closing or void tag, e.g. `<br/>`, `<img ...>`, `<hr>`.
    Void(String),
    /// Decoded text content.
    Text(String),
}

/// Void elements that never have a closing tag.
fn is_void(name: &str) -> bool {
    matches!(name, "br" | "hr" | "img")
}

/// Elements whose entire content (including nested markup) is dropped.
fn is_dropped(name: &str) -> bool {
    matches!(name, "script" | "style" | "iframe" | "head")
}

/// Split raw HTML into tokens, decoding entities in text and dropping the
/// contents of script/style/etc. entirely.
pub(super) fn tokenise(html: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut text = String::new();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if !text.trim().is_empty() {
                tokens.push(Token::Text(decode_entities(text.trim())));
            }
            text.clear();
            // Find the end of the tag.
            let Some(end) = html[i..].find('>').map(|e| i + e) else {
                break;
            };
            let raw = &html[i + 1..end];
            i = end + 1;
            if let Some(comment) = raw.strip_prefix("!--") {
                // Skip comments / doctype; `comment` unused beyond the guard.
                let _ = comment;
                continue;
            }
            if raw.starts_with('!') {
                continue; // doctype
            }
            if let Some(name) = raw.strip_prefix('/') {
                tokens.push(Token::Close(tag_name(name)));
                continue;
            }
            let name = tag_name(raw);
            if is_dropped(&name) {
                // Skip to the matching close tag, discarding everything.
                let close = format!("</{name}");
                if let Some(rel) = html[i..].to_ascii_lowercase().find(&close) {
                    if let Some(gt) = html[i + rel..].find('>') {
                        i = i + rel + gt + 1;
                    }
                }
                continue;
            }
            if name == "img" {
                // An image becomes a labelled placeholder; it is NEVER fetched.
                let alt = attr(raw, "alt")
                    .or_else(|| attr(raw, "src"))
                    .unwrap_or_default();
                tokens.push(Token::Void(format!("img:{alt}")));
            } else if raw.ends_with('/') || is_void(&name) {
                tokens.push(Token::Void(name));
            } else {
                tokens.push(Token::Open(name));
                // Keep the raw attributes for tags that need them (a).
                if let Some(href) = attr(raw, "href") {
                    tokens.push(Token::Text(format!("\u{0}href={href}")));
                }
            }
        } else {
            text.push(bytes[i] as char);
            i += 1;
        }
    }
    if !text.trim().is_empty() {
        tokens.push(Token::Text(decode_entities(text.trim())));
    }
    tokens
}

/// Extract the lowercased tag name from a raw tag body.
fn tag_name(raw: &str) -> String {
    raw.trim()
        .trim_end_matches('/')
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Extract a quoted attribute value from a raw tag body.
fn attr(raw: &str, key: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let pat = format!("{key}=");
    let pos = lower.find(&pat)? + pat.len();
    let rest = &raw[pos..];
    let rest = rest.trim_start();
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let end = rest[1..].find(quote)? + 1;
        Some(decode_entities(&rest[1..end]))
    } else {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest.len());
        Some(decode_entities(&rest[..end]))
    }
}

/// Decode the small set of HTML entities the subset needs.
fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
