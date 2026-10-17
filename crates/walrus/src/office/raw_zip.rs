use std::io::Cursor;

use crate::office_envelope::{CompressionMethod, ZipEnvelope, ZipMember};
use crate::Result;

use super::office_zip_error;

pub(super) struct RawMemberLayout {
    local_header_start: u64,
    data_start: u64,
    central_header_start: u64,
    compressed_size: u64,
    central_header_length: usize,
    descriptor_length: usize,
}

pub(super) fn parse_raw_member(
    archive_bytes: &[u8],
    member: &::zip::read::ZipFile<'_, Cursor<&[u8]>>,
) -> Result<(ZipMember, Vec<u8>, RawMemberLayout)> {
    let local_header_start = member.header_start();
    let data_start = member.data_start();
    let central_header_start = member.central_header_start();
    let compressed_size = member.compressed_size();
    let uncompressed_size = member.size();
    let crc32 = member.crc32();
    let raw_name = member.name_raw();
    let path = member.name();
    let local_start = usize_from_u64(local_header_start)?;
    let data_offset = usize_from_u64(data_start)?;
    let local_header = archive_bytes
        .get(local_start..data_offset)
        .ok_or_else(|| office_zip_error("invalid local ZIP header range"))?;
    if local_header.len() < 30 || &local_header[..4] != b"PK\x03\x04" {
        return Err(office_zip_error("invalid local ZIP header"));
    }
    let local_name_length = usize::from(read_u16(local_header, 26)?);
    let local_extra_length = usize::from(read_u16(local_header, 28)?);
    let local_data_offset = 30usize
        .checked_add(local_name_length)
        .and_then(|offset| offset.checked_add(local_extra_length))
        .ok_or_else(|| office_zip_error("local ZIP header length overflow"))?;
    if local_data_offset != local_header.len()
        || local_header.get(30..30 + local_name_length) != Some(raw_name)
    {
        return Err(office_zip_error(
            "local ZIP header name or length disagrees with parsed member",
        ));
    }
    let local_extra = &local_header[30 + local_name_length..];
    reject_zip64_extra(local_extra)?;
    let local_flags = read_u16(local_header, 6)?;
    let local_method = read_u16(local_header, 8)?;

    let central_start = usize_from_u64(central_header_start)?;
    let central_fixed = archive_bytes
        .get(central_start..central_start.saturating_add(46))
        .ok_or_else(|| office_zip_error("invalid central ZIP header range"))?;
    if &central_fixed[..4] != b"PK\x01\x02" {
        return Err(office_zip_error("invalid central ZIP header"));
    }
    let name_length = usize::from(read_u16(central_fixed, 28)?);
    let extra_length = usize::from(read_u16(central_fixed, 30)?);
    let comment_length = usize::from(read_u16(central_fixed, 32)?);
    let central_header_length = 46usize
        .checked_add(name_length)
        .and_then(|length| length.checked_add(extra_length))
        .and_then(|length| length.checked_add(comment_length))
        .ok_or_else(|| office_zip_error("central ZIP header length overflow"))?;
    let central_header = archive_bytes
        .get(central_start..central_start.saturating_add(central_header_length))
        .ok_or_else(|| office_zip_error("truncated central ZIP header"))?;
    if central_header.get(46..46 + name_length) != Some(raw_name) {
        return Err(office_zip_error(
            "central ZIP header name disagrees with parsed member",
        ));
    }
    let central_extra = &central_header[46 + name_length..46 + name_length + extra_length];
    reject_zip64_extra(central_extra)?;

    let flags = read_u16(central_header, 8)?;
    let method_id = read_u16(central_header, 10)?;
    let central_crc = read_u32(central_header, 16)?;
    let central_compressed_size = read_u32(central_header, 20)?;
    let central_uncompressed_size = read_u32(central_header, 24)?;
    if local_flags != flags || local_method != method_id {
        return Err(office_zip_error(
            "local and central ZIP headers disagree on flags or compression",
        ));
    }
    if central_compressed_size == u32::MAX || central_uncompressed_size == u32::MAX {
        return Err(office_zip_error("ZIP64 member sizes are unsupported"));
    }
    if u64::from(central_compressed_size) != compressed_size
        || u64::from(central_uncompressed_size) != uncompressed_size
        || central_crc != crc32
    {
        return Err(office_zip_error(
            "central ZIP metadata disagrees with parsed member",
        ));
    }

    let method = match (method_id, member.compression()) {
        (0, ::zip::CompressionMethod::Stored) => CompressionMethod::Store,
        (8, ::zip::CompressionMethod::Deflated) => CompressionMethod::Deflate,
        _ => return Err(office_zip_error("unsupported ZIP compression method")),
    };
    let compressed_end = data_offset
        .checked_add(usize_from_u64(compressed_size)?)
        .ok_or_else(|| office_zip_error("compressed ZIP data range overflow"))?;
    let compressed_content = archive_bytes
        .get(data_offset..compressed_end)
        .ok_or_else(|| office_zip_error("truncated compressed ZIP member"))?
        .to_vec();
    let descriptor = if flags & 0x0008 != 0 {
        Some(parse_data_descriptor(
            archive_bytes,
            compressed_end,
            central_crc,
            central_compressed_size,
            central_uncompressed_size,
        )?)
    } else {
        None
    };
    let data_descriptor_hex = descriptor.as_deref().map(encode_hex);
    let descriptor_length = descriptor.as_ref().map_or(0, Vec::len);

    Ok((
        ZipMember {
            path: path.to_string(),
            member_id: None,
            content_sha256: None,
            compressed_sha256: None,
            deflate_level: None,
            method,
            crc32,
            compressed_size,
            uncompressed_size,
            dos_time: read_u16(central_header, 12)?,
            dos_date: read_u16(central_header, 14)?,
            gp_flags: flags,
            version_made_by: read_u16(central_header, 4)?,
            version_needed: read_u16(central_header, 6)?,
            external_attrs: read_u32(central_header, 38)?,
            internal_attrs: read_u16(central_header, 36)?,
            local_extra_hex: encode_hex(local_extra),
            central_extra_hex: encode_hex(central_extra),
            local_header_hex: encode_hex(local_header),
            central_header_hex: encode_hex(central_header),
            data_descriptor_hex,
            extra: Default::default(),
        },
        compressed_content,
        RawMemberLayout {
            local_header_start,
            data_start,
            central_header_start,
            compressed_size,
            central_header_length,
            descriptor_length,
        },
    ))
}

pub(super) fn assemble_zip_envelope(
    bytes: &[u8],
    archive: &::zip::ZipArchive<Cursor<&[u8]>>,
    members: Vec<ZipMember>,
    layouts: &[RawMemberLayout],
) -> Result<ZipEnvelope> {
    if members.is_empty() || members.len() != layouts.len() {
        return Err(office_zip_error("office ZIP has no consistent member list"));
    }
    let first_local = usize_from_u64(layouts[0].local_header_start)?;
    let central_start = usize_from_u64(archive.central_directory_start())?;
    for (index, layout) in layouts.iter().enumerate() {
        if index > 0
            && (layout.local_header_start <= layouts[index - 1].local_header_start
                || layout.central_header_start <= layouts[index - 1].central_header_start)
        {
            return Err(office_zip_error(
                "ZIP central-directory order differs from local member order",
            ));
        }
        let local_end = usize_from_u64(layout.data_start)?
            .checked_add(usize_from_u64(layout.compressed_size)?)
            .and_then(|end| end.checked_add(layout.descriptor_length))
            .ok_or_else(|| office_zip_error("ZIP local member range overflow"))?;
        let next_local_start = layouts
            .get(index + 1)
            .map(|next| usize_from_u64(next.local_header_start))
            .transpose()?
            .unwrap_or(central_start);
        if local_end != next_local_start {
            return Err(office_zip_error(
                "gaps or overlapping records between ZIP local members are unsupported",
            ));
        }
        let central_end = usize_from_u64(layout.central_header_start)?
            .checked_add(layout.central_header_length)
            .ok_or_else(|| office_zip_error("ZIP central member range overflow"))?;
        let next_central_start = layouts
            .get(index + 1)
            .map(|next| usize_from_u64(next.central_header_start))
            .transpose()?
            .unwrap_or(central_end);
        if index == 0 && usize_from_u64(layout.central_header_start)? != central_start {
            return Err(office_zip_error(
                "unsupported data before the first central-directory entry",
            ));
        }
        if index + 1 < layouts.len() && central_end != next_central_start {
            return Err(office_zip_error(
                "gaps between central-directory entries are unsupported",
            ));
        }
    }

    let last = layouts
        .last()
        .ok_or_else(|| office_zip_error("empty ZIP"))?;
    let central_end = usize_from_u64(last.central_header_start)?
        .checked_add(last.central_header_length)
        .ok_or_else(|| office_zip_error("ZIP central directory range overflow"))?;
    let (end_start, end_after_comment) =
        find_end_record(bytes, archive, central_start, central_end)?;
    if archive.zip64_comment().is_some()
        || (end_start >= 20 && bytes.get(end_start - 20..end_start - 16) == Some(b"PK\x06\x07"))
    {
        return Err(office_zip_error("ZIP64 end records are unsupported"));
    }
    let end_records = bytes
        .get(end_start..end_after_comment)
        .ok_or_else(|| office_zip_error("invalid ZIP end-record range"))?;
    let central_suffix = bytes
        .get(central_end..end_start)
        .ok_or_else(|| office_zip_error("invalid central-directory suffix range"))?;
    let trailing = bytes
        .get(end_after_comment..)
        .ok_or_else(|| office_zip_error("invalid ZIP trailing-data range"))?;
    let prefix = bytes
        .get(..first_local)
        .ok_or_else(|| office_zip_error("invalid ZIP prefix range"))?;

    Ok(ZipEnvelope {
        archive_prefix_hex: encode_hex(prefix),
        archive_comment_hex: encode_hex(archive.comment()),
        central_directory_suffix_hex: encode_hex(central_suffix),
        end_records_hex: encode_hex(end_records),
        archive_trailing_hex: encode_hex(trailing),
        members,
        extra: Default::default(),
    })
}

fn find_end_record(
    bytes: &[u8],
    archive: &::zip::ZipArchive<Cursor<&[u8]>>,
    central_start: usize,
    central_end: usize,
) -> Result<(usize, usize)> {
    let last_candidate = bytes.len().saturating_sub(22);
    if central_end > last_candidate {
        return Err(office_zip_error(
            "ZIP end-of-central-directory record is missing",
        ));
    }
    for start in (central_end..=last_candidate).rev() {
        if bytes.get(start..start + 4) != Some(b"PK\x05\x06") {
            continue;
        }
        let comment_length = usize::from(read_u16(bytes, start + 20)?);
        let end = start
            .checked_add(22)
            .and_then(|offset| offset.checked_add(comment_length))
            .ok_or_else(|| office_zip_error("ZIP end-record length overflow"))?;
        if end > bytes.len() || bytes.get(start + 22..end) != Some(archive.comment()) {
            continue;
        }
        let disk = read_u16(bytes, start + 4)?;
        let directory_disk = read_u16(bytes, start + 6)?;
        let disk_entries = read_u16(bytes, start + 8)?;
        let total_entries = read_u16(bytes, start + 10)?;
        let directory_size = read_u32(bytes, start + 12)?;
        let directory_offset = read_u32(bytes, start + 16)?;
        if [disk_entries, total_entries].contains(&u16::MAX)
            || directory_size == u32::MAX
            || directory_offset == u32::MAX
        {
            return Err(office_zip_error("ZIP64 end records are unsupported"));
        }
        let expected_start = archive.offset().checked_add(u64::from(directory_offset));
        let expected_end = usize::try_from(directory_size)
            .ok()
            .and_then(|size| central_start.checked_add(size));
        if disk == 0
            && directory_disk == 0
            && disk_entries == total_entries
            && usize::from(total_entries) == archive.len()
            && expected_start == u64::try_from(central_start).ok()
            && expected_end == Some(start)
        {
            return Ok((start, end));
        }
    }
    Err(office_zip_error(
        "cannot locate a consistent ZIP end-of-central-directory record",
    ))
}

fn parse_data_descriptor(
    archive_bytes: &[u8],
    start: usize,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
) -> Result<Vec<u8>> {
    let prefix = archive_bytes
        .get(start..start.saturating_add(4))
        .ok_or_else(|| office_zip_error("truncated ZIP data descriptor"))?;
    let (offset, descriptor_length) = if prefix == b"PK\x07\x08" {
        (4, 16)
    } else {
        (0, 12)
    };
    let descriptor = archive_bytes
        .get(start..start.saturating_add(descriptor_length))
        .ok_or_else(|| office_zip_error("truncated ZIP data descriptor"))?;
    if read_u32(descriptor, offset)? != crc32
        || read_u32(descriptor, offset + 4)? != compressed_size
        || read_u32(descriptor, offset + 8)? != uncompressed_size
    {
        return Err(office_zip_error(
            "ZIP data descriptor disagrees with central header",
        ));
    }
    Ok(descriptor.to_vec())
}

fn reject_zip64_extra(extra: &[u8]) -> Result<()> {
    let mut offset = 0usize;
    while offset < extra.len() {
        let header_end = offset
            .checked_add(4)
            .ok_or_else(|| office_zip_error("ZIP extra-field length overflow"))?;
        let header = extra
            .get(offset..header_end)
            .ok_or_else(|| office_zip_error("truncated ZIP extra field"))?;
        let field_id = u16::from_le_bytes([header[0], header[1]]);
        let field_length = usize::from(u16::from_le_bytes([header[2], header[3]]));
        let data_end = header_end
            .checked_add(field_length)
            .ok_or_else(|| office_zip_error("ZIP extra-field length overflow"))?;
        if data_end > extra.len() {
            return Err(office_zip_error("truncated ZIP extra-field data"));
        }
        if field_id == 0x0001 {
            return Err(office_zip_error("ZIP64 extra fields are unsupported"));
        }
        offset = data_end;
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let field = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| office_zip_error("truncated ZIP metadata field"))?;
    Ok(u16::from_le_bytes([field[0], field[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let field = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| office_zip_error("truncated ZIP metadata field"))?;
    Ok(u32::from_le_bytes([field[0], field[1], field[2], field[3]]))
}

fn usize_from_u64(value: u64) -> Result<usize> {
    usize::try_from(value).map_err(|_| office_zip_error("ZIP offset exceeds addressable memory"))
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
