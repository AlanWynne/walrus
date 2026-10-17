use std::io::{Cursor, Read};

use crate::{OfficeKind, Result, WalrusError};

use super::{office_zip_error, zip_error};

pub(crate) fn detect_office(bytes: &[u8]) -> Result<Option<OfficeKind>> {
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Err(WalrusError::UnsupportedOfficeFormat {
            format: "OLE2/CFBF".to_string(),
        });
    }
    if !bytes.starts_with(b"PK\x03\x04") && !bytes.windows(4).any(|window| window == b"PK\x03\x04")
    {
        return Ok(None);
    }

    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let first_index = (0..archive.len())
        .map(|index| {
            let member = archive.by_index(index).map_err(zip_error)?;
            Ok((member.header_start(), index))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .min_by_key(|(offset, _)| *offset)
        .map(|(_, index)| index);

    if let Some(index) = first_index {
        let mut first = archive.by_index(index).map_err(zip_error)?;
        if first.name() == "mimetype" {
            if first.encrypted() {
                return Err(office_zip_error(
                    "encrypted ODF mimetype member is unsupported",
                ));
            }
            if first.compression() != zip::CompressionMethod::Stored {
                return Err(office_zip_error("ODF mimetype member must be stored"));
            }
            if first.size() > 128 {
                return Err(office_zip_error("ODF mimetype member is too large"));
            }
            let mut mimetype = Vec::new();
            first
                .read_to_end(&mut mimetype)
                .map_err(|error| office_zip_error(&error.to_string()))?;
            if mimetype.starts_with(b"application/vnd.oasis.opendocument.")
                && mimetype.len() <= 128
                && mimetype.is_ascii()
                && !mimetype.iter().any(u8::is_ascii_whitespace)
            {
                return Ok(Some(OfficeKind::Odf));
            }
        }
    }

    for index in 0..archive.len() {
        let member = archive.by_index(index).map_err(zip_error)?;
        if member.encrypted() {
            return Err(office_zip_error("encrypted ZIP members are unsupported"));
        }
        if member.name() == "[Content_Types].xml" {
            return Ok(Some(OfficeKind::Ooxml));
        }
    }
    Err(WalrusError::UnrecognizedOfficeZip)
}
