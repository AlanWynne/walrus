---
inclusion: always
---

# Documentation Character Set -- MANDATORY

## Scope
- All Markdown and plain-text files (`README.md`, `FORMAT.md`, `ROADMAP.md`,
  `examples/`, and any `docs/`).
- All steering files under `.kiro/steering/`.
- All Rust source files under `crates/` (`.rs`).

Rules differ slightly between Markdown/text and Rust source -- see below.

## Markdown and Text Files

### Allowed non-ASCII
Box-drawing characters (U+2500-U+257F) are ALLOWED in Markdown. They render
correctly in VS Code, GitHub, and UTF-8 terminals, and appear only in fenced
code blocks or ASCII-art diagrams, never in prose that automated tools must
pattern-match.

All other non-ASCII characters are PROHIBITED unless listed as allowed above.

### Prohibited characters
These cause silent pattern-matching failures in automated editing tools (the
tool cannot find `oldStr` when the editor has auto-substituted the character):

| Unicode | Name | Substitute |
|---------|------|------------|
| U+2014 | em dash | `--` or `: ` |
| U+2013 | en dash | `-` |
| U+2018 | left single quote | `'` |
| U+2019 | right single quote | `'` |
| U+201C | left double quote | `"` |
| U+201D | right double quote | `"` |
| U+2026 | ellipsis | `...` |
| U+FEFF | BOM / zero-width no-break space | remove |
| U+2192 | rightwards arrow | `->` |
| U+2194 | left-right arrow | `<->` |
| U+2264 | less-than or equal | `<=` |
| U+2265 | greater-than or equal | `>=` |
| U+2260 | not equal | `!=` |

### Substitution rules
- Em dash: `--` or `: ` by context. En dash: `-`.
- Curly single/double quotes: straight `'` / `"`. Ellipsis: `...`.
- BOM bytes at file start: remove entirely.
- Math/comparison symbols: use the ASCII substitutes above.

Note: the existing `README.md`, `FORMAT.md`, and `ROADMAP.md` already follow
this convention (ASCII `->`, `<->`, `<=`, `--`). Keep it that way.

## Rust Source Files
Plain ASCII only (0x00-0x7F). Box-drawing is NOT allowed in `.rs` files.
Section separators use ASCII only:
```rust
// === Section Name ===================================================
```

## Enforcement
```bash
# Markdown/text (allows box-drawing, flags everything else)
rg "[^\x00-\x7F\u2500-\u257F]" README.md FORMAT.md ROADMAP.md .kiro/ examples/ --glob "*.md"

# Rust source (strict ASCII only)
rg "[^\x00-\x7F]" crates/ --glob "*.rs"
```
Any match (outside allowed ranges) is a violation. Fix with the ASCII
substitute.

## Why these characters
1. **Editor auto-substitution** (em dash, curly quotes, ellipsis): editors
   silently replace typed characters with typographic equivalents; automated
   `oldStr` matching then fails. This is the primary source of tool failures.
2. **Comparison / arrow symbols**: cause the same failures and render
   inconsistently in terminals without full Unicode support.
3. **Box-drawing is allowed** because it appears only in fenced code blocks and
   diagrams, never in searchable prose, and renders correctly everywhere.
