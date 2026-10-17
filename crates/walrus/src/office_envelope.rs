//! Deterministic, serde-backed office ZIP envelope metadata.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{OfficeKind, Result, WalrusError};

const ENVELOPE_SCHEMA: &str = "walrus-office-envelope";
const ENVELOPE_SCHEMA_VERSION: &str = "0.1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OfficeEnvelope {
    pub(crate) schema: String,
    pub(crate) schema_version: String,
    pub(crate) kind: OfficeKind,
    pub(crate) zip: ZipEnvelope,
    #[serde(default, flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl OfficeEnvelope {
    pub(crate) fn new(kind: OfficeKind, zip: ZipEnvelope) -> Self {
        Self {
            schema: ENVELOPE_SCHEMA.to_string(),
            schema_version: ENVELOPE_SCHEMA_VERSION.to_string(),
            kind,
            zip,
            extra: BTreeMap::new(),
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.schema != ENVELOPE_SCHEMA || self.schema_version != ENVELOPE_SCHEMA_VERSION {
            return Err(envelope_error("unsupported envelope schema or version"));
        }
        validate_hex(&self.zip.archive_prefix_hex, "archive_prefix_hex")?;
        validate_hex(&self.zip.archive_comment_hex, "archive_comment_hex")?;
        validate_hex(
            &self.zip.central_directory_suffix_hex,
            "central_directory_suffix_hex",
        )?;
        validate_hex(&self.zip.end_records_hex, "end_records_hex")?;
        validate_hex(&self.zip.archive_trailing_hex, "archive_trailing_hex")?;

        for member in &self.zip.members {
            member.validate()?;
        }
        match self.kind {
            OfficeKind::Odf => {
                let first = self
                    .zip
                    .members
                    .first()
                    .ok_or_else(|| envelope_error("ODF ZIP has no members"))?;
                if first.path != "mimetype" || first.method != CompressionMethod::Store {
                    return Err(envelope_error(
                        "ODF mimetype member must be first and stored",
                    ));
                }
            }
            OfficeKind::Ooxml => {
                if !self
                    .zip
                    .members
                    .iter()
                    .any(|member| member.path == "[Content_Types].xml")
                {
                    return Err(envelope_error("OOXML ZIP is missing [Content_Types].xml"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ZipEnvelope {
    pub(crate) archive_prefix_hex: String,
    pub(crate) archive_comment_hex: String,
    pub(crate) central_directory_suffix_hex: String,
    pub(crate) end_records_hex: String,
    pub(crate) archive_trailing_hex: String,
    pub(crate) members: Vec<ZipMember>,
    #[serde(default, flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ZipMember {
    pub(crate) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) member_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) compressed_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) deflate_level: Option<u8>,
    pub(crate) method: CompressionMethod,
    pub(crate) crc32: u32,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
    pub(crate) dos_time: u16,
    pub(crate) dos_date: u16,
    pub(crate) gp_flags: u16,
    pub(crate) version_made_by: u16,
    pub(crate) version_needed: u16,
    pub(crate) external_attrs: u32,
    pub(crate) internal_attrs: u16,
    pub(crate) local_extra_hex: String,
    pub(crate) central_extra_hex: String,
    pub(crate) local_header_hex: String,
    pub(crate) central_header_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) data_descriptor_hex: Option<String>,
    #[serde(default, flatten)]
    pub(crate) extra: BTreeMap<String, Value>,
}

impl ZipMember {
    fn validate(&self) -> Result<()> {
        if self.path.is_empty() {
            return Err(envelope_error("ZIP member path must not be empty"));
        }
        if self.member_id.is_some() == self.content_sha256.is_some() {
            return Err(envelope_error(
                "each ZIP member must have exactly one content reference",
            ));
        }
        if self.member_id.as_deref() == Some("") {
            return Err(envelope_error("member_id must not be empty"));
        }
        if let Some(hash) = &self.content_sha256 {
            validate_sha256(hash, "content_sha256")?;
        }
        if let Some(hash) = &self.compressed_sha256 {
            validate_sha256(hash, "compressed_sha256")?;
        }
        if self.deflate_level.is_some_and(|level| level > 9) {
            return Err(envelope_error("deflate_level must be between 0 and 9"));
        }
        if self.method == CompressionMethod::Deflate
            && self.deflate_level.is_none()
            && self.compressed_sha256.is_none()
        {
            return Err(envelope_error(
                "DEFLATE member requires a level or compressed_sha256",
            ));
        }
        if self.method == CompressionMethod::Store && self.deflate_level.is_some() {
            return Err(envelope_error("STORED member cannot have deflate_level"));
        }
        validate_hex(&self.local_extra_hex, "local_extra_hex")?;
        validate_hex(&self.central_extra_hex, "central_extra_hex")?;
        validate_hex(&self.local_header_hex, "local_header_hex")?;
        validate_hex(&self.central_header_hex, "central_header_hex")?;
        if let Some(descriptor) = &self.data_descriptor_hex {
            validate_hex(descriptor, "data_descriptor_hex")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CompressionMethod {
    Store,
    Deflate,
}

fn validate_sha256(value: &str, field: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(envelope_error(&format!(
            "{field} must be a 64-character hexadecimal SHA-256"
        )));
    }
    Ok(())
}

fn validate_hex(value: &str, field: &str) -> Result<()> {
    if !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(envelope_error(&format!(
            "{field} must be lowercase, even-length hexadecimal"
        )));
    }
    Ok(())
}

fn envelope_error(reason: &str) -> WalrusError {
    WalrusError::OfficeEnvelope {
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ODF_ENVELOPE: &str = r#"{
        "schema": "walrus-office-envelope",
        "schema_version": "0.1",
        "kind": "odf",
        "zip": {
            "archive_prefix_hex": "",
            "archive_comment_hex": "",
            "central_directory_suffix_hex": "",
            "end_records_hex": "",
            "archive_trailing_hex": "",
            "members": [
                {
                    "path": "mimetype",
                    "member_id": "f0",
                    "method": "store",
                    "crc32": 1,
                    "compressed_size": 3,
                    "uncompressed_size": 3,
                    "dos_time": 0,
                    "dos_date": 0,
                    "gp_flags": 0,
                    "version_made_by": 20,
                    "version_needed": 10,
                    "external_attrs": 0,
                    "internal_attrs": 0,
                    "local_extra_hex": "",
                    "central_extra_hex": "",
                    "local_header_hex": "",
                    "central_header_hex": ""
                }
            ]
        }
    }"#;

    #[test]
    fn envelope_serializes_deterministically_and_round_trips_unknown_fields() {
        let input = ODF_ENVELOPE
            .replace(
                "\"kind\": \"odf\",",
                "\"kind\": \"odf\", \"future_root_field\": {\"stable\": true},",
            )
            .replace("\"members\": [", "\"future_zip_field\": 2, \"members\": [")
            .replace(
                "\"path\": \"mimetype\",",
                "\"path\": \"mimetype\", \"future_member_field\": \"kept\",",
            );
        let envelope: OfficeEnvelope = serde_json::from_str(&input).expect("parse envelope");

        let first = serde_json::to_vec(&envelope).expect("serialize envelope");
        let parsed: OfficeEnvelope =
            serde_json::from_slice(&first).expect("parse serialized envelope");
        parsed.validate().expect("validate envelope");
        let second = serde_json::to_vec(&parsed).expect("serialize parsed envelope");

        assert_eq!(first, second);
        assert_eq!(
            parsed.extra["future_root_field"],
            serde_json::json!({"stable": true})
        );
        assert_eq!(parsed.zip.extra["future_zip_field"], 2);
        assert_eq!(parsed.zip.members[0].extra["future_member_field"], "kept");
    }

    #[test]
    fn envelope_rejects_members_with_zero_or_two_content_references() {
        let mut envelope: OfficeEnvelope =
            serde_json::from_str(ODF_ENVELOPE).expect("parse envelope");
        envelope.zip.members[0].member_id = None;
        assert!(
            envelope.validate().is_err(),
            "member requires a content reference"
        );

        envelope.zip.members[0].member_id = Some("f0".to_string());
        envelope.zip.members[0].content_sha256 = Some("a".repeat(64));
        assert!(
            envelope.validate().is_err(),
            "member cannot have two content references"
        );
    }

    #[test]
    fn odf_envelope_requires_mimetype_first_and_stored() {
        let mut envelope: OfficeEnvelope =
            serde_json::from_str(ODF_ENVELOPE).expect("parse envelope");
        envelope.zip.members[0].method = CompressionMethod::Deflate;

        assert!(envelope.validate().is_err());
    }

    #[test]
    fn ooxml_envelope_requires_content_types_member() {
        let input = ODF_ENVELOPE
            .replace("\"kind\": \"odf\"", "\"kind\": \"ooxml\"")
            .replace(
                "\"path\": \"mimetype\"",
                "\"path\": \"[Content_Types].xml\"",
            );
        let envelope: OfficeEnvelope = serde_json::from_str(&input).expect("parse envelope");

        envelope.validate().expect("valid OOXML signature member");

        let input = input.replace("[Content_Types].xml", "other.xml");
        let envelope: OfficeEnvelope = serde_json::from_str(&input).expect("parse envelope");
        assert!(envelope.validate().is_err());
    }

    #[test]
    fn envelope_rejects_uppercase_content_hash() {
        let input = ODF_ENVELOPE
            .replace("\"member_id\": \"f0\",", "")
            .replace(
                "\"method\": \"store\",",
                "\"content_sha256\": \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\",\
                 \"method\": \"store\",",
            );
        let envelope: OfficeEnvelope = serde_json::from_str(&input).expect("parse envelope");

        assert!(envelope.validate().is_err());
    }
}
