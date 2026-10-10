//! Shared rich-text document model and egui painter for the HTML and Markdown
//! renderers (design section 2.3). This file is the SPLIT SEAM: `html.rs` and
//! `markdown.rs` both lower their input to `Vec<Block>` here, and the single
//! painter below renders it, so the two renderers share one layout path.
//!
//! The model is intentionally small and GUI-free so the lowering functions in
//! `html.rs` / `markdown.rs` are pure (bytes -> `Vec<Block>`) and unit-testable
//! without a running egui context.

use eframe::egui;

/// An inline run inside a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    /// Plain text.
    Text(String),
    /// Bold text.
    Bold(String),
    /// Italic text.
    Italic(String),
    /// Inline code span (monospace).
    Code(String),
    /// A link shown as styled text; the href is a tooltip only (never
    /// navigated -- untrusted content).
    Link {
        /// The visible link text.
        text: String,
        /// The href, shown in a tooltip.
        href: String,
    },
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A heading of level 1..=6.
    Heading(u8, Vec<Inline>),
    /// A paragraph.
    Paragraph(Vec<Inline>),
    /// A fenced / preformatted code block (rendered monospace).
    CodeBlock(String),
    /// An ordered or unordered list; each item is its own block sequence.
    List {
        /// Whether the list is ordered (numbered).
        ordered: bool,
        /// The list items, each a sequence of blocks.
        items: Vec<Vec<Block>>,
    },
    /// A block quote.
    Quote(Vec<Block>),
    /// A table with a header row and body rows (cells are inline runs).
    Table {
        /// Header cells.
        head: Vec<Vec<Inline>>,
        /// Body rows of cells.
        rows: Vec<Vec<Vec<Inline>>>,
    },
    /// A horizontal rule.
    Rule,
    /// A placeholder for an image (never fetched -- untrusted content). Carries
    /// the alt text or source for display.
    ImagePlaceholder(String),
}

/// Paint a lowered document into the given `Ui`.
pub fn paint(ui: &mut egui::Ui, blocks: &[Block]) {
    for block in blocks {
        paint_block(ui, block);
    }
}

/// Paint one block.
fn paint_block(ui: &mut egui::Ui, block: &Block) {
    match block {
        Block::Heading(level, inlines) => {
            let size = heading_size(*level);
            ui.horizontal_wrapped(|ui| {
                for inline in inlines {
                    paint_inline(ui, inline, Some(size));
                }
            });
        }
        Block::Paragraph(inlines) => {
            ui.horizontal_wrapped(|ui| {
                for inline in inlines {
                    paint_inline(ui, inline, None);
                }
            });
        }
        Block::CodeBlock(code) => {
            ui.add(
                egui::Label::new(egui::RichText::new(code).monospace())
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        }
        Block::List { ordered, items } => {
            for (i, item) in items.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    let marker = if *ordered {
                        format!("{}.", i + 1)
                    } else {
                        "-".to_string()
                    };
                    ui.label(egui::RichText::new(marker).monospace());
                    for block in item {
                        paint_block(ui, block);
                    }
                });
            }
        }
        Block::Quote(blocks) => {
            ui.group(|ui| {
                for block in blocks {
                    paint_block(ui, block);
                }
            });
        }
        Block::Table { head, rows } => paint_table(ui, head, rows),
        Block::Rule => {
            ui.separator();
        }
        Block::ImagePlaceholder(alt) => {
            ui.label(
                egui::RichText::new(format!("[image: {alt}]"))
                    .italics()
                    .weak(),
            );
        }
    }
}

/// Paint a table with a header row and body rows.
fn paint_table(ui: &mut egui::Ui, head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>]) {
    use egui_extras::{Column, TableBuilder};

    let columns = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }
    let mut builder = TableBuilder::new(ui).striped(true);
    for _ in 0..columns {
        builder = builder.column(Column::auto().resizable(true));
    }
    builder
        .header(20.0, |mut header| {
            for cell in head {
                header.col(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        for inline in cell {
                            paint_inline(ui, inline, None);
                        }
                    });
                });
            }
            // Pad missing header cells so the column count matches.
            for _ in head.len()..columns {
                header.col(|ui| {
                    ui.label("");
                });
            }
        })
        .body(|mut body| {
            for row in rows {
                body.row(18.0, |mut row_ui| {
                    for cell in row {
                        row_ui.col(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                for inline in cell {
                                    paint_inline(ui, inline, None);
                                }
                            });
                        });
                    }
                    for _ in row.len()..columns {
                        row_ui.col(|ui| {
                            ui.label("");
                        });
                    }
                });
            }
        });
}

/// Paint one inline run, optionally forcing a heading font size.
fn paint_inline(ui: &mut egui::Ui, inline: &Inline, heading: Option<f32>) {
    let apply_size = |text: egui::RichText| match heading {
        Some(size) => text.size(size).strong(),
        None => text,
    };
    match inline {
        Inline::Text(s) => {
            ui.label(apply_size(egui::RichText::new(s)));
        }
        Inline::Bold(s) => {
            ui.label(apply_size(egui::RichText::new(s).strong()));
        }
        Inline::Italic(s) => {
            ui.label(apply_size(egui::RichText::new(s).italics()));
        }
        Inline::Code(s) => {
            ui.label(apply_size(egui::RichText::new(s).monospace()));
        }
        Inline::Link { text, href } => {
            ui.label(apply_size(egui::RichText::new(text).underline()))
                .on_hover_text(href);
        }
    }
}

/// Font size for a heading level (1 is largest).
fn heading_size(level: u8) -> f32 {
    match level {
        1 => 28.0,
        2 => 24.0,
        3 => 20.0,
        4 => 18.0,
        5 => 16.0,
        _ => 14.0,
    }
}
