//! CSV export backend (feature `export-csv`).

use csv::{ReaderBuilder, WriterBuilder};

/// Export tabular document bytes as CSV via a parse/re-serialise round trip.
///
/// Flexible parsing preserves ragged rows (records with differing field
/// counts) rather than rejecting them, so a round trip reproduces the row
/// structure. Fields are re-quoted by the writer as needed.
///
/// # Errors
/// Returns [`crate::WalrusError::Export`] if the CSV cannot be parsed or
/// written. Never panics.
pub fn to_csv(doc: &[u8]) -> crate::Result<Vec<u8>> {
    let mut reader = ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(doc);

    let mut writer = WriterBuilder::new().flexible(true).from_writer(Vec::new());

    for record in reader.records() {
        let record = record.map_err(|e| crate::WalrusError::Export {
            backend: "csv",
            reason: e.to_string(),
        })?;
        writer
            .write_record(&record)
            .map_err(|e| crate::WalrusError::Export {
                backend: "csv",
                reason: e.to_string(),
            })?;
    }

    let inner = writer
        .into_inner()
        .map_err(|e| crate::WalrusError::Export {
            backend: "csv",
            reason: e.to_string(),
        })?;
    Ok(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rows_preserving_ragged_records() {
        // Second row has fewer fields than the first; flexible parsing keeps it.
        let input = b"a,b,c\n1,2\nx,y,z\n";
        let out = to_csv(input).expect("csv export");
        let text = String::from_utf8(out).expect("utf-8");
        assert_eq!(text, "a,b,c\n1,2\nx,y,z\n");
    }

    #[test]
    fn quotes_fields_containing_commas() {
        let input = b"\"a,b\",c\n";
        let out = to_csv(input).expect("csv export");
        let text = String::from_utf8(out).expect("utf-8");
        assert_eq!(text, "\"a,b\",c\n");
    }
}
