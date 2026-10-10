//! The per-file change-log header (`.wchg`), per FORMAT.md section 4.2.
//!
//! A `.wchg` is a UTF-8, LF-delimited stream whose first line is a JSON header
//! object and whose remaining lines are change records (see `record.rs`). The
//! header makes the log self-describing so it can be replayed standalone,
//! outside its parent archive: it states the replay `direction` and the base the
//! log expects.

use serde::{Deserialize, Serialize};

/// The direction a `.wchg` log replays, per FORMAT.md section 4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Replay on top of the ORIGINAL base toward the current version (`.wal`).
    Forward,
    /// Replay on top of the CURRENT base toward the original version (`.wbl`).
    Reverse,
}

/// The mandatory first line of a `.wchg` log (FORMAT.md section 4.2).
///
/// `format` MUST be `"wchg"`; a reader rejects any other value. Unknown header
/// fields are ignored by a reader (forward compatibility), so this struct does
/// not use `deny_unknown_fields`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WchgHeader {
    /// MUST be `"wchg"`.
    pub format: String,
    /// The `.wchg` spec version string, e.g. `"0.1"`.
    pub spec_version: String,
    /// Per-file replay direction; authoritative for how this file reconstructs.
    /// The archive variant only seeds the default direction for newly added
    /// files; it is not a per-file constraint.
    pub direction: Direction,
    /// Retention cap: `0` = unlimited (default); `N >= 1` = rolling window.
    pub generations: u32,
    /// The original filename/path, for standalone readability.
    pub name: String,
    /// Declared text encoding of the base and reconstructed versions.
    pub encoding: String,
    /// Number of change records that follow the header.
    pub count: usize,
    /// Optional diff-granularity hint recorded by the writer (e.g. `"line"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granularity_hint: Option<String>,
}

impl WchgHeader {
    /// The `.wchg` spec version this crate reads and writes.
    pub const SPEC_VERSION: &'static str = "0.1";

    /// The required `format` tag value.
    pub const FORMAT_TAG: &'static str = "wchg";

    /// Build a header for a file's log.
    pub fn new(
        direction: Direction,
        generations: u32,
        name: impl Into<String>,
        encoding: impl Into<String>,
        count: usize,
    ) -> Self {
        Self {
            format: Self::FORMAT_TAG.to_string(),
            spec_version: Self::SPEC_VERSION.to_string(),
            direction,
            generations,
            name: name.into(),
            encoding: encoding.into(),
            count,
            granularity_hint: None,
        }
    }

    /// Parse a header from the first line of a `.wchg` stream.
    ///
    /// # Errors
    /// Returns [`crate::WalrusError::WchgHeader`] if the line is not a JSON
    /// object or its `format` tag is not `"wchg"`, and
    /// [`crate::WalrusError::Json`] on malformed JSON.
    pub fn parse(line: &str) -> crate::Result<Self> {
        let header: WchgHeader = serde_json::from_str(line).map_err(|e| {
            crate::WalrusError::WchgHeader(format!("header line is not a valid wchg object: {e}"))
        })?;
        if header.format != Self::FORMAT_TAG {
            return Err(crate::WalrusError::WchgHeader(format!(
                "unexpected .wchg format tag '{}' (expected '{}')",
                header.format,
                Self::FORMAT_TAG
            )));
        }
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_round_trips_through_json() {
        let header = WchgHeader::new(Direction::Forward, 0, "src/document.txt", "utf-8", 42);
        let line = serde_json::to_string(&header).expect("serialise header");
        let parsed = WchgHeader::parse(&line).expect("parse header");
        assert_eq!(parsed, header);
    }

    #[test]
    fn parse_accepts_a_spec_example_header() {
        // The exact example from FORMAT.md section 4.2.
        let line = r#"{ "format": "wchg", "spec_version": "0.1", "direction": "forward", "generations": 0, "name": "src/document.txt", "encoding": "utf-8", "count": 42 }"#;
        let header = WchgHeader::parse(line).expect("parse spec example");
        assert_eq!(header.direction, Direction::Forward);
        assert_eq!(header.generations, 0);
        assert_eq!(header.count, 42);
    }

    #[test]
    fn parse_rejects_a_non_wchg_format_tag() {
        let line = r#"{ "format": "notwchg", "spec_version": "0.1", "direction": "reverse", "generations": 0, "name": "x", "encoding": "utf-8", "count": 0 }"#;
        let err = WchgHeader::parse(line).expect_err("non-wchg tag must be rejected");
        assert!(matches!(err, crate::WalrusError::WchgHeader(_)));
    }

    #[test]
    fn parse_rejects_a_record_line_mistaken_for_a_header() {
        // A splice record on the first line is NOT a header; it lacks `format`.
        let line = r#"{ "op": "splice", "at": 0, "delete_len": 0, "insert": "x" }"#;
        let err = WchgHeader::parse(line).expect_err("a record line is not a header");
        assert!(matches!(err, crate::WalrusError::WchgHeader(_)));
    }

    #[test]
    fn parse_ignores_unknown_header_fields() {
        let line = r#"{ "format": "wchg", "spec_version": "0.1", "direction": "reverse", "generations": 3, "name": "a", "encoding": "utf-8", "count": 5, "future_field": true }"#;
        let header = WchgHeader::parse(line).expect("unknown fields ignored");
        assert_eq!(header.direction, Direction::Reverse);
        assert_eq!(header.generations, 3);
    }

    #[test]
    fn direction_serialises_lowercase() {
        assert_eq!(
            serde_json::to_string(&Direction::Forward).expect("ser"),
            "\"forward\""
        );
        assert_eq!(
            serde_json::to_string(&Direction::Reverse).expect("ser"),
            "\"reverse\""
        );
    }
}
