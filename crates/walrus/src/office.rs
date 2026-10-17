#[path = "office/compression.rs"]
mod compression;
#[path = "office/detect.rs"]
mod detect;
#[path = "office/hash.rs"]
mod hash;
#[path = "office/ingest.rs"]
mod ingest;
#[path = "office/raw_zip.rs"]
mod raw_zip;
#[path = "office/repack.rs"]
mod repack;
#[cfg(test)]
#[path = "office/tests.rs"]
mod tests;
#[path = "office/zip.rs"]
mod zip;

const MAX_OFFICE_UNCOMPRESSED_SIZE: u64 = 1024 * 1024 * 1024;

/// Detect whether `bytes` contain a supported ODF or OOXML ZIP document.
///
/// # Errors
/// Returns a typed error for malformed ZIP data or unsupported OLE2 office
/// containers.
pub fn detect_office(bytes: &[u8]) -> crate::Result<Option<crate::OfficeKind>> {
    detect::detect_office(bytes)
}

fn zip_error(error: ::zip::result::ZipError) -> crate::WalrusError {
    office_zip_error(&error.to_string())
}

fn office_zip_error(reason: &str) -> crate::WalrusError {
    crate::WalrusError::OfficeZip {
        reason: reason.to_string(),
    }
}

fn office_envelope_error(reason: &str) -> crate::WalrusError {
    crate::WalrusError::OfficeEnvelope {
        reason: reason.to_string(),
    }
}
