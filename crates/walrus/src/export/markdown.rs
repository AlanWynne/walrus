//! Markdown export backend (feature `export-md`).

/// Export document bytes as Markdown.
///
/// A document that is already Markdown passes through verbatim; any other text
/// is wrapped in a fenced code block so its exact bytes survive the round trip
/// without being reinterpreted as Markdown syntax. Bytes are decoded lossily
/// (the manifest declares utf-8), so this never panics.
///
/// # Errors
/// Currently infallible, but returns [`crate::Result`] to match the export
/// backend contract (so richer rendering can report failures later).
pub fn to_markdown(doc: &[u8]) -> crate::Result<Vec<u8>> {
    let text = String::from_utf8_lossy(doc);
    let fenced = format!("```\n{text}\n```\n");
    Ok(fenced.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_plain_text_in_a_fenced_block() {
        let out = to_markdown(b"hello walrus").expect("markdown export");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(text.starts_with("```\n"));
        assert!(text.trim_end().ends_with("```"));
        assert!(text.contains("hello walrus"));
    }
}
