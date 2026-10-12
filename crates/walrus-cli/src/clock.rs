//! The CLI's wall-clock reader: format "now" as the fixed UTC shape
//! `YYYY-MM-DDThh:mm:ssZ` the record stamping seam requires (FR-W11 AC8).
//!
//! The `walrus-fmt` library is deliberately clock-free and receives the string;
//! reading the clock is the CLI's job (CLI-DESIGN 6a). This uses only `std`
//! (`SystemTime`) and a dependency-free civil-date conversion, so it widens no
//! auditable dependency (INV-2). Only the live `add`/`update` path calls this;
//! tests pass fixed timestamps and never read the clock.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current UTC instant formatted as `YYYY-MM-DDThh:mm:ssZ` (seconds
/// resolution). A clock before the Unix epoch is clamped to the epoch.
pub fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_epoch_utc(secs)
}

/// The source file's last-modified instant formatted as `YYYY-MM-DDThh:mm:ssZ`.
/// Falls back to [`now_utc`] when the mtime is unavailable (platform/FS quirk)
/// or pre-epoch. Reading the FILESYSTEM mtime is CLI-only; the `walrus-fmt`
/// library stays filesystem-free and receives only the formatted string.
pub fn file_mtime_utc(path: &std::path::Path) -> String {
    match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(mtime) => match mtime.duration_since(UNIX_EPOCH) {
            Ok(d) => format_epoch_utc(d.as_secs() as i64),
            Err(_) => now_utc(), // pre-epoch mtime: fall back
        },
        Err(_) => now_utc(), // mtime unreadable: fall back
    }
}

/// Format epoch seconds as the fixed UTC shape `YYYY-MM-DDThh:mm:ssZ`.
fn format_epoch_utc(epoch: i64) -> String {
    let days = epoch.div_euclid(86_400);
    let secs_of_day = epoch.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert days-since-epoch to a civil `(year, month, day)` via Howard
/// Hinnant's `civil_from_days` (the inverse of the `days_from_civil` the
/// library parser uses).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_epoch_utc_matches_the_fixed_shape_at_known_instants() {
        assert_eq!(format_epoch_utc(0), "1970-01-01T00:00:00Z");
        // 2026-10-08T12:00:00Z == 1_791_460_800 (the library parser's example).
        assert_eq!(format_epoch_utc(1_791_460_800), "2026-10-08T12:00:00Z");
        // 2026-10-11T16:18:44Z (the user's gate-run instant).
        assert_eq!(format_epoch_utc(1_791_735_524), "2026-10-11T16:18:44Z");
    }

    #[test]
    fn now_utc_has_the_fixed_width_and_trailing_z() {
        let s = now_utc();
        assert_eq!(s.len(), 20, "fixed width YYYY-MM-DDThh:mm:ssZ");
        assert!(s.ends_with('Z'), "trailing Z (UTC)");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
    }

    #[test]
    fn file_mtime_utc_formats_a_known_file_mtime() {
        use std::time::Duration;
        // Write a tempfile and set a KNOWN mtime; assert the formatted string
        // matches the SAME epoch routed through format_epoch_utc.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("stamp.txt");
        std::fs::write(&path, b"x").expect("write tempfile");
        let epoch = 1_791_735_524i64; // 2026-10-11T16:18:44Z
        let mtime = UNIX_EPOCH + Duration::from_secs(epoch as u64);
        let f = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open for set_modified");
        f.set_modified(mtime).expect("set mtime");
        drop(f);
        assert_eq!(file_mtime_utc(&path), format_epoch_utc(epoch));
    }

    #[test]
    fn file_mtime_utc_falls_back_for_a_missing_path() {
        let missing = std::path::Path::new("definitely-no-such-file-xyzzy.txt");
        let s = file_mtime_utc(missing);
        assert_eq!(s.len(), 20, "fallback is a fixed-shape now_utc stamp");
        assert!(s.ends_with('Z'), "trailing Z (UTC)");
    }
}
