//! CSV column-grid view (design 2.2). Used ONLY for actual `text/csv`.
//!
//! Parsing (bytes -> rows) is a pure helper so it is unit-tested without egui;
//! the grid is painted with `egui_extras::TableBuilder` (unconditional in
//! egui_extras -- there is NO separate `table` feature).

use eframe::egui;
use egui_extras::{Column, TableBuilder};

/// Parse CSV bytes into rows of string cells (pure, testable).
///
/// Ragged rows are preserved as-is. The reader is configured with
/// `flexible(true)` and no header interpretation so the grid shows every row,
/// including the first, verbatim.
pub fn parse_rows(bytes: &[u8]) -> Result<Vec<Vec<String>>, csv::Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(bytes);
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        rows.push(record.iter().map(|f| f.to_string()).collect());
    }
    Ok(rows)
}

/// Paint the parsed rows as an aligned, scrollable grid. The first row is shown
/// as the header; remaining rows are the body.
pub fn paint(ui: &mut egui::Ui, rows: &[Vec<String>]) {
    if rows.is_empty() {
        ui.label("(empty CSV)");
        return;
    }
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        ui.label("(no columns)");
        return;
    }
    let (header, body) = rows.split_first().expect("rows is non-empty");

    let mut builder = TableBuilder::new(ui).striped(true);
    for _ in 0..columns {
        builder = builder.column(Column::auto().resizable(true));
    }
    builder
        .header(20.0, |mut head| {
            for c in 0..columns {
                head.col(|ui| {
                    let text = header.get(c).map(String::as_str).unwrap_or("");
                    ui.strong(text);
                });
            }
        })
        .body(|mut table_body| {
            for row in body {
                table_body.row(18.0, |mut row_ui| {
                    for c in 0..columns {
                        row_ui.col(|ui| {
                            let text = row.get(c).map(String::as_str).unwrap_or("");
                            ui.label(text);
                        });
                    }
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_simple_grid() {
        let csv = b"name,age\nalice,30\nbob,25\n";
        let rows = parse_rows(csv).expect("valid csv parses");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec!["name", "age"]);
        assert_eq!(rows[1], vec!["alice", "30"]);
    }

    #[test]
    fn preserves_ragged_rows() {
        let csv = b"a,b,c\n1,2\n3,4,5,6\n";
        let rows = parse_rows(csv).expect("flexible reader accepts ragged rows");
        assert_eq!(rows[0].len(), 3);
        assert_eq!(rows[1].len(), 2);
        assert_eq!(rows[2].len(), 4);
    }
}
