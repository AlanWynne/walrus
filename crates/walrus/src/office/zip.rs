use std::io::{Cursor, Read};

use crate::office_envelope::{ZipEnvelope, ZipMember};
use crate::{OfficeKind, Result, WalrusError};

use super::detect::detect_office;
use super::{office_zip_error, raw_zip, zip_error, MAX_OFFICE_UNCOMPRESSED_SIZE};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParsedOffice {
    pub(crate) kind: OfficeKind,
    pub(crate) members: Vec<ParsedOfficeMember>,
    pub(crate) zip: ZipEnvelope,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParsedOfficeMember {
    pub(crate) path: String,
    pub(crate) content: Vec<u8>,
    pub(crate) compressed_content: Vec<u8>,
}

pub(crate) fn parse_office(bytes: &[u8]) -> Result<Option<ParsedOffice>> {
    let Some(kind) = detect_office(bytes)? else {
        return Ok(None);
    };
    let mut archive = ::zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_error)?;
    let mut local_order = (0..archive.len())
        .map(|index| {
            let member = archive.by_index(index).map_err(zip_error)?;
            Ok((member.header_start(), index))
        })
        .collect::<Result<Vec<_>>>()?;
    local_order.sort_unstable();

    let mut total_size = 0;
    let mut members = Vec::with_capacity(local_order.len());
    let mut zip_members = Vec::<ZipMember>::with_capacity(local_order.len());
    let mut layouts = Vec::with_capacity(local_order.len());
    for (_, index) in local_order {
        let mut member = archive.by_index(index).map_err(zip_error)?;
        if member.encrypted() {
            return Err(office_zip_error("encrypted ZIP members are unsupported"));
        }
        let declared_size = member.size();
        total_size =
            checked_uncompressed_size(total_size, declared_size, MAX_OFFICE_UNCOMPRESSED_SIZE)?;

        let mut content = Vec::new();
        let remaining = MAX_OFFICE_UNCOMPRESSED_SIZE.saturating_sub(total_size - declared_size);
        (&mut member)
            .take(remaining.saturating_add(1))
            .read_to_end(&mut content)
            .map_err(|error| office_zip_error(&error.to_string()))?;
        if content.len() as u64 > remaining {
            return Err(WalrusError::OfficeUncompressedSizeLimit {
                actual: MAX_OFFICE_UNCOMPRESSED_SIZE.saturating_add(1),
                limit: MAX_OFFICE_UNCOMPRESSED_SIZE,
            });
        }
        if content.len() as u64 != declared_size {
            return Err(office_zip_error(
                "ZIP member size differs from its declared uncompressed size",
            ));
        }

        let (metadata, compressed_content, layout) = raw_zip::parse_raw_member(bytes, &member)?;
        let mut metadata = metadata;
        if metadata.method == crate::office_envelope::CompressionMethod::Deflate {
            metadata.deflate_level =
                super::compression::matching_deflate_level(&content, &compressed_content)?;
        }
        members.push(ParsedOfficeMember {
            path: member.name().to_string(),
            content,
            compressed_content,
        });
        zip_members.push(metadata);
        layouts.push(layout);
    }
    let zip = raw_zip::assemble_zip_envelope(bytes, &archive, zip_members, &layouts)?;
    Ok(Some(ParsedOffice { kind, members, zip }))
}

pub(super) fn checked_uncompressed_size(total: u64, additional: u64, limit: u64) -> Result<u64> {
    let actual = total.saturating_add(additional);
    if actual > limit {
        return Err(WalrusError::OfficeUncompressedSizeLimit { actual, limit });
    }
    Ok(actual)
}
