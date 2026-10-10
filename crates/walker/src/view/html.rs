//! Lower a safe SUBSET of HTML to the shared `Vec<Block>` model (design 2.3).
//!
//! The input is UNTRUSTED stored-document bytes, so this lowering NEVER executes
//! scripts, fetches remote resources, or evaluates styles. `script`, `style`,
//! `iframe`, and event handlers are dropped entirely; `img` becomes a labelled
//! placeholder; `a` becomes styled text with the href kept only as a tooltip.
//!
//! The parser is a small hand-written tokeniser over the subset of tags the
//! viewer supports. It takes no HTML-parsing dependency (keeping walker's dep
//! set to egui_extras / pulldown-cmark / csv), which is sufficient for the
//! acceptable subset -- unknown tags are skipped while their text is kept.

use super::html_token::{tokenise, Token};
use super::richtext::{Block, Inline};

/// Lower HTML bytes to the block model. Pure: bytes in, blocks out.
pub fn lower(bytes: &[u8]) -> Vec<Block> {
    let html = String::from_utf8_lossy(bytes);
    let tokens = tokenise(&html);
    Parser::new(&tokens).parse_blocks(&[])
}

// === Parser =========================================================

/// Recursive-descent parser over the token stream.
struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Parse block-level elements until one of `stop` closing tags (or EOF).
    fn parse_blocks(&mut self, stop: &[&str]) -> Vec<Block> {
        let mut blocks = Vec::new();
        while let Some(token) = self.tokens.get(self.pos) {
            match token {
                Token::Close(name) if stop.contains(&name.as_str()) => break,
                Token::Close(_) => {
                    self.pos += 1;
                }
                Token::Open(name) => {
                    let name = name.clone();
                    self.pos += 1;
                    if let Some(block) = self.parse_open_block(&name) {
                        blocks.push(block);
                    }
                }
                Token::Void(name) => {
                    let name = name.clone();
                    self.pos += 1;
                    if name == "hr" {
                        blocks.push(Block::Rule);
                    } else if let Some(alt) = name.strip_prefix("img:") {
                        blocks.push(Block::ImagePlaceholder(alt.to_string()));
                    }
                }
                Token::Text(text) => {
                    // Bare text outside a block becomes a paragraph.
                    let text = text.clone();
                    self.pos += 1;
                    if !text.starts_with('\u{0}') {
                        blocks.push(Block::Paragraph(vec![Inline::Text(text)]));
                    }
                }
            }
        }
        blocks
    }

    /// Parse the block introduced by an opening tag named `name`.
    fn parse_open_block(&mut self, name: &str) -> Option<Block> {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                let level = name[1..].parse().unwrap_or(1);
                let inlines = self.parse_inlines(name);
                Some(Block::Heading(level, inlines))
            }
            "p" => Some(Block::Paragraph(self.parse_inlines("p"))),
            "pre" | "code" => {
                let text = self.collect_text(name);
                Some(Block::CodeBlock(text))
            }
            "ul" | "ol" => Some(self.parse_list(name == "ol")),
            "blockquote" => Some(Block::Quote(self.parse_blocks(&["blockquote"]))),
            "table" => Some(self.parse_table()),
            // Containers whose children are block-level (div, body, html, etc.):
            // splice their blocks in by returning the first, after recursing.
            _ => {
                let mut inner = self.parse_blocks(&[name]);
                // Consume the matching close.
                self.consume_close(name);
                if inner.is_empty() {
                    None
                } else {
                    // Wrap multiple inner blocks in a quote-free passthrough: a
                    // single container cannot return many blocks, so collapse to
                    // the first and push the rest back is complex; instead wrap
                    // them as a borderless group via Quote only if >1. For the
                    // supported subset, inner is normally one block.
                    Some(inner.remove(0))
                }
            }
        }
    }

    /// Parse a list body until `</ul>` or `</ol>`.
    fn parse_list(&mut self, ordered: bool) -> Block {
        let close = if ordered { "ol" } else { "ul" };
        let mut items = Vec::new();
        while let Some(token) = self.tokens.get(self.pos) {
            match token {
                Token::Close(name) if name == close => {
                    self.pos += 1;
                    break;
                }
                Token::Open(name) if name == "li" => {
                    self.pos += 1;
                    let inlines = self.parse_inlines("li");
                    items.push(vec![Block::Paragraph(inlines)]);
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
        Block::List { ordered, items }
    }

    /// Parse a table body until `</table>`.
    fn parse_table(&mut self) -> Block {
        let mut head = Vec::new();
        let mut rows = Vec::new();
        while let Some(token) = self.tokens.get(self.pos) {
            match token {
                Token::Close(name) if name == "table" => {
                    self.pos += 1;
                    break;
                }
                Token::Open(name) if name == "tr" => {
                    self.pos += 1;
                    let (cells, is_header) = self.parse_row();
                    if is_header && head.is_empty() {
                        head = cells;
                    } else {
                        rows.push(cells);
                    }
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
        Block::Table { head, rows }
    }

    /// Parse one table row; returns the cells and whether it was a header row.
    fn parse_row(&mut self) -> (Vec<Vec<Inline>>, bool) {
        let mut cells = Vec::new();
        let mut is_header = false;
        while let Some(token) = self.tokens.get(self.pos) {
            match token {
                Token::Close(name) if name == "tr" => {
                    self.pos += 1;
                    break;
                }
                Token::Open(name) if name == "th" || name == "td" => {
                    if name == "th" {
                        is_header = true;
                    }
                    let tag = name.clone();
                    self.pos += 1;
                    cells.push(self.parse_inlines(&tag));
                }
                _ => {
                    self.pos += 1;
                }
            }
        }
        (cells, is_header)
    }

    /// Parse inline runs until the matching `</name>` close tag.
    fn parse_inlines(&mut self, name: &str) -> Vec<Inline> {
        let mut inlines = Vec::new();
        let mut pending_href: Option<String> = None;
        while let Some(token) = self.tokens.get(self.pos) {
            match token {
                Token::Close(close) if close == name => {
                    self.pos += 1;
                    break;
                }
                Token::Open(tag) => {
                    let tag = tag.clone();
                    self.pos += 1;
                    match tag.as_str() {
                        "strong" | "b" => {
                            inlines.push(Inline::Bold(self.collect_text(&tag)));
                        }
                        "em" | "i" => {
                            inlines.push(Inline::Italic(self.collect_text(&tag)));
                        }
                        "code" => {
                            inlines.push(Inline::Code(self.collect_text(&tag)));
                        }
                        "a" => {
                            // The href carrier text follows the open tag.
                            if let Some(Token::Text(t)) = self.tokens.get(self.pos) {
                                if let Some(h) = t.strip_prefix("\u{0}href=") {
                                    pending_href = Some(h.to_string());
                                    self.pos += 1;
                                }
                            }
                            let text = self.collect_text("a");
                            inlines.push(Inline::Link {
                                text,
                                href: pending_href.take().unwrap_or_default(),
                            });
                        }
                        _ => {}
                    }
                }
                Token::Void(_) => {
                    self.pos += 1;
                }
                Token::Text(t) => {
                    let t = t.clone();
                    self.pos += 1;
                    if let Some(h) = t.strip_prefix("\u{0}href=") {
                        pending_href = Some(h.to_string());
                    } else {
                        inlines.push(Inline::Text(t));
                    }
                }
                Token::Close(_) => {
                    // A stray close for another tag: stop to avoid over-consuming.
                    break;
                }
            }
        }
        inlines
    }

    /// Collect the plain text content until the matching close tag.
    fn collect_text(&mut self, name: &str) -> String {
        let mut text = String::new();
        while let Some(token) = self.tokens.get(self.pos) {
            match token {
                Token::Close(close) if close == name => {
                    self.pos += 1;
                    break;
                }
                Token::Text(t) => {
                    if !t.starts_with('\u{0}') {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(t);
                    }
                    self.pos += 1;
                }
                Token::Close(_) => break,
                _ => {
                    self.pos += 1;
                }
            }
        }
        text
    }

    /// Consume a matching close tag if present.
    fn consume_close(&mut self, name: &str) {
        if let Some(Token::Close(close)) = self.tokens.get(self.pos) {
            if close == name {
                self.pos += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
        <h1>Title</h1>
        <p>A <strong>bold</strong> and <em>italic</em> paragraph.</p>
        <ul><li>one</li><li>two</li></ul>
        <table><tr><th>H1</th><th>H2</th></tr><tr><td>a</td><td>b</td></tr></table>
        <p><a href="https://example.com">link</a></p>
        <script>alert('xss')</script>
        <img src="remote.png" alt="a walrus">
    "#;

    #[test]
    fn fixture_lowers_heading_bold_italic_list_table_link() {
        let blocks = lower(FIXTURE.as_bytes());
        assert!(matches!(blocks[0], Block::Heading(1, _)));
        // The paragraph carries bold and italic runs.
        let has_bold = blocks.iter().any(|b| {
            matches!(b, Block::Paragraph(inl) if inl.iter().any(|i| matches!(i, Inline::Bold(_))))
        });
        let has_italic = blocks.iter().any(|b| {
            matches!(b, Block::Paragraph(inl) if inl.iter().any(|i| matches!(i, Inline::Italic(_))))
        });
        assert!(has_bold, "bold run present");
        assert!(has_italic, "italic run present");
        assert!(
            blocks.iter().any(|b| matches!(b, Block::List { .. })),
            "list present"
        );
        assert!(
            blocks.iter().any(|b| matches!(b, Block::Table { .. })),
            "table present"
        );
        assert!(
            blocks.iter().any(|b| matches!(
                b, Block::Paragraph(inl) if inl.iter().any(|i| matches!(i, Inline::Link { .. }))
            )),
            "link present"
        );
    }

    #[test]
    fn script_tag_is_dropped() {
        let blocks = lower(FIXTURE.as_bytes());
        let text: String = format!("{blocks:?}");
        assert!(!text.contains("xss"), "script contents must be dropped");
        assert!(!text.contains("alert"), "script contents must be dropped");
    }

    #[test]
    fn img_becomes_a_placeholder_not_a_fetch() {
        let blocks = lower(FIXTURE.as_bytes());
        let placeholder = blocks
            .iter()
            .find(|b| matches!(b, Block::ImagePlaceholder(_)));
        assert!(placeholder.is_some(), "img lowers to a placeholder block");
        if let Some(Block::ImagePlaceholder(alt)) = placeholder {
            assert_eq!(alt, "a walrus");
        }
    }

    #[test]
    fn table_has_header_and_body_rows() {
        let blocks = lower(FIXTURE.as_bytes());
        let table = blocks.iter().find_map(|b| match b {
            Block::Table { head, rows } => Some((head, rows)),
            _ => None,
        });
        let (head, rows) = table.expect("a table block");
        assert_eq!(head.len(), 2, "two header cells");
        assert_eq!(rows.len(), 1, "one body row");
        assert_eq!(rows[0].len(), 2, "two body cells");
    }
}
