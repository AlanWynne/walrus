//! The per-file history view and Markdown change-report export (FR-K9).
//!
//! Two pieces, both driven through the format-agnostic [`HistorySource`] seam
//! so this module never names a `walrus_fmt` type (FR-K1 AC2 boundary):
//!   - [`history_report_markdown`]: a PURE, deterministic formatter turning a
//!     file's [`HistoryEntry`] list into a stable Markdown table. Fully
//!     headless-testable (no clock, no locale, no map iteration).
//!   - [`history_panel`]: an egui panel listing the selected file's entries in
//!     index order and offering an "Export report" button that yields the
//!     generated Markdown as an intent for the app shell to save.
//!
//! The `rfd` save dialog and pixel-exact appearance are the only MANUAL
//! carve-outs (see the stated-reason comments at the call sites); the row
//! listing and the export-intent string are proven headlessly.

use eframe::egui;

use crate::source::{FileRef, HistoryEntry, HistorySource};

// === Markdown report (pure, deterministic) ==============================

/// Render a file's change history as a deterministic Markdown report (FR-K9
/// AC2).
///
/// The output is a level-1 heading, a blank line, then a GitHub-style table
/// with a fixed header and one row per entry IN THE GIVEN ORDER (the caller
/// passes `source.history(f)`, already index-ordered -- this does not re-sort).
/// Every cell is fixed-shape so the whole string is deterministic:
///   - Index: the decimal `entry.index`.
///   - Timestamp: the fixed ISO-8601 UTC shape `YYYY-MM-DDThh:mm:ssZ` when
///     `Some`, else the literal `--`.
///   - Note: the note text when `Some` (any `|` escaped as `\|` so the table
///     stays well-formed), else `--`.
///   - Splice: the fixed form `@<at> -<deleted_len> +<inserted_len>`.
///
/// A trailing newline closes the document.
pub fn history_report_markdown(file_name: &str, entries: &[HistoryEntry]) -> String {
    let mut out = String::new();
    out.push_str("# History: ");
    out.push_str(file_name);
    out.push('\n');
    out.push('\n');
    out.push_str("| Index | Timestamp | Note | Splice |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for entry in entries {
        let timestamp = match entry.timestamp {
            Some(secs) => format_epoch_iso(secs),
            None => "--".to_string(),
        };
        let note = match &entry.note {
            Some(text) => escape_pipes(text),
            None => "--".to_string(),
        };
        out.push_str(&format!(
            "| {} | {} | {} | @{} -{} +{} |\n",
            entry.index,
            timestamp,
            note,
            entry.splice.at,
            entry.splice.deleted_len,
            entry.splice.inserted_len,
        ));
    }
    out
}

/// Escape every `|` in a table cell as `\|` so a note never breaks the table.
fn escape_pipes(text: &str) -> String {
    text.replace('|', "\\|")
}

/// Format epoch seconds as the fixed ISO-8601 UTC shape `YYYY-MM-DDThh:mm:ssZ`.
///
/// The dependency-free inverse of `source/walrus.rs`'s `parse_iso_epoch`: no
/// date crate, no locale, so the output is byte-stable for a given input. Only
/// non-negative epoch seconds occur here (timestamps come from the writer).
fn format_epoch_iso(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Civil `(year, month, day)` from days since the Unix epoch (Howard Hinnant's
/// civil_from_days, the inverse of walrus.rs's `days_from_civil`).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

// === egui history panel =================================================

/// What the history panel asked the app to do this frame.
///
/// `export_markdown` is set to the generated report string when the user
/// clicked "Export report"; the app shell saves it via `rfd` (MANUAL).
#[derive(Debug, Default)]
pub struct HistoryPanelAction {
    /// The Markdown report the app should save this frame, if requested.
    pub export_markdown: Option<String>,
}

/// Draw the per-file history view for `file` and return the frame's action.
///
/// Lists `source.history(file)` in index order: one non-interactive label per
/// entry carrying the index, the formatted timestamp (`--` when None), the note
/// (when `Some`), and the splice summary `@<at> -<deleted> +<inserted>`. Each
/// row gets a stable egui id so a headless harness can target it. An "Export
/// report" button yields the deterministic Markdown as an export intent.
pub fn history_panel(
    ui: &mut egui::Ui,
    source: &dyn HistorySource,
    file: &FileRef,
) -> HistoryPanelAction {
    let mut action = HistoryPanelAction::default();
    let entries = source.history(file);

    ui.label(format!("History: {}", file.name()));

    for entry in &entries {
        let timestamp = match entry.timestamp {
            Some(secs) => format_epoch_iso(secs),
            None => "--".to_string(),
        };
        let note = entry.note.as_deref().unwrap_or("--");
        // The row label embeds the index so it is unique for accessibility-tree
        // queries; the stable id lets a harness target the row directly.
        let label = format!(
            "#{} {} {} @{} -{} +{}",
            entry.index,
            timestamp,
            note,
            entry.splice.at,
            entry.splice.deleted_len,
            entry.splice.inserted_len,
        );
        ui.push_id(("history-row", entry.index), |ui| {
            ui.label(label);
        });
    }

    // The pure report string is produced here and returned as an intent; the
    // actual save is the app's MANUAL `rfd` dialog (not headless-drivable).
    if ui.button("Export report").clicked() {
        action.export_markdown = Some(history_report_markdown(file.name(), &entries));
    }

    action
}

// === Tests ==============================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SpliceSummary;

    fn entry(index: usize, timestamp: Option<i64>, note: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            index,
            timestamp,
            note: note.map(str::to_string),
            splice: SpliceSummary {
                at: index,
                deleted_len: 0,
                inserted_len: index + 1,
            },
        }
    }

    #[test]
    fn report_renders_heading_table_header_and_one_row_per_entry() {
        let entries = vec![entry(0, Some(0), Some("first")), entry(1, None, None)];
        let md = history_report_markdown("notes.txt", &entries);
        let expected = "# History: notes.txt\n\
\n\
| Index | Timestamp | Note | Splice |\n\
| --- | --- | --- | --- |\n\
| 0 | 1970-01-01T00:00:00Z | first | @0 -0 +1 |\n\
| 1 | -- | -- | @1 -0 +2 |\n";
        assert_eq!(md, expected);
    }

    #[test]
    fn report_formats_a_present_timestamp_as_fixed_iso_utc() {
        assert_eq!(format_epoch_iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_epoch_iso(1_791_460_800), "2026-10-08T12:00:00Z");
    }

    #[test]
    fn report_shows_double_dash_for_a_missing_timestamp_and_note() {
        let md = history_report_markdown("f.txt", &[entry(0, None, None)]);
        assert!(md.contains("| 0 | -- | -- | @0 -0 +1 |\n"));
    }

    #[test]
    fn report_escapes_a_pipe_in_a_note() {
        let md = history_report_markdown("f.txt", &[entry(0, None, Some("a|b"))]);
        assert!(
            md.contains("| a\\|b |"),
            "a pipe in a note is escaped: {md}"
        );
    }

    #[test]
    fn report_splice_cell_uses_the_at_deleted_inserted_form() {
        let e = HistoryEntry {
            index: 0,
            timestamp: None,
            note: None,
            splice: SpliceSummary {
                at: 5,
                deleted_len: 0,
                inserted_len: 3,
            },
        };
        let md = history_report_markdown("f.txt", &[e]);
        assert!(md.contains("@5 -0 +3"), "splice cell shape: {md}");
    }
}
