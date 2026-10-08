//! The physical container: sniff codec, decompress, untar, parse members.
//!
//! Layout (see `FORMAT.md` section 2):
//!   outer: zstd (default) or gzip compression
//!   inner: ustar tar with fixed members `manifest.json`, `base`, `log.jsonl`

use std::io::Read;

use crate::manifest::Manifest;
use crate::record::ChangeRecord;
use crate::{Result, WalrusError};

const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];
const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

/// The outer compression codec, sniffed from magic bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Codec {
    Zstd,
    Gzip,
}

fn sniff_codec(bytes: &[u8]) -> Result<Codec> {
    if bytes.len() >= 4 && bytes[0..4] == ZSTD_MAGIC {
        Ok(Codec::Zstd)
    } else if bytes.len() >= 2 && bytes[0..2] == GZIP_MAGIC {
        Ok(Codec::Gzip)
    } else {
        Err(WalrusError::UnknownCodec)
    }
}

/// Decompress the outer layer into the raw tar bytes.
fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
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
        Codec::Gzip => Err(WalrusError::Manifest(
            "gzip containers are not yet supported by this build (use zstd)".to_string(),
        )),
    }
}

/// Read a whole compressed container into (manifest, base bytes, log).
pub(crate) fn read_container(bytes: &[u8]) -> Result<(Manifest, Vec<u8>, Vec<ChangeRecord>)> {
    let tar_bytes = decompress(bytes)?;
    let mut archive = tar::Archive::new(&tar_bytes[..]);

    let mut manifest_json: Option<Vec<u8>> = None;
    let mut base: Option<Vec<u8>> = None;
    let mut log_jsonl: Option<Vec<u8>> = None;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        match path.as_str() {
            "manifest.json" => manifest_json = Some(buf),
            "base" => base = Some(buf),
            "log.jsonl" => log_jsonl = Some(buf),
            // meta/* and unknown members are ignored.
            _ => {}
        }
    }

    let manifest_json = manifest_json.ok_or(WalrusError::MissingMember("manifest.json"))?;
    let base = base.ok_or(WalrusError::MissingMember("base"))?;
    let log_jsonl = log_jsonl.ok_or(WalrusError::MissingMember("log.jsonl"))?;

    let manifest: Manifest = serde_json::from_slice(&manifest_json)?;
    manifest.validate()?;

    let log = parse_log(&log_jsonl)?;
    Ok((manifest, base, log))
}

/// Parse `log.jsonl`: one JSON [`ChangeRecord`] per non-empty line.
fn parse_log(bytes: &[u8]) -> Result<Vec<ChangeRecord>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|e| WalrusError::Manifest(format!("log.jsonl is not UTF-8: {e}")))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let record: ChangeRecord =
            serde_json::from_str(trimmed).map_err(|e| WalrusError::Record {
                line: i + 1,
                reason: e.to_string(),
            })?;
        out.push(record);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_zstd_magic() {
        assert_eq!(sniff_codec(&ZSTD_MAGIC).unwrap(), Codec::Zstd);
    }

    #[test]
    fn sniffs_gzip_magic() {
        assert_eq!(sniff_codec(&[0x1F, 0x8B, 0x00]).unwrap(), Codec::Gzip);
    }

    #[test]
    fn rejects_unknown_magic() {
        assert!(sniff_codec(b"not a container").is_err());
    }

    #[test]
    fn parses_jsonl_log_skipping_blanks() {
        let log = b"{\"op\":\"splice\",\"at\":0,\"delete_len\":0,\"insert\":\"x\"}\n\n{\"op\":\"splice\",\"at\":1,\"delete_len\":0,\"insert\":\"y\"}\n";
        let records = parse_log(log).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].is_splice());
    }

    #[test]
    fn reports_line_number_on_bad_record() {
        let log = b"{\"op\":\"splice\",\"at\":0,\"delete_len\":0,\"insert\":\"x\"}\n{ not json }\n";
        let err = parse_log(log).unwrap_err();
        match err {
            WalrusError::Record { line, .. } => assert_eq!(line, 2),
            other => panic!("expected Record error, got {other:?}"),
        }
    }
}
