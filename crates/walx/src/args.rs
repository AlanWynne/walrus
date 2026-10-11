//! Hand-rolled argument parsing for the two walx subcommands and the shared
//! version selector (CLI-DESIGN section 4, 5.2).
//!
//! The surface is tiny and fixed, so a hand parser stays leaner than clap and
//! keeps `cargo tree -p walx` auditable (INV-2). This module owns: the
//! mutually-exclusive version selector, the fixed-shape UTC `--at` parser, and
//! the typed [`CliError`] carrying a return code for `main` to translate.

use std::fmt;

use walrus_fmt::Version;

/// A usage/safety error carrying the process return code it maps to
/// (CLI-DESIGN section 6). Only the codes walx raises at the arg layer appear
/// here; richer per-file outcomes are decided in the command modules.
#[derive(Debug, PartialEq, Eq)]
pub struct CliError {
    /// Human-readable reason, printed to stderr by `main`.
    pub message: String,
    /// The process return code (2 usage/safety for everything parsed here).
    pub code: u8,
}

impl CliError {
    /// Build a usage/safety error (RC 2).
    pub fn usage(message: impl Into<String>) -> Self {
        CliError {
            message: message.into(),
            code: 2,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

/// The parsed version selector, before mapping to the library [`Version`].
/// At most one may be supplied; the default (nothing given) is `Current`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// Default / `--version current` -> index 0.
    Current,
    /// `--version original` -> each file's own `count`.
    Original,
    /// `--index <n>` -> a numeric index (triggers the beyond-last fallback).
    Index(usize),
    /// `--version <+/-n>` -> a signed offset relative to current.
    Relative(i64),
    /// `--at <T>` -> a UTC snapshot, stored as epoch seconds.
    At(i64),
}

impl Selector {
    /// Map this selector to the library [`Version`] it resolves through.
    pub fn to_version(&self) -> Version {
        match self {
            Selector::Current => Version::Current,
            Selector::Original => Version::Original,
            Selector::Index(n) => Version::Index(*n),
            Selector::Relative(n) => Version::Relative(*n),
            Selector::At(epoch) => Version::Timestamp(*epoch),
        }
    }

    /// True when this selector is a NUMERIC `--index`, the only spelling that
    /// triggers the beyond-last-index clamp-and-warn fallback (AC2).
    pub fn is_numeric_index(&self) -> bool {
        matches!(self, Selector::Index(_))
    }
}

/// Resolve the effective selector: the one supplied, or [`Selector::Current`]
/// (index 0) when none was given (CLI-DESIGN 5.2.1 default).
pub fn effective_selector(slot: Option<Selector>) -> Selector {
    slot.unwrap_or(Selector::Current)
}

/// Set `slot` to `value`, or raise RC 2 if a selector was already supplied
/// (CLI-DESIGN 5.2: at most one selector).
pub fn set_selector(slot: &mut Option<Selector>, value: Selector) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::usage("at most one version selector may be given"));
    }
    *slot = Some(value);
    Ok(())
}

/// Parse a `--version <v>` value into a [`Selector`]. Accepts `current`,
/// `original`, or a signed/bare integer (mapped to `Relative`); anything else
/// is RC 2.
pub fn parse_version_value(value: &str) -> Result<Selector, CliError> {
    match value {
        "current" => Ok(Selector::Current),
        "original" => Ok(Selector::Original),
        other => {
            let n: i64 = other
                .parse()
                .map_err(|_| CliError::usage(format!("invalid --version value '{other}'")))?;
            Ok(Selector::Relative(n))
        }
    }
}

/// Parse an `--index <n>` value: a non-negative integer only (relative lives
/// under `--version`). A negative or non-numeric value is RC 2.
pub fn parse_index_value(value: &str) -> Result<Selector, CliError> {
    let n: usize = value
        .parse()
        .map_err(|_| CliError::usage(format!("invalid --index value '{value}'")))?;
    Ok(Selector::Index(n))
}

/// Parse and validate a fixed-shape UTC instant `YYYY-MM-DDThh:mm:ssZ` into
/// epoch seconds. The trailing `Z` is required and no other shape is accepted
/// (CLI-DESIGN 5.2.1). A malformed value is RC 2.
pub fn parse_at_utc(text: &str) -> Result<i64, CliError> {
    let bytes = text.as_bytes();
    let bad = || CliError::usage(format!("--at value '{text}' must be YYYY-MM-DDThh:mm:ssZ"));
    if bytes.len() != 20 {
        return Err(bad());
    }
    // Fixed separators at their exact positions.
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return Err(bad());
    }
    let field = |start: usize, len: usize| -> Result<i64, CliError> {
        let slice = &text[start..start + len];
        if !slice.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        slice.parse::<i64>().map_err(|_| bad())
    };
    let year = field(0, 4)?;
    let month = field(5, 2)?;
    let day = field(8, 2)?;
    let hour = field(11, 2)?;
    let minute = field(14, 2)?;
    let second = field(17, 2)?;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(bad());
    }
    Ok(civil_to_epoch(year, month, day, hour, minute, second))
}

/// Convert a validated civil UTC date-time to epoch seconds using the
/// days-from-civil algorithm (self-contained; walx cannot reach the library's
/// `pub(crate)` parser). Mirrors the library's conversion.
fn civil_to_epoch(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    // Howard Hinnant's days_from_civil: days since 1970-01-01 (UTC).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146097 + doe - 719468;
    days * 86_400 + hour * 3_600 + minute * 60 + second
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_selector_defaults_to_current() {
        // No selector supplied resolves to Current (index 0); a supplied one is
        // passed through unchanged.
        assert_eq!(effective_selector(None), Selector::Current);
        assert_eq!(effective_selector(None).to_version(), Version::Current);
        assert_eq!(
            effective_selector(Some(Selector::Original)),
            Selector::Original
        );
    }

    #[test]
    fn version_original_maps_to_version_original() {
        assert_eq!(
            parse_version_value("original").expect("original"),
            Selector::Original
        );
        assert_eq!(Selector::Original.to_version(), Version::Original);
    }

    #[test]
    fn index_n_maps_to_version_index() {
        assert_eq!(parse_index_value("3").expect("index"), Selector::Index(3));
        assert_eq!(Selector::Index(3).to_version(), Version::Index(3));
        assert!(Selector::Index(3).is_numeric_index());
    }

    #[test]
    fn version_signed_maps_to_relative() {
        assert_eq!(
            parse_version_value("-1").expect("neg"),
            Selector::Relative(-1)
        );
        assert_eq!(
            parse_version_value("+2").expect("pos"),
            Selector::Relative(2)
        );
        assert_eq!(
            parse_version_value("5").expect("bare"),
            Selector::Relative(5)
        );
        assert_eq!(Selector::Relative(-1).to_version(), Version::Relative(-1));
    }

    #[test]
    fn at_accepts_fixed_utc_shape_and_rejects_other_shapes() {
        // Accept the canonical shape and map to the correct epoch.
        let epoch = parse_at_utc("2026-10-08T12:00:00Z").expect("valid shape");
        assert_eq!(epoch, 1_791_460_800);
        // The Unix epoch itself round-trips.
        assert_eq!(parse_at_utc("1970-01-01T00:00:00Z").expect("epoch zero"), 0);

        // Reject: missing Z, space instead of T, bad month, bad hour, wrong length.
        assert!(parse_at_utc("2026-10-08T12:00:00").is_err());
        assert!(parse_at_utc("2026-10-08 12:00:00Z").is_err());
        assert!(parse_at_utc("2026-13-08T12:00:00Z").is_err());
        assert!(parse_at_utc("2026-10-08T24:00:00Z").is_err());
        assert!(parse_at_utc("2026-10-08T12:00Z").is_err());
    }

    #[test]
    fn two_selectors_is_usage_error_rc2() {
        let mut slot = None;
        set_selector(&mut slot, Selector::Current).expect("first ok");
        let err = set_selector(&mut slot, Selector::Original).expect_err("second rejected");
        assert_eq!(err.code, 2);
    }

    #[test]
    fn invalid_version_value_is_rc2() {
        let err = parse_version_value("sideways").expect_err("not a word or int");
        assert_eq!(err.code, 2);
    }
}
