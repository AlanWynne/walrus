//! JSON export backend (feature `export-json`).
//!
//! `serde_json` is already a non-optional dependency, so this feature pulls in
//! no new crate; the gate exists only to keep the surface opt-in.

/// Export document bytes wrapped in a pretty-printed JSON object.
///
/// The text is decoded lossily and placed under a `"text"` key. This is
/// deliberately forgiving: non-JSON input is still wrapped rather than
/// rejected, honouring the design's forgiving contract (section 7.4).
///
/// # Errors
/// Returns [`crate::WalrusError::Json`] only if serialising the wrapper object
/// fails, which does not happen for a plain string value.
pub fn to_json(doc: &[u8]) -> crate::Result<Vec<u8>> {
    let text = String::from_utf8_lossy(doc);
    let value = serde_json::json!({ "text": text });
    let bytes = serde_json::to_vec_pretty(&value)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_valid_json_wrapping_the_text() {
        let out = to_json(b"not: valid json").expect("json export");
        let parsed: serde_json::Value = serde_json::from_slice(&out).expect("valid json");
        assert_eq!(parsed["text"], "not: valid json");
    }
}
