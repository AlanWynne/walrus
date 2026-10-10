//! A dependency-free parser for the fixed ISO-8601 timestamp shape the Walrus
//! writer emits, used only to order change-record timestamps for `Timestamp`
//! version addressing (FORMAT.md section 5).

/// Parse the fixed ISO-8601 UTC shape `YYYY-MM-DDThh:mm:ssZ` into epoch seconds.
///
/// This is NOT a general ISO-8601 parser. It accepts only the exact fixed-width
/// shape the writer emits (four-digit year, `-`, two-digit month, `-`,
/// two-digit day, `T`, two-digit hour, `:`, minute, `:`, second, trailing `Z`)
/// and returns `None` for anything else. The matching rule for `Timestamp`
/// addressing needs only a monotonic ordering of the timestamps the writer
/// emits, so a strict fixed-shape parser is sufficient and keeps the crate
/// dependency-free (INV-2).
///
/// The returned value is seconds since the Unix epoch (1970-01-01T00:00:00Z),
/// computed from the proleptic Gregorian calendar.
pub(crate) fn parse_ts_epoch(ts: &str) -> Option<i64> {
    let bytes = ts.as_bytes();
    // Exact length of "YYYY-MM-DDThh:mm:ssZ" is 20.
    if bytes.len() != 20 {
        return None;
    }
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes[19] != b'Z'
    {
        return None;
    }

    let year = parse_digits(&bytes[0..4])?;
    let month = parse_digits(&bytes[5..7])?;
    let day = parse_digits(&bytes[8..10])?;
    let hour = parse_digits(&bytes[11..13])?;
    let minute = parse_digits(&bytes[14..16])?;
    let second = parse_digits(&bytes[17..19])?;

    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Parse an ASCII digit run into an `i64`; `None` if any byte is not a digit.
fn parse_digits(bytes: &[u8]) -> Option<i64> {
    let mut value: i64 = 0;
    for &b in bytes {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value * 10 + i64::from(b - b'0');
    }
    Some(value)
}

/// Days since the Unix epoch for a proleptic-Gregorian civil date. Adapted from
/// the well-known days_from_civil algorithm (Howard Hinnant), kept branch-free.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    // Parenthesise the whole conditional: without the parens, `/ 400` would bind
    // only to the `else` arm (Rust precedence), leaving `era == y` for y >= 0.
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ts_epoch_parses_the_iso_shape_and_rejects_garbage() {
        // The Unix epoch itself.
        assert_eq!(parse_ts_epoch("1970-01-01T00:00:00Z"), Some(0));
        // A known instant: 2026-10-08T12:00:00Z.
        assert_eq!(parse_ts_epoch("2026-10-08T12:00:00Z"), Some(1_791_460_800));
        // Ordering is monotonic: a later wall-clock time is a larger epoch.
        let earlier = parse_ts_epoch("2026-10-08T12:00:00Z").expect("earlier parses");
        let later = parse_ts_epoch("2026-10-08T12:00:01Z").expect("later parses");
        assert!(later > earlier);

        // Garbage and wrong shapes are rejected.
        assert_eq!(parse_ts_epoch(""), None);
        assert_eq!(parse_ts_epoch("not a timestamp"), None);
        assert_eq!(parse_ts_epoch("2026-10-08 12:00:00Z"), None); // space, not T
        assert_eq!(parse_ts_epoch("2026-10-08T12:00:00"), None); // missing Z
        assert_eq!(parse_ts_epoch("2026-13-08T12:00:00Z"), None); // bad month
        assert_eq!(parse_ts_epoch("2026-10-08T24:00:00Z"), None); // bad hour
    }
}
