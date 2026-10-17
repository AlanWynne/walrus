use std::collections::BTreeSet;

use crate::manifest_group::DocumentGroup;
use crate::office_envelope::{CompressionMethod, OfficeEnvelope, ZipMember};
use crate::{Archive, FileId, Result, Version, WalrusError};

use super::compression::deflate;
use super::hash::sha256_hex;
use super::zip::checked_uncompressed_size;
use super::{office_envelope_error, MAX_OFFICE_UNCOMPRESSED_SIZE};

impl Archive {
    /// Reconstruct an office document at a document-group version.
    ///
    /// # Errors
    /// Returns a typed error for an unknown group, invalid version alignment,
    /// missing/corrupt payload blobs, or malformed ZIP metadata.
    pub fn extract_office(&self, group_selector: &str, version: Version) -> Result<Vec<u8>> {
        let group = self.group(group_selector).cloned().ok_or_else(|| {
            WalrusError::UnknownDocumentGroup {
                selector: group_selector.to_string(),
            }
        })?;
        let row = self.resolve_group_version(group_selector, version)?.clone();
        let envelope_bytes = self.reconstruct(
            &FileId::from(group.envelope_id.as_str()),
            Version::Index(row.envelope_index),
        )?;
        let envelope: OfficeEnvelope = serde_json::from_slice(&envelope_bytes)?;
        envelope.validate()?;
        validate_alignment(&group, &row, &envelope)?;

        let mut total_size = 0;
        let mut output = decode_hex(&envelope.zip.archive_prefix_hex)?;
        for member in &envelope.zip.members {
            total_size = checked_uncompressed_size(
                total_size,
                member.uncompressed_size,
                MAX_OFFICE_UNCOMPRESSED_SIZE,
            )?;
            validate_member_headers(member)?;
            let content = member_content(self, &group, &row, member)?;
            if content.len() as u64 != member.uncompressed_size
                || crc32fast::hash(&content) != member.crc32
            {
                return Err(office_envelope_error(
                    "member content size or CRC does not match the envelope",
                ));
            }
            let compressed = member_compressed_content(self, member, &content)?;
            if compressed.len() as u64 != member.compressed_size {
                return Err(office_envelope_error(
                    "compressed member size does not match the envelope",
                ));
            }
            output.extend_from_slice(&decode_hex(&member.local_header_hex)?);
            output.extend_from_slice(&compressed);
            if let Some(descriptor) = &member.data_descriptor_hex {
                output.extend_from_slice(&decode_hex(descriptor)?);
            }
        }
        append_central_directory(&mut output, &envelope)?;
        Ok(output)
    }
}

fn validate_alignment(
    group: &DocumentGroup,
    row: &crate::manifest_group::DocumentVersionRow,
    envelope: &OfficeEnvelope,
) -> Result<()> {
    if envelope.kind != group.kind {
        return Err(office_envelope_error(
            "envelope kind disagrees with its document group",
        ));
    }
    let mut referenced = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for member in &envelope.zip.members {
        if !paths.insert(member.path.as_str()) {
            return Err(office_envelope_error(
                "envelope contains duplicate member paths",
            ));
        }
        if let Some(id) = &member.member_id {
            if !group.member_ids.contains(id) || !referenced.insert(id.as_str()) {
                return Err(office_envelope_error(
                    "envelope references an unknown or duplicate text member",
                ));
            }
        }
    }
    let row_members: BTreeSet<_> = row.members.keys().map(String::as_str).collect();
    if referenced != row_members {
        return Err(office_envelope_error(
            "version row text membership disagrees with its envelope",
        ));
    }
    Ok(())
}

fn member_content(
    archive: &Archive,
    group: &DocumentGroup,
    row: &crate::manifest_group::DocumentVersionRow,
    member: &ZipMember,
) -> Result<Vec<u8>> {
    if let Some(id) = &member.member_id {
        let index = row
            .members
            .get(id)
            .ok_or_else(|| office_envelope_error("text member is missing from version row"))?;
        let tracked = archive
            .manifest()
            .file(id)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.clone() })?;
        if tracked.name != format!("{}/{}", group.name, member.path) {
            return Err(office_envelope_error(
                "text member path disagrees with its tracked-file name",
            ));
        }
        archive.reconstruct(&FileId::from(id.as_str()), Version::Index(*index))
    } else if let Some(hash) = &member.content_sha256 {
        verified_blob(archive, hash)
    } else {
        Err(office_envelope_error("member has no content reference"))
    }
}

fn member_compressed_content(
    archive: &Archive,
    member: &ZipMember,
    content: &[u8],
) -> Result<Vec<u8>> {
    match member.method {
        CompressionMethod::Store => {
            if member.compressed_sha256.is_some() || member.deflate_level.is_some() {
                return Err(office_envelope_error(
                    "STORED member cannot reference a compressed stream or DEFLATE level",
                ));
            }
            Ok(content.to_vec())
        }
        CompressionMethod::Deflate => {
            if let Some(hash) = &member.compressed_sha256 {
                verified_blob(archive, hash)
            } else if let Some(level) = member.deflate_level {
                deflate(content, level)
            } else {
                Err(office_envelope_error(
                    "DEFLATE member has no reproducible level or stored stream",
                ))
            }
        }
    }
}

fn verified_blob(archive: &Archive, hash: &str) -> Result<Vec<u8>> {
    let bytes = archive
        .blobs_ref()
        .get(hash)
        .ok_or_else(|| WalrusError::OfficeBlob {
            hash: hash.to_string(),
            reason: "payload is missing".to_string(),
        })?;
    if sha256_hex(bytes) != hash {
        return Err(WalrusError::OfficeBlob {
            hash: hash.to_string(),
            reason: "payload hash does not match its content".to_string(),
        });
    }
    Ok(bytes.clone())
}

fn validate_member_headers(member: &ZipMember) -> Result<()> {
    let local = decode_hex(&member.local_header_hex)?;
    let central = decode_hex(&member.central_header_hex)?;
    if local.len() < 30 || local.get(..4) != Some(b"PK\x03\x04") {
        return Err(office_envelope_error("invalid local ZIP header"));
    }
    let local_name_len = usize::from(read_u16(&local, 26)?);
    let local_extra_len = usize::from(read_u16(&local, 28)?);
    if 30usize
        .checked_add(local_name_len)
        .and_then(|length| length.checked_add(local_extra_len))
        != Some(local.len())
    {
        return Err(office_envelope_error("invalid local ZIP header length"));
    }
    let local_name = &local[30..30 + local_name_len];
    if local[30 + local_name_len..] != decode_hex(&member.local_extra_hex)?[..]
        || read_u16(&local, 6)? != member.gp_flags
        || read_u16(&local, 8)? != method_id(member.method)
    {
        return Err(office_envelope_error(
            "local ZIP header disagrees with envelope metadata",
        ));
    }
    let descriptor_present = member.gp_flags & 0x0008 != 0;
    let local_crc = read_u32(&local, 14)?;
    let local_compressed = read_u32(&local, 18)?;
    let local_uncompressed = read_u32(&local, 22)?;
    if local_compressed == u32::MAX || local_uncompressed == u32::MAX {
        return Err(office_envelope_error(
            "ZIP64 local member sizes are unsupported",
        ));
    }
    let local_fields_match = local_crc == member.crc32
        && u64::from(local_compressed) == member.compressed_size
        && u64::from(local_uncompressed) == member.uncompressed_size;
    let local_fields_empty = local_crc == 0 && local_compressed == 0 && local_uncompressed == 0;
    if (!descriptor_present && !local_fields_match)
        || (descriptor_present && !local_fields_match && !local_fields_empty)
    {
        return Err(office_envelope_error(
            "local ZIP sizes or CRC disagree with the envelope",
        ));
    }
    if descriptor_present != member.data_descriptor_hex.is_some() {
        return Err(office_envelope_error(
            "ZIP data-descriptor flag disagrees with envelope metadata",
        ));
    }

    if central.len() < 46 || central.get(..4) != Some(b"PK\x01\x02") {
        return Err(office_envelope_error("invalid central ZIP header"));
    }
    let central_name_len = usize::from(read_u16(&central, 28)?);
    let central_extra_len = usize::from(read_u16(&central, 30)?);
    let central_comment_len = usize::from(read_u16(&central, 32)?);
    if 46usize
        .checked_add(central_name_len)
        .and_then(|length| length.checked_add(central_extra_len))
        .and_then(|length| length.checked_add(central_comment_len))
        != Some(central.len())
    {
        return Err(office_envelope_error("invalid central ZIP header length"));
    }
    let central_name = &central[46..46 + central_name_len];
    if local_name != central_name
        || central[46 + central_name_len..46 + central_name_len + central_extra_len]
            != decode_hex(&member.central_extra_hex)?[..]
        || read_u16(&central, 8)? != member.gp_flags
        || read_u16(&central, 10)? != method_id(member.method)
        || read_u32(&central, 16)? != member.crc32
        || u64::from(read_u32(&central, 20)?) != member.compressed_size
        || u64::from(read_u32(&central, 24)?) != member.uncompressed_size
        || read_u16(&central, 12)? != member.dos_time
        || read_u16(&central, 14)? != member.dos_date
        || read_u16(&central, 4)? != member.version_made_by
        || read_u16(&central, 6)? != member.version_needed
        || read_u16(&central, 36)? != member.internal_attrs
        || read_u32(&central, 38)? != member.external_attrs
    {
        return Err(office_envelope_error(
            "central ZIP header disagrees with envelope metadata",
        ));
    }
    if let Some(descriptor) = &member.data_descriptor_hex {
        validate_data_descriptor(member, &decode_hex(descriptor)?)?;
    }
    Ok(())
}

fn validate_data_descriptor(member: &ZipMember, descriptor: &[u8]) -> Result<()> {
    let (offset, expected_length) = if descriptor.starts_with(b"PK\x07\x08") {
        (4, 16)
    } else {
        (0, 12)
    };
    if descriptor.len() != expected_length
        || read_u32(descriptor, offset)? != member.crc32
        || u64::from(read_u32(descriptor, offset + 4)?) != member.compressed_size
        || u64::from(read_u32(descriptor, offset + 8)?) != member.uncompressed_size
    {
        return Err(office_envelope_error("invalid ZIP data descriptor"));
    }
    Ok(())
}

fn append_central_directory(output: &mut Vec<u8>, envelope: &OfficeEnvelope) -> Result<()> {
    let mut directory_size = 0u64;
    for member in &envelope.zip.members {
        let header = decode_hex(&member.central_header_hex)?;
        directory_size = directory_size
            .checked_add(header.len() as u64)
            .ok_or_else(|| office_envelope_error("central directory size overflow"))?;
        output.extend_from_slice(&header);
    }
    let suffix = decode_hex(&envelope.zip.central_directory_suffix_hex)?;
    directory_size = directory_size
        .checked_add(suffix.len() as u64)
        .ok_or_else(|| office_envelope_error("central directory size overflow"))?;
    output.extend_from_slice(&suffix);
    let end_records = decode_hex(&envelope.zip.end_records_hex)?;
    let comment = decode_hex(&envelope.zip.archive_comment_hex)?;
    validate_end_records(
        &end_records,
        &comment,
        envelope.zip.members.len(),
        directory_size,
    )?;
    output.extend_from_slice(&end_records);
    output.extend_from_slice(&decode_hex(&envelope.zip.archive_trailing_hex)?);
    Ok(())
}

fn validate_end_records(
    end_records: &[u8],
    comment: &[u8],
    member_count: usize,
    directory_size: u64,
) -> Result<()> {
    if end_records.len() < 22 || end_records.get(..4) != Some(b"PK\x05\x06") {
        return Err(office_envelope_error(
            "invalid ZIP end-of-central-directory record",
        ));
    }
    let comment_len = usize::from(read_u16(end_records, 20)?);
    if end_records.len() != 22 + comment_len
        || end_records.get(22..) != Some(comment)
        || usize::from(read_u16(end_records, 8)?) != member_count
        || usize::from(read_u16(end_records, 10)?) != member_count
        || u64::from(read_u32(end_records, 12)?) != directory_size
        || read_u16(end_records, 4)? != 0
        || read_u16(end_records, 6)? != 0
    {
        return Err(office_envelope_error(
            "ZIP end record disagrees with its members or archive comment",
        ));
    }
    Ok(())
}

fn method_id(method: CompressionMethod) -> u16 {
    match method {
        CompressionMethod::Store => 0,
        CompressionMethod::Deflate => 8,
    }
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(office_envelope_error("invalid odd-length hex value"));
    }
    pairs
        .iter()
        .map(|pair| {
            let high = hex_digit(pair[0])?;
            let low = hex_digit(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_digit(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(office_envelope_error("invalid lowercase hex value")),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let field = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| office_envelope_error("truncated ZIP metadata field"))?;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let field = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| office_envelope_error("truncated ZIP metadata field"))?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}
