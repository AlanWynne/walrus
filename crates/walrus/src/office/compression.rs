use std::io::Write;

use crate::Result;

pub(super) fn matching_deflate_level(content: &[u8], original: &[u8]) -> Result<Option<u8>> {
    let levels = [6, 9, 1, 0, 2, 3, 4, 5, 7, 8];
    for level in levels {
        if deflate(content, level)? == original {
            return Ok(Some(level));
        }
    }
    Ok(None)
}

pub(super) fn deflate(content: &[u8], level: u8) -> Result<Vec<u8>> {
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(u32::from(level)));
    encoder.write_all(content)?;
    Ok(encoder.finish()?)
}
