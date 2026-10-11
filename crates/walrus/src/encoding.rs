//! FR-W8 encoding transcode (feature `i18n`): decode a declared source encoding
//! into UTF-8 on the way in, and re-encode UTF-8 back to a label on the way out.
//!
//! UTF-8 is the single internal storage encoding; this module is the only place
//! a non-UTF-8 byte stream is turned into the stored UTF-8 and back. It wraps
//! `encoding_rs` and maps an unknown label or a lossy decode to the typed
//! [`crate::WalrusError::Encoding`]. It adds no new apply path.

use encoding_rs::Encoding;

use crate::{Result, WalrusError};

/// Decode `bytes` in the named source encoding to a UTF-8 [`String`], returning
/// the resolved [`Encoding`] so the caller can record its canonical label.
///
/// # Errors
/// Returns [`WalrusError::Encoding`] when `label` names no known encoding, or
/// when the decode encountered malformed input the encoding reports as errors.
pub(crate) fn decode_to_utf8(bytes: &[u8], label: &str) -> Result<(String, &'static Encoding)> {
    let encoding = Encoding::for_label(label.as_bytes()).ok_or_else(|| WalrusError::Encoding {
        label: label.to_string(),
        reason: "unknown encoding label".to_string(),
    })?;
    let (text, _, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err(WalrusError::Encoding {
            label: label.to_string(),
            reason: "input could not be decoded under this encoding".to_string(),
        });
    }
    Ok((text.into_owned(), encoding))
}

/// Encode a reconstructed UTF-8 `text` back to the named encoding.
///
/// # Errors
/// Returns [`WalrusError::Encoding`] when `label` names no known encoding, or
/// when the text cannot be represented in the target encoding (the encoder
/// reports unmappable characters).
pub(crate) fn encode_from_utf8(text: &str, label: &str) -> Result<Vec<u8>> {
    let encoding = Encoding::for_label(label.as_bytes()).ok_or_else(|| WalrusError::Encoding {
        label: label.to_string(),
        reason: "unknown encoding label".to_string(),
    })?;
    let (bytes, _, had_errors) = encoding.encode(text);
    if had_errors {
        return Err(WalrusError::Encoding {
            label: label.to_string(),
            reason: "text could not be encoded to this encoding".to_string(),
        });
    }
    Ok(bytes.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_label_is_a_typed_encoding_error() {
        let err = decode_to_utf8(b"abc", "no-such-encoding")
            .expect_err("an unknown label must be a typed error");
        assert!(matches!(err, WalrusError::Encoding { .. }));
        let err = encode_from_utf8("abc", "no-such-encoding")
            .expect_err("an unknown label must be a typed error");
        assert!(matches!(err, WalrusError::Encoding { .. }));
    }

    #[test]
    fn ascii_round_trips_through_a_legacy_label() {
        // ASCII bytes are a subset of every supported legacy encoding, so decode
        // then encode restores them exactly.
        let (text, enc) = decode_to_utf8(b"hello", "gbk").expect("decode ascii under gbk");
        assert_eq!(text, "hello");
        let back = encode_from_utf8(&text, enc.name()).expect("encode back");
        assert_eq!(back, b"hello");
    }
}
