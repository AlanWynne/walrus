//! The physical container: sniff codec, decompress, and parse per-file members.
//!
//! Layout (see `FORMAT.md` section 2):
//!   outer: zstd (default) or gzip compression
//!   inner: ustar tar with `manifest.json` plus, per tracked file,
//!          `files/<id>/base` and `files/<id>/<name>.wchg`
//!
//! Decompression is shared with [`crate::Archive::read`], which untars the
//! member set and parses each file's `.wchg` stream via [`parse_wchg`].

use std::io::Read;

use crate::record::ChangeRecord;
use crate::wchg::WchgHeader;
use crate::{Result, WalrusError};

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

/// The outer compression codec, sniffed from magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Codec {
    Zstd,
    Gzip,
}

pub(crate) fn sniff_codec(bytes: &[u8]) -> Result<Codec> {
    if bytes.len() >= 4 && bytes[0..4] == ZSTD_MAGIC {
        Ok(Codec::Zstd)
    } else if bytes.len() >= 2 && bytes[0..2] == GZIP_MAGIC {
        Ok(Codec::Gzip)
    } else {
        Err(WalrusError::UnknownCodec)
    }
}

/// Decompress the outer layer into the raw tar bytes.
pub(crate) fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    match sniff_codec(bytes)? {
        Codec::Zstd => {
            let mut out = Vec::new();
            let mut decoder = zstd::stream::read::Decoder::new(bytes)?;
            decoder.read_to_end(&mut out)?;
            Ok(out)
        }
        // gzip support is intentionally deferred: the `flate2` dependency is not
        // yet wired. Writers default to zstd, so this only affects foreign gzip
        // inputs. Tracked in FORMAT.md section 2.
        Codec::Gzip => Err(WalrusError::UnsupportedCodec { codec: "gzip" }),
    }
}

// === .wchg stream parsing (per-file layout) ============================

/// Parse a per-file `.wchg` stream into its header and change records.
///
/// Framing per FORMAT.md section 4.1: line 1 is the JSON header (parsed via
/// [`WchgHeader::parse`]); lines 2..N are one [`ChangeRecord`] each. Lines are
/// split on LF (`\n`) ONLY; a line that still ends in `\r` (a CRLF terminator)
/// is malformed and rejected. Record content is never trimmed. The first record
/// line is header-adjusted line 2, which is the line number carried by a parse
/// error. The `id` identifies the owning tracked file in any error.
///
/// # Errors
/// Returns [`WalrusError::Wchg`] for a non-UTF-8 stream (line 0), a CRLF
/// terminator, an interior blank line, or a record that is not valid JSON;
/// returns a [`WalrusError::WchgHeader`] when the header line is missing or not
/// a `wchg` object.
pub(crate) fn parse_wchg(id: &str, bytes: &[u8]) -> Result<(WchgHeader, Vec<ChangeRecord>)> {
    let text = std::str::from_utf8(bytes).map_err(|e| WalrusError::Wchg {
        id: id.to_string(),
        line: 0,
        reason: format!(".wchg is not UTF-8: {e}"),
    })?;

    // Split on LF only. A conforming .wchg ends every line with LF, so a final
    // empty segment is the terminator of the last real line, not a record.
    let mut segments: Vec<&str> = text.split('\n').collect();
    if matches!(segments.last(), Some(&"")) {
        segments.pop();
    }
    if segments.is_empty() {
        return Err(WalrusError::WchgHeader(
            "empty .wchg stream has no header line".to_string(),
        ));
    }

    let header = reject_crlf(id, 1, segments[0]).and_then(WchgHeader::parse)?;

    let mut records = Vec::with_capacity(segments.len().saturating_sub(1));
    for (index, raw) in segments.iter().enumerate().skip(1) {
        let line_number = index + 1; // header-adjusted: first record is line 2
        let line = reject_crlf(id, line_number, raw)?;
        if line.is_empty() {
            return Err(WalrusError::Wchg {
                id: id.to_string(),
                line: line_number,
                reason: "blank line is not a valid .wchg record".to_string(),
            });
        }
        let record: ChangeRecord = serde_json::from_str(line).map_err(|e| WalrusError::Wchg {
            id: id.to_string(),
            line: line_number,
            reason: e.to_string(),
        })?;
        records.push(record);
    }
    Ok((header, records))
}

/// Reject a CRLF-terminated line (a trailing `\r` after the LF split); never
/// trims. Returns the line unchanged when it carries no trailing `\r`.
fn reject_crlf<'a>(id: &str, line_number: usize, raw: &'a str) -> Result<&'a str> {
    if raw.ends_with('\r') {
        return Err(WalrusError::Wchg {
            id: id.to_string(),
            line: line_number,
            reason: "CRLF line ending is not allowed in a .wchg (LF only)".to_string(),
        });
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::Op;
    use crate::wchg::{Direction, WchgHeader};

    #[test]
    fn sniffs_zstd_magic() {
        assert_eq!(sniff_codec(&ZSTD_MAGIC).unwrap(), Codec::Zstd);
    }

    #[test]
    fn sniffs_gzip_magic() {
        assert_eq!(sniff_codec(&[0x1F, 0x8B, 0x00]).unwrap(), Codec::Gzip);
    }

    #[test]
    fn gzip_codec_is_reported_as_unsupported() {
        let err = decompress(&[0x1F, 0x8B, 0x00]).expect_err("gzip decode is not built in");
        assert!(matches!(
            &err,
            WalrusError::UnsupportedCodec { codec: "gzip" }
        ));
        assert_eq!(err.to_string(), "unsupported container codec: gzip");
    }

    #[test]
    fn rejects_unknown_magic() {
        assert!(sniff_codec(b"not a container").is_err());
    }

    // === parse_wchg (per-file .wchg stream) =============================

    fn wchg_header_line(direction: Direction, count: usize) -> String {
        let header = WchgHeader::new(direction, 0, "doc.txt", "utf-8", count);
        serde_json::to_string(&header).expect("serialise header")
    }

    #[test]
    fn wchg_stream_parses_header_then_records() {
        // AC2: line 1 is the header, lines 2..N are records.
        let mut stream = wchg_header_line(Direction::Forward, 2);
        stream.push('\n');
        stream.push_str(r#"{"op":"splice","at":0,"delete_len":0,"insert":"a"}"#);
        stream.push('\n');
        stream.push_str(r#"{"op":"splice","at":1,"delete_len":0,"insert":"b"}"#);
        stream.push('\n');
        let (header, records) =
            parse_wchg("f0", stream.as_bytes()).expect("well-formed stream parses");
        assert_eq!(header.direction, Direction::Forward);
        assert_eq!(records.len(), 2);
        assert!(records[0].is_splice());
    }

    #[test]
    fn wchg_bad_record_reports_owning_file_and_header_adjusted_line() {
        // AC2: a bad record on the first record line is reported as line 2.
        let mut stream = wchg_header_line(Direction::Forward, 1);
        stream.push('\n');
        stream.push_str("{ not json }");
        stream.push('\n');
        let err = parse_wchg("fX", stream.as_bytes()).expect_err("bad record rejected");
        match err {
            WalrusError::Wchg { id, line, .. } => {
                assert_eq!(id, "fX");
                assert_eq!(line, 2, "first record line is header-adjusted line 2");
            }
            other => panic!("expected Wchg error, got {other:?}"),
        }
    }

    #[test]
    fn wchg_crlf_terminated_line_is_rejected() {
        // AC2: a CRLF-terminated record line (trailing \r) is malformed.
        let mut stream = wchg_header_line(Direction::Forward, 1);
        stream.push('\n');
        stream.push_str(r#"{"op":"splice","at":0,"delete_len":0,"insert":"a"}"#);
        stream.push_str("\r\n");
        let err = parse_wchg("f0", stream.as_bytes()).expect_err("CRLF line rejected");
        match err {
            WalrusError::Wchg { line, .. } => assert_eq!(line, 2),
            other => panic!("expected Wchg error, got {other:?}"),
        }
    }

    #[test]
    fn wchg_never_trims_record_content() {
        // FORMAT.md 4.1: trailing spaces inside an insert string are DATA.
        let insert_with_spaces = "line   ";
        let record = ChangeRecord::splice(0, 0, insert_with_spaces);
        let mut stream = wchg_header_line(Direction::Forward, 1);
        stream.push('\n');
        stream.push_str(&serde_json::to_string(&record).expect("serialise record"));
        stream.push('\n');
        let (_header, records) = parse_wchg("f0", stream.as_bytes()).expect("parses");
        match &records[0].op {
            Op::Splice { insert, .. } => assert_eq!(insert, insert_with_spaces),
            other => panic!("expected splice, got {other:?}"),
        }
    }

    #[test]
    fn wchg_non_wchg_header_is_a_typed_header_error() {
        // AC + 4.2: a header whose format != "wchg" is a WchgHeader error.
        let mut stream =
            r#"{ "format": "notwchg", "spec_version": "0.1", "direction": "forward", "generations": 0, "name": "x", "encoding": "utf-8", "count": 0 }"#.to_string();
        stream.push('\n');
        let err = parse_wchg("f0", stream.as_bytes()).expect_err("non-wchg header rejected");
        assert!(matches!(err, WalrusError::WchgHeader(_)));
    }

    #[test]
    fn wchg_interior_blank_line_is_a_malformed_record() {
        // An empty segment between content lines is not a valid record.
        let mut stream = wchg_header_line(Direction::Forward, 1);
        stream.push('\n');
        stream.push('\n'); // interior blank line
        stream.push_str(r#"{"op":"splice","at":0,"delete_len":0,"insert":"a"}"#);
        stream.push('\n');
        let err = parse_wchg("f0", stream.as_bytes()).expect_err("blank record rejected");
        assert!(matches!(err, WalrusError::Wchg { .. }));
    }

    #[test]
    fn wchg_empty_log_parses_to_header_and_no_records() {
        // A header-only .wchg (count 0) ends with its LF terminator.
        let mut stream = wchg_header_line(Direction::Reverse, 0);
        stream.push('\n');
        let (header, records) = parse_wchg("f0", stream.as_bytes()).expect("header-only parses");
        assert_eq!(header.direction, Direction::Reverse);
        assert!(records.is_empty());
    }
}
