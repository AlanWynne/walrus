use super::zip::checked_uncompressed_size;
use super::{detect, zip};
use crate::{Archive, FileId, Manifest, OfficeKind, Variant, Version, WalrusError};
use std::io::{Cursor, Write};

use ::zip::write::{SimpleFileOptions, ZipWriter};

#[test]
fn parses_members_in_local_order_and_extracts_their_contents() {
    let bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);

    let parsed = zip::parse_office(&bytes)
        .expect("parse ODF")
        .expect("recognized office document");

    assert_eq!(parsed.kind, OfficeKind::Odf);
    assert_eq!(
        parsed
            .members
            .iter()
            .map(|member| member.path.as_str())
            .collect::<Vec<_>>(),
        ["mimetype", "content.xml"]
    );
    assert_eq!(
        parsed.members[0].content,
        b"application/vnd.oasis.opendocument.text"
    );
    assert_eq!(parsed.members[1].content, b"<document/>");
    assert_eq!(
        parsed.zip.members[0].method,
        crate::office_envelope::CompressionMethod::Store
    );
    assert_eq!(parsed.zip.members[0].deflate_level, None);
    assert_eq!(
        parsed.members[0].compressed_content,
        parsed.members[0].content
    );
    assert!(parsed.zip.archive_comment_hex.is_empty());
    assert_eq!(
        parsed.zip.members[0].crc32,
        crc32(&parsed.members[0].content)
    );
    assert_eq!(parsed.zip.members[0].dos_time, 0);
    assert_eq!(parsed.zip.members[0].dos_date, 0);
    assert_eq!(
        parsed.zip.members[0].local_header_hex,
        encode_hex(&bytes[..38])
    );
    let central_start = bytes
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .expect("central directory header");
    assert_eq!(
        parsed.zip.members[0].central_header_hex,
        encode_hex(&bytes[central_start..central_start + 54])
    );
    let end_record = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("end-of-central-directory record");
    assert_eq!(parsed.zip.end_records_hex, encode_hex(&bytes[end_record..]));
    assert!(parsed.zip.archive_prefix_hex.is_empty());
    assert!(parsed.zip.central_directory_suffix_hex.is_empty());
    assert!(parsed.zip.archive_trailing_hex.is_empty());
}

#[test]
fn adds_and_extracts_a_grouped_odf_document() {
    let bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.odt", &bytes)
        .expect("add ODF document");

    let group = archive.group("report.odt").expect("document group");
    assert_eq!(group.kind, OfficeKind::Odf);
    assert_eq!(group.member_ids.len(), 2);
    assert_eq!(group.version_map.len(), 1);
    let row = group.version_row(0).expect("current document version");
    for (id, index) in &row.members {
        assert_eq!(
            archive
                .reconstruct(&FileId::from(id.as_str()), Version::Index(*index))
                .expect("reconstruct member"),
            match archive.file(id).expect("tracked member").name.as_str() {
                "report.odt/mimetype" => b"application/vnd.oasis.opendocument.text".to_vec(),
                "report.odt/content.xml" => b"<document/>".to_vec(),
                other => panic!("unexpected office member {other}"),
            }
        );
    }
    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(0))
            .expect("extract ODF"),
        bytes
    );
}

#[test]
fn successive_office_saves_extract_current_and_older_bytes() {
    let first = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document>first</document>"),
    ]);
    let second = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document>second</document>"),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.odt", &first)
        .expect("add first version");
    archive
        .add_office("report.odt", &second)
        .expect("add second version");

    let group = archive.group("report.odt").expect("document group");
    assert_eq!(group.version_map.len(), 2);
    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(0))
            .expect("extract current"),
        second
    );
    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(1))
            .expect("extract older"),
        first
    );
}

#[test]
fn office_text_changes_use_member_history_without_a_monolithic_zip_blob() {
    let first = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document>first</document>"),
    ]);
    let second = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document>second</document>"),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));
    archive
        .add_office("report.odt", &first)
        .expect("add first version");
    archive
        .add_office("report.odt", &second)
        .expect("add second version");

    assert!(archive
        .blobs_ref()
        .values()
        .all(|blob| blob != &first && blob != &second));

    let group = archive.group("report.odt").expect("document group");
    for member_id in &group.member_ids {
        let member = archive.file(member_id).expect("tracked office member");
        let expected_versions = if member.name.ends_with("content.xml") {
            1
        } else {
            0
        };
        assert_eq!(member.versions.count, expected_versions, "{}", member.name);
    }
}

#[test]
fn office_membership_additions_and_removals_are_versioned() {
    let first = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);
    let second = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("styles.xml", b"<styles/>"),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.odt", &first)
        .expect("add first version");
    archive
        .add_office("report.odt", &second)
        .expect("add second version");

    let group = archive.group("report.odt").expect("document group");
    assert_eq!(group.member_ids.len(), 3);
    assert_eq!(group.version_map[0].members.len(), 2);
    assert_eq!(group.version_map[1].members.len(), 2);
    let content_id = group
        .member_ids
        .iter()
        .find(|id| {
            archive
                .file(id)
                .is_some_and(|file| file.name.ends_with("content.xml"))
        })
        .expect("removed member id");
    let styles_id = group
        .member_ids
        .iter()
        .find(|id| {
            archive
                .file(id)
                .is_some_and(|file| file.name.ends_with("styles.xml"))
        })
        .expect("added member id");
    assert!(!group.version_map[0].members.contains_key(content_id));
    assert!(group.version_map[1].members.contains_key(content_id));
    assert!(group.version_map[0].members.contains_key(styles_id));
    assert!(!group.version_map[1].members.contains_key(styles_id));
    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(1))
            .expect("extract prior membership"),
        first
    );
    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(0))
            .expect("extract current membership"),
        second
    );
}

#[test]
fn binary_office_members_are_content_addressed_and_verified() {
    let image = [0, 255, 4, 0, 128];
    let bytes = stored_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", b"<document/>"),
        ("word/media/image.bin", &image),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.docx", &bytes)
        .expect("add OOXML document");

    let group = archive.group("report.docx").expect("document group");
    let envelope_bytes = archive
        .reconstruct(&FileId::from(group.envelope_id.as_str()), Version::Index(0))
        .expect("reconstruct envelope");
    let envelope: crate::office_envelope::OfficeEnvelope =
        serde_json::from_slice(&envelope_bytes).expect("parse envelope");
    let image_member = envelope
        .zip
        .members
        .iter()
        .find(|member| member.path == "word/media/image.bin")
        .expect("binary member");
    let hash = image_member
        .content_sha256
        .as_ref()
        .expect("binary content hash");
    assert_eq!(
        archive.blobs_ref().get(hash).map(Vec::as_slice),
        Some(&image[..])
    );
    assert_eq!(archive.blobs_ref().len(), 1);
    assert_eq!(
        archive
            .extract_office("report.docx", Version::Index(0))
            .expect("extract OOXML"),
        bytes
    );

    let updated = stored_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", b"<document>updated</document>"),
        ("word/media/image.bin", &image),
    ]);
    archive
        .add_office("report.docx", &updated)
        .expect("save OOXML update");
    assert_eq!(archive.blobs_ref().len(), 1);
    assert_eq!(
        archive
            .extract_office("report.docx", Version::Index(1))
            .expect("extract older OOXML"),
        bytes
    );
    assert_eq!(
        archive
            .extract_office("report.docx", Version::Index(0))
            .expect("extract updated OOXML"),
        updated
    );
    let changed_image = [7, 0, 9, 10];
    let changed = stored_zip(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", b"<document>updated</document>"),
        ("word/media/image.bin", &changed_image),
    ]);
    archive
        .add_office("report.docx", &changed)
        .expect("save changed binary member");
    assert_eq!(archive.blobs_ref().len(), 2);
    assert_eq!(
        archive
            .extract_office("report.docx", Version::Index(1))
            .expect("extract prior binary version"),
        updated
    );
    assert_eq!(
        archive
            .extract_office("report.docx", Version::Index(0))
            .expect("extract changed binary"),
        changed
    );
    let serialized = archive.to_bytes().expect("serialize office archive");
    let reopened = Archive::read(&serialized).expect("reopen office archive");
    assert_eq!(
        reopened
            .extract_office("report.docx", Version::Index(0))
            .expect("extract reopened OOXML"),
        changed
    );

    archive.blobs_mut().remove(hash);
    assert!(matches!(
        archive.extract_office("report.docx", Version::Index(2)),
        Err(WalrusError::OfficeBlob { .. })
    ));
    archive
        .blobs_mut()
        .insert(hash.clone(), b"corrupt".to_vec());
    assert!(matches!(
        archive.extract_office("report.docx", Version::Index(2)),
        Err(WalrusError::OfficeBlob { .. })
    ));
}

#[test]
fn identical_office_save_does_not_create_a_version() {
    let bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.odt", &bytes)
        .expect("add document");
    archive
        .add_office("report.odt", &bytes)
        .expect("add identical document");

    assert_eq!(
        archive
            .group("report.odt")
            .expect("document group")
            .version_map
            .len(),
        1
    );
}

#[test]
fn office_ingest_collision_does_not_partially_add_a_group() {
    let bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);
    let mut archive = Archive::new(Manifest::new(Variant::Wal));
    archive
        .add("report.odt/content.xml", b"occupied", None, None)
        .expect("add unrelated file");
    let file_count = archive.files().count();

    assert!(archive.add_office("report.odt", &bytes).is_err());
    assert_eq!(archive.files().count(), file_count);
    assert!(archive.group("report.odt").is_none());
}

#[test]
fn identifies_odf_from_first_mimetype_member() {
    let bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);

    assert_eq!(
        detect::detect_office(&bytes).expect("detect ODF"),
        Some(OfficeKind::Odf)
    );
}

#[test]
fn identifies_ooxml_from_content_types_member() {
    let bytes = stored_zip(&[("[Content_Types].xml", b"<Types/>")]);

    assert_eq!(
        detect::detect_office(&bytes).expect("detect OOXML"),
        Some(OfficeKind::Ooxml)
    );
}

#[test]
fn parses_deflated_members_and_preserves_the_archive_comment() {
    let bytes = deflated_zip(
        &[
            ("[Content_Types].xml", &b"<Types/>"[..]),
            ("word/document.xml", &b"<document>content</document>"[..]),
        ],
        b"office-comment",
    );

    let parsed = zip::parse_office(&bytes)
        .expect("parse OOXML")
        .expect("recognized office document");

    assert_eq!(parsed.kind, OfficeKind::Ooxml);
    assert_eq!(
        parsed.zip.members[0].method,
        crate::office_envelope::CompressionMethod::Deflate
    );
    assert_ne!(
        parsed.members[0].compressed_content,
        parsed.members[0].content
    );
    assert!(parsed.zip.members[0].deflate_level.is_some());
    assert_eq!(
        parsed.zip.archive_comment_hex,
        encode_hex(b"office-comment")
    );
    assert_eq!(parsed.members[1].content, b"<document>content</document>");
}

#[test]
fn preserves_a_stored_member_data_descriptor() {
    let bytes = stored_zip_with_descriptor();
    let content = b"<document/>";

    let parsed = zip::parse_office(&bytes)
        .expect("parse ODF with descriptor")
        .expect("recognized office document");

    let member = &parsed.zip.members[1];
    let expected_descriptor = format!(
        "504b0708{}{}{}",
        encode_hex(&crc32(content).to_le_bytes()),
        encode_hex(&(content.len() as u32).to_le_bytes()),
        encode_hex(&(content.len() as u32).to_le_bytes())
    );
    assert_eq!(
        member.data_descriptor_hex.as_deref(),
        Some(expected_descriptor.as_str())
    );
}

#[test]
fn office_repack_preserves_data_descriptors_exactly() {
    let bytes = stored_zip_with_descriptor();
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.odt", &bytes)
        .expect("add ODF with data descriptor");

    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(0))
            .expect("extract ODF with data descriptor"),
        bytes
    );
}

#[test]
fn office_repack_preserves_deflate_streams_and_archive_comments() {
    let bytes = deflated_zip(
        &[
            ("[Content_Types].xml", &b"<Types/>"[..]),
            ("word/document.xml", &b"<document>content</document>"[..]),
        ],
        b"office-comment",
    );
    let mut archive = Archive::new(Manifest::new(Variant::Wal));

    archive
        .add_office("report.docx", &bytes)
        .expect("add deflated OOXML document");

    assert_eq!(
        archive
            .extract_office("report.docx", Version::Index(0))
            .expect("extract deflated OOXML document"),
        bytes
    );
}

#[test]
fn preserves_archive_prefix_and_trailing_bytes() {
    let mut bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);
    let original_end = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("end record");
    let mut prefix = b"stub!".to_vec();
    prefix.append(&mut bytes);
    let central_headers = prefix
        .windows(4)
        .enumerate()
        .filter_map(|(offset, window)| (window == b"PK\x01\x02").then_some(offset))
        .collect::<Vec<_>>();
    for header in central_headers {
        let local_offset = u32::from_le_bytes(
            prefix[header + 42..header + 46]
                .try_into()
                .expect("local header offset"),
        );
        prefix[header + 42..header + 46].copy_from_slice(&(local_offset + 5).to_le_bytes());
    }
    let end = original_end + 5;
    let directory_offset = u32::from_le_bytes(
        prefix[end + 16..end + 20]
            .try_into()
            .expect("directory offset"),
    );
    prefix[end + 16..end + 20].copy_from_slice(&(directory_offset + 5).to_le_bytes());
    prefix.extend_from_slice(b"tail");

    let parsed = zip::parse_office(&prefix)
        .expect("parse prefixed ODF")
        .expect("recognized office document");
    assert_eq!(parsed.zip.archive_prefix_hex, encode_hex(b"stub!"));
    assert_eq!(parsed.zip.archive_trailing_hex, encode_hex(b"tail"));

    let mut archive = Archive::new(Manifest::new(Variant::Wal));
    archive
        .add_office("report.odt", &prefix)
        .expect("add prefixed ODF");
    assert_eq!(
        archive
            .extract_office("report.odt", Version::Index(0))
            .expect("extract prefixed ODF"),
        prefix
    );
}

#[test]
fn ignores_non_zip_input_and_rejects_ole2() {
    assert_eq!(detect::detect_office(b"plain text").expect("non-ZIP"), None);
    assert!(matches!(
        detect::detect_office(&[0xD0, 0xCF, 0x11, 0xE0]),
        Err(WalrusError::UnsupportedOfficeFormat { .. })
    ));
}

#[test]
fn rejects_zip_without_an_office_signature() {
    let bytes = stored_zip(&[("data.txt", b"not an office file")]);

    assert!(matches!(
        detect::detect_office(&bytes),
        Err(WalrusError::UnrecognizedOfficeZip)
    ));
}

#[test]
fn rejects_declared_uncompressed_size_over_the_limit() {
    let actual = checked_uncompressed_size(5, 6, 10)
        .expect_err("declared sizes above the limit must be rejected");

    assert!(matches!(
        actual,
        WalrusError::OfficeUncompressedSizeLimit {
            actual: 11,
            limit: 10
        }
    ));
}

#[test]
fn rejects_a_zip_whose_declared_total_size_exceeds_one_gibibyte() {
    let mut bytes = stored_zip(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<document/>"),
    ]);
    let central_header = bytes
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == b"PK\x01\x02")
        .nth(1)
        .map(|(offset, _)| offset)
        .expect("central directory header");
    bytes[central_header + 24..central_header + 28].copy_from_slice(&u32::MAX.to_le_bytes());

    assert!(matches!(
        zip::parse_office(&bytes),
        Err(WalrusError::OfficeUncompressedSizeLimit { .. })
    ));
}

#[test]
fn reports_malformed_zip_with_a_typed_error() {
    assert!(matches!(
        detect::detect_office(b"PK\x03\x04truncated"),
        Err(WalrusError::OfficeZip { .. })
    ));
}

fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut central = Vec::new();

    for (name, content) in files {
        let name_bytes = name.as_bytes();
        let offset = bytes.len() as u32;
        let checksum = crc32(content);
        let size = content.len() as u32;

        bytes.extend_from_slice(b"PK\x03\x04");
        push_u16(&mut bytes, 10);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, checksum);
        push_u32(&mut bytes, size);
        push_u32(&mut bytes, size);
        push_u16(&mut bytes, name_bytes.len() as u16);
        push_u16(&mut bytes, 0);
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(content);

        central.extend_from_slice(b"PK\x01\x02");
        push_u16(&mut central, 20);
        push_u16(&mut central, 10);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, checksum);
        push_u32(&mut central, size);
        push_u32(&mut central, size);
        push_u16(&mut central, name_bytes.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, offset);
        central.extend_from_slice(name_bytes);
    }

    let central_offset = bytes.len() as u32;
    bytes.extend_from_slice(&central);
    bytes.extend_from_slice(b"PK\x05\x06");
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, files.len() as u16);
    push_u16(&mut bytes, files.len() as u16);
    push_u32(&mut bytes, central.len() as u32);
    push_u32(&mut bytes, central_offset);
    push_u16(&mut bytes, 0);
    bytes
}

fn stored_zip_with_descriptor() -> Vec<u8> {
    let mimetype = b"application/vnd.oasis.opendocument.text";
    let content = b"<document/>";
    let mut bytes = stored_zip(&[("mimetype", mimetype), ("content.xml", content)]);
    let local_start = 30 + "mimetype".len() + mimetype.len();
    let data_start = local_start + 30 + "content.xml".len();
    let data_end = data_start + content.len();
    bytes[local_start + 6..local_start + 8].copy_from_slice(&8u16.to_le_bytes());
    bytes[local_start + 14..local_start + 26].fill(0);

    let mut descriptor = b"PK\x07\x08".to_vec();
    descriptor.extend_from_slice(&crc32(content).to_le_bytes());
    descriptor.extend_from_slice(&(content.len() as u32).to_le_bytes());
    descriptor.extend_from_slice(&(content.len() as u32).to_le_bytes());
    bytes.splice(data_end..data_end, descriptor);

    let central_start = bytes
        .windows(4)
        .position(|window| window == b"PK\x01\x02")
        .expect("central directory");
    let second_central = central_start + 46 + "mimetype".len();
    bytes[second_central + 8..second_central + 10].copy_from_slice(&8u16.to_le_bytes());
    let end = bytes
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("end record");
    let directory_offset = u32::from_le_bytes(
        bytes[end + 16..end + 20]
            .try_into()
            .expect("directory offset"),
    );
    bytes[end + 16..end + 20].copy_from_slice(&(directory_offset + 16).to_le_bytes());
    bytes
}

fn deflated_zip(files: &[(&str, &[u8])], comment: &[u8]) -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    archive.set_raw_comment(comment.to_vec().into_boxed_slice());
    let options =
        SimpleFileOptions::default().compression_method(::zip::CompressionMethod::Deflated);
    for (path, content) in files {
        archive.start_file(path, options).expect("start ZIP member");
        archive.write_all(content).expect("write ZIP member");
    }
    archive.finish().expect("finish ZIP").into_inner()
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
