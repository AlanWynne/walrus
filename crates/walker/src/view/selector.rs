//! Pure renderer selection from a document's MIME type and name.
//!
//! `select_renderer` is the single source of truth for which view a document
//! gets. It is deliberately free of any egui dependency so it is unit-tested
//! directly (design section 2.1). Resolution order:
//!   1. Match the normalised MIME (lowercased, parameters after `;` stripped).
//!      If it maps to anything other than `Text`, use it.
//!   2. Otherwise (generic / unknown / a refinable `text/plain`), fall back to
//!      the lowercased extension of the document name.
//!   3. If neither resolves, `Renderer::Text`.

/// Which renderer a document is shown with.
///
/// A data-carrying enum so illegal states are unrepresentable: a `Code` view
/// always carries the language it highlights (rust-standards).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Renderer {
    /// Safe HTML subset rendered natively.
    Html,
    /// Markdown rendered natively (shares the richtext path with HTML).
    Markdown,
    /// CSV column grid (only for actual `text/csv`).
    Csv,
    /// Syntax-highlighted code or structured text.
    Code(CodeLang),
    /// Plain monospace fallback.
    Text,
}

/// The language hint passed to `egui_extras::syntax_highlighting`.
///
/// Each variant carries the short token the highlighter expects (`"rs"`,
/// `"json"`, ...). An unknown token degrades to monospace in the highlighter,
/// so no explicit fallback variant is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeLang {
    /// JSON (pretty-printed before highlighting).
    Json,
    /// TOML.
    Toml,
    /// XML.
    Xml,
    /// Java.
    Java,
    /// C.
    C,
    /// C++.
    Cpp,
    /// Python.
    Python,
    /// Rust.
    Rust,
    /// JavaScript.
    JavaScript,
    /// TypeScript.
    TypeScript,
    /// Shell.
    Shell,
}

impl CodeLang {
    /// The short language token the highlighter expects.
    pub fn token(self) -> &'static str {
        match self {
            CodeLang::Json => "json",
            CodeLang::Toml => "toml",
            CodeLang::Xml => "xml",
            CodeLang::Java => "java",
            CodeLang::C => "c",
            CodeLang::Cpp => "cpp",
            CodeLang::Python => "py",
            CodeLang::Rust => "rs",
            CodeLang::JavaScript => "js",
            CodeLang::TypeScript => "ts",
            CodeLang::Shell => "sh",
        }
    }
}

/// Normalise a MIME type: lowercase and strip any `;` parameters.
fn normalise_mime(mime: &str) -> String {
    mime.split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// The lowercased extension of a file name (without the dot), if any.
fn extension_of(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    let ext = &name[dot + 1..];
    if ext.is_empty() {
        None
    } else {
        Some(ext.to_ascii_lowercase())
    }
}

/// Map a normalised MIME to a renderer. `Text` means "unresolved / refinable".
fn renderer_for_mime(mime: &str) -> Renderer {
    match mime {
        "text/html" | "application/xhtml+xml" => Renderer::Html,
        "text/markdown" | "text/x-markdown" => Renderer::Markdown,
        "text/csv" => Renderer::Csv,
        "application/json" => Renderer::Code(CodeLang::Json),
        "application/toml" | "text/toml" => Renderer::Code(CodeLang::Toml),
        "application/xml" | "text/xml" => Renderer::Code(CodeLang::Xml),
        _ => Renderer::Text,
    }
}

/// Map a lowercased extension to a renderer, or `Text` if unrecognised.
fn renderer_for_extension(ext: &str) -> Renderer {
    match ext {
        "html" | "htm" | "xhtml" => Renderer::Html,
        "md" | "markdown" => Renderer::Markdown,
        "csv" => Renderer::Csv,
        "json" => Renderer::Code(CodeLang::Json),
        "toml" => Renderer::Code(CodeLang::Toml),
        "java" => Renderer::Code(CodeLang::Java),
        "c" | "h" => Renderer::Code(CodeLang::C),
        "cpp" | "cc" | "cxx" | "hpp" => Renderer::Code(CodeLang::Cpp),
        "py" => Renderer::Code(CodeLang::Python),
        "rs" => Renderer::Code(CodeLang::Rust),
        "js" | "mjs" => Renderer::Code(CodeLang::JavaScript),
        "ts" => Renderer::Code(CodeLang::TypeScript),
        "xml" => Renderer::Code(CodeLang::Xml),
        "sh" | "bash" => Renderer::Code(CodeLang::Shell),
        _ => Renderer::Text,
    }
}

/// Choose the renderer for a document from its MIME type and name.
///
/// See the module docs for the resolution order. A generic MIME
/// (`application/octet-stream`, empty, or a refinable `text/plain`) falls
/// through to the extension of `name`.
pub fn select_renderer(mime: &str, name: &str) -> Renderer {
    let normalised = normalise_mime(mime);
    let by_mime = renderer_for_mime(&normalised);
    if by_mime != Renderer::Text {
        return by_mime;
    }
    // Generic / unknown / refinable text/plain: refine from the extension.
    match extension_of(name) {
        Some(ext) => renderer_for_extension(&ext),
        None => Renderer::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_mime_selects_html() {
        assert_eq!(select_renderer("text/html", "page"), Renderer::Html);
    }

    #[test]
    fn xhtml_mime_selects_html() {
        assert_eq!(
            select_renderer("application/xhtml+xml", "page"),
            Renderer::Html
        );
    }

    #[test]
    fn markdown_mime_selects_markdown() {
        assert_eq!(select_renderer("text/markdown", "doc"), Renderer::Markdown);
    }

    #[test]
    fn x_markdown_mime_selects_markdown() {
        assert_eq!(
            select_renderer("text/x-markdown", "doc"),
            Renderer::Markdown
        );
    }

    #[test]
    fn csv_mime_selects_csv() {
        assert_eq!(select_renderer("text/csv", "data"), Renderer::Csv);
    }

    #[test]
    fn json_mime_selects_code_json() {
        assert_eq!(
            select_renderer("application/json", "data"),
            Renderer::Code(CodeLang::Json)
        );
    }

    #[test]
    fn toml_mime_selects_code_toml() {
        assert_eq!(
            select_renderer("application/toml", "cfg"),
            Renderer::Code(CodeLang::Toml)
        );
        assert_eq!(
            select_renderer("text/toml", "cfg"),
            Renderer::Code(CodeLang::Toml)
        );
    }

    #[test]
    fn application_xml_mime_selects_code_xml() {
        assert_eq!(
            select_renderer("application/xml", "doc"),
            Renderer::Code(CodeLang::Xml)
        );
    }

    #[test]
    fn text_xml_mime_selects_code_xml() {
        assert_eq!(
            select_renderer("text/xml", "doc"),
            Renderer::Code(CodeLang::Xml)
        );
    }

    #[test]
    fn mime_parameters_and_case_are_ignored() {
        assert_eq!(
            select_renderer("TEXT/CSV; charset=utf-8", "data"),
            Renderer::Csv
        );
    }

    #[test]
    fn plain_text_with_no_refining_extension_selects_text() {
        assert_eq!(select_renderer("text/plain", "notes.txt"), Renderer::Text);
    }

    #[test]
    fn plain_text_with_md_extension_refines_to_markdown() {
        assert_eq!(
            select_renderer("text/plain", "report.md"),
            Renderer::Markdown
        );
    }

    #[test]
    fn plain_text_with_json_extension_refines_to_code_json() {
        assert_eq!(
            select_renderer("text/plain", "data.json"),
            Renderer::Code(CodeLang::Json)
        );
    }

    #[test]
    fn generic_octet_stream_falls_back_to_extension() {
        assert_eq!(
            select_renderer("application/octet-stream", "main.rs"),
            Renderer::Code(CodeLang::Rust)
        );
    }

    #[test]
    fn empty_mime_falls_back_to_extension() {
        assert_eq!(
            select_renderer("", "script.py"),
            Renderer::Code(CodeLang::Python)
        );
    }

    #[test]
    fn html_extension_fallback_selects_html() {
        for name in ["a.html", "a.htm", "a.xhtml"] {
            assert_eq!(select_renderer("", name), Renderer::Html, "name {name}");
        }
    }

    #[test]
    fn c_family_extension_fallbacks() {
        assert_eq!(select_renderer("", "a.c"), Renderer::Code(CodeLang::C));
        assert_eq!(select_renderer("", "a.h"), Renderer::Code(CodeLang::C));
        assert_eq!(select_renderer("", "a.cpp"), Renderer::Code(CodeLang::Cpp));
        assert_eq!(select_renderer("", "a.cc"), Renderer::Code(CodeLang::Cpp));
        assert_eq!(select_renderer("", "a.cxx"), Renderer::Code(CodeLang::Cpp));
        assert_eq!(select_renderer("", "a.hpp"), Renderer::Code(CodeLang::Cpp));
    }

    #[test]
    fn script_language_extension_fallbacks() {
        assert_eq!(
            select_renderer("", "a.js"),
            Renderer::Code(CodeLang::JavaScript)
        );
        assert_eq!(
            select_renderer("", "a.mjs"),
            Renderer::Code(CodeLang::JavaScript)
        );
        assert_eq!(
            select_renderer("", "a.ts"),
            Renderer::Code(CodeLang::TypeScript)
        );
        assert_eq!(select_renderer("", "a.sh"), Renderer::Code(CodeLang::Shell));
        assert_eq!(
            select_renderer("", "a.bash"),
            Renderer::Code(CodeLang::Shell)
        );
        assert_eq!(select_renderer("", "a.xml"), Renderer::Code(CodeLang::Xml));
        assert_eq!(
            select_renderer("", "a.toml"),
            Renderer::Code(CodeLang::Toml)
        );
        assert_eq!(
            select_renderer("", "a.java"),
            Renderer::Code(CodeLang::Java)
        );
    }

    #[test]
    fn unknown_mime_and_unknown_extension_select_text() {
        assert_eq!(
            select_renderer("application/unknown", "file.xyz"),
            Renderer::Text
        );
    }

    #[test]
    fn unknown_mime_and_no_extension_select_text() {
        assert_eq!(
            select_renderer("application/octet-stream", "README"),
            Renderer::Text
        );
    }
}
