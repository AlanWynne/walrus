//! Lower Markdown to the shared `Vec<Block>` model (design 2.3) using
//! `pulldown-cmark`. This feeds the SAME `richtext` painter as `html.rs`, so
//! Markdown and HTML render through one layout path.
//!
//! Like `html.rs`, this is pure (bytes -> `Vec<Block>`). Raw inline/HTML blocks
//! embedded in the Markdown are treated as untrusted and are NOT executed; an
//! image becomes a placeholder, never a fetch.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::richtext::{Block, Inline};

/// Lower Markdown bytes to the block model. Pure: bytes in, blocks out.
pub fn lower(bytes: &[u8]) -> Vec<Block> {
    let text = String::from_utf8_lossy(bytes);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&text, options);
    Lowerer::default().run(parser)
}

/// Running state while folding pulldown-cmark events into blocks.
#[derive(Default)]
struct Lowerer {
    blocks: Vec<Block>,
    /// Inline runs for the block currently being built.
    inlines: Vec<Inline>,
    /// Stack of inline emphasis states (bold / italic / code).
    emphasis: Vec<Emphasis>,
    /// Pending link href while inside an `[text](href)` span.
    link_href: Option<String>,
    /// List nesting: whether each open list is ordered, and its gathered items.
    lists: Vec<ListFrame>,
    /// Code-block accumulator.
    code: Option<String>,
    /// Table accumulator.
    table: Option<TableFrame>,
}

#[derive(Clone, Copy)]
enum Emphasis {
    Bold,
    Italic,
}

struct ListFrame {
    ordered: bool,
    items: Vec<Vec<Block>>,
    current: Vec<Block>,
}

#[derive(Default)]
struct TableFrame {
    head: Vec<Vec<Inline>>,
    rows: Vec<Vec<Vec<Inline>>>,
    current_row: Vec<Vec<Inline>>,
    in_head: bool,
}

impl Lowerer {
    fn run(mut self, parser: Parser<'_>) -> Vec<Block> {
        for event in parser {
            self.event(event);
        }
        self.blocks
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(t.into_string()),
            Event::Code(t) => self.inlines.push(Inline::Code(t.into_string())),
            Event::Html(_) | Event::InlineHtml(_) => {
                // Untrusted embedded HTML is not executed or rendered.
            }
            Event::SoftBreak | Event::HardBreak => {
                self.inlines.push(Inline::Text(" ".to_string()));
            }
            Event::Rule => self.blocks.push(Block::Rule),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { .. } | Tag::Paragraph => self.inlines.clear(),
            Tag::Strong => self.emphasis.push(Emphasis::Bold),
            Tag::Emphasis => self.emphasis.push(Emphasis::Italic),
            Tag::CodeBlock(_) => self.code = Some(String::new()),
            Tag::List(first) => self.lists.push(ListFrame {
                ordered: first.is_some(),
                items: Vec::new(),
                current: Vec::new(),
            }),
            Tag::Item => {
                self.inlines.clear();
            }
            Tag::BlockQuote(_) => {}
            Tag::Table(_) => self.table = Some(TableFrame::default()),
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.current_row = Vec::new();
                }
            }
            Tag::TableCell => self.inlines.clear(),
            Tag::Link { dest_url, .. } => self.link_href = Some(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                self.blocks
                    .push(Block::ImagePlaceholder(dest_url.into_string()));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(level) => {
                let inlines = std::mem::take(&mut self.inlines);
                self.push_block(Block::Heading(heading_level(level), inlines));
            }
            TagEnd::Paragraph => {
                let inlines = std::mem::take(&mut self.inlines);
                if !inlines.is_empty() {
                    self.push_block(Block::Paragraph(inlines));
                }
            }
            TagEnd::Strong | TagEnd::Emphasis => {
                self.emphasis.pop();
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    self.push_block(Block::CodeBlock(code.trim_end().to_string()));
                }
            }
            TagEnd::Item => {
                if let Some(frame) = self.lists.last_mut() {
                    let inlines = std::mem::take(&mut self.inlines);
                    if !inlines.is_empty() {
                        frame.current.push(Block::Paragraph(inlines));
                    }
                    let item = std::mem::take(&mut frame.current);
                    frame.items.push(item);
                }
            }
            TagEnd::List(_) => {
                if let Some(frame) = self.lists.pop() {
                    self.push_block(Block::List {
                        ordered: frame.ordered,
                        items: frame.items,
                    });
                }
            }
            TagEnd::TableCell => {
                let inlines = std::mem::take(&mut self.inlines);
                if let Some(t) = self.table.as_mut() {
                    t.current_row.push(inlines);
                }
            }
            TagEnd::TableRow | TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.current_row);
                    if t.in_head {
                        t.head = row;
                        t.in_head = false;
                    } else {
                        t.rows.push(row);
                    }
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.blocks.push(Block::Table {
                        head: t.head,
                        rows: t.rows,
                    });
                }
            }
            TagEnd::Link => {
                // Link inline was emitted on its text; drop the pending href.
                self.link_href = None;
            }
            _ => {}
        }
    }

    fn text(&mut self, text: String) {
        if let Some(code) = self.code.as_mut() {
            code.push_str(&text);
            return;
        }
        if let Some(href) = self.link_href.take() {
            self.inlines.push(Inline::Link { text, href });
            return;
        }
        let inline = match self.emphasis.last() {
            Some(Emphasis::Bold) => Inline::Bold(text),
            Some(Emphasis::Italic) => Inline::Italic(text),
            None => Inline::Text(text),
        };
        self.inlines.push(inline);
    }

    /// Push a finished block into the current list item (if any) or the top.
    fn push_block(&mut self, block: Block) {
        if let Some(frame) = self.lists.last_mut() {
            frame.current.push(block);
        } else {
            self.blocks.push(block);
        }
    }
}

/// Map a pulldown-cmark heading level to 1..=6.
fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "# Title\n\nA **bold** and *italic* paragraph.\n\n\
        - one\n- two\n\n\
        | H1 | H2 |\n|----|----|\n| a | b |\n\n\
        [link](https://example.com)\n\n\
        ![a walrus](remote.png)\n";

    #[test]
    fn fixture_lowers_heading_bold_italic_list_table_link() {
        let blocks = lower(FIXTURE.as_bytes());
        assert!(matches!(blocks[0], Block::Heading(1, _)));
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
    fn image_becomes_a_placeholder_not_a_fetch() {
        let blocks = lower(FIXTURE.as_bytes());
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, Block::ImagePlaceholder(_))),
            "image lowers to a placeholder block"
        );
    }

    #[test]
    fn table_has_header_and_body_rows() {
        let blocks = lower(FIXTURE.as_bytes());
        let (head, rows) = blocks
            .iter()
            .find_map(|b| match b {
                Block::Table { head, rows } => Some((head, rows)),
                _ => None,
            })
            .expect("a table block");
        assert_eq!(head.len(), 2, "two header cells");
        assert_eq!(rows.len(), 1, "one body row");
        assert_eq!(rows[0].len(), 2, "two body cells");
    }
}
