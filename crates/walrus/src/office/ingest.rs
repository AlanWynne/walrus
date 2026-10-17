use std::collections::{BTreeMap, HashMap, HashSet};

use crate::manifest_group::{DocumentGroup, DocumentVersionRow};
use crate::office_envelope::OfficeEnvelope;
use crate::{Archive, FileId, Result, Version, WalrusError};

use super::hash::sha256_hex;
use super::zip::{parse_office, ParsedOffice};
use super::{office_envelope_error, office_zip_error};

impl Archive {
    /// Add or save an ODF or OOXML ZIP document as a logical document group.
    ///
    /// Text members are tracked individually; binary member payloads are
    /// content-addressed. Each distinct input creates one aligned document
    /// version, with no retention trimming.
    ///
    /// # Errors
    /// Returns a typed office error for unsupported or malformed containers,
    /// conflicting tracked paths, or inconsistent document-group metadata.
    pub fn add_office(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        if name.is_empty() {
            return Err(office_envelope_error(
                "document group name must not be empty",
            ));
        }
        let parsed = parse_office(bytes)?
            .ok_or_else(|| office_zip_error("input is not a supported ODF or OOXML ZIP"))?;
        let group = self.group(name).cloned();
        if let Some(group) = &group {
            if group.name != name {
                return Err(office_envelope_error(
                    "document group selector conflicts with an existing group id",
                ));
            }
            if group.kind != parsed.kind {
                return Err(office_envelope_error(
                    "document group kind cannot change between saves",
                ));
            }
            if self.extract_office(name, Version::Index(0))? == bytes {
                return Ok(());
            }
        } else if self.manifest().files.iter().any(|file| file.name == name) {
            return Err(office_envelope_error(
                "document group name conflicts with a tracked file",
            ));
        }

        self.transaction(|archive| ingest_parsed(archive, name, parsed, group))
    }
}

fn ingest_parsed(
    archive: &mut Archive,
    name: &str,
    mut parsed: ParsedOffice,
    existing_group: Option<DocumentGroup>,
) -> Result<()> {
    if parsed.members.len() != parsed.zip.members.len() {
        return Err(office_envelope_error(
            "parsed ZIP member metadata is inconsistent",
        ));
    }
    let (envelope_name, old_envelope_count) = match existing_group.as_ref() {
        Some(group) => {
            let envelope = archive.manifest().file(&group.envelope_id).ok_or_else(|| {
                WalrusError::UnknownFile {
                    id: group.envelope_id.clone(),
                }
            })?;
            (envelope.name.clone(), Some(envelope.versions.count))
        }
        None => {
            let envelope_name = member_name(name, "envelope.json");
            if archive
                .manifest()
                .files
                .iter()
                .any(|file| file.name == envelope_name)
            {
                return Err(office_envelope_error(
                    "office envelope path conflicts with a tracked file",
                ));
            }
            (envelope_name, None)
        }
    };
    let member_ids_by_path = existing_group
        .as_ref()
        .map(|group| existing_member_paths(archive, group))
        .transpose()?
        .unwrap_or_default();
    let (text_ids, changed_ids) = validate_members(archive, name, &parsed, &member_ids_by_path)?;

    let mut group = existing_group.unwrap_or_else(|| DocumentGroup {
        id: next_group_id(archive),
        name: name.to_string(),
        kind: parsed.kind,
        member_ids: Vec::new(),
        envelope_id: String::new(),
        version_map: Vec::new(),
    });
    shift_group_history(&mut group, &changed_ids)?;

    let mut pending_blobs = BTreeMap::new();
    for (index, (member, zip_member)) in parsed
        .members
        .iter()
        .zip(parsed.zip.members.iter_mut())
        .enumerate()
    {
        if is_text(&member.content) {
            let tracked_name = member_name(name, &member.path);
            archive.add(&tracked_name, &member.content, Some(0), None)?;
            let tracked = archive
                .manifest()
                .files
                .iter()
                .find(|file| file.name == tracked_name)
                .ok_or_else(|| office_envelope_error("added text member is not tracked"))?;
            let id = tracked.id.clone();
            if text_ids[index]
                .as_ref()
                .is_some_and(|expected| expected != &id)
            {
                return Err(office_envelope_error(
                    "tracked member identity changed unexpectedly",
                ));
            }
            zip_member.member_id = Some(id);
            zip_member.content_sha256 = None;
            if !group.member_ids.contains(&tracked.id) {
                group.member_ids.push(tracked.id.clone());
            }
        } else {
            let hash = sha256_hex(&member.content);
            zip_member.member_id = None;
            zip_member.content_sha256 = Some(hash.clone());
            insert_blob(&mut pending_blobs, &hash, member.content.clone())?;
        }
        if zip_member.method == crate::office_envelope::CompressionMethod::Deflate
            && zip_member.deflate_level.is_none()
        {
            let compressed_hash = sha256_hex(&member.compressed_content);
            zip_member.compressed_sha256 = Some(compressed_hash.clone());
            insert_blob(
                &mut pending_blobs,
                &compressed_hash,
                member.compressed_content.clone(),
            )?;
        }
    }

    let envelope = OfficeEnvelope::new(parsed.kind, parsed.zip);
    envelope.validate()?;
    let envelope_bytes = serde_json::to_vec(&envelope)?;
    archive.add(&envelope_name, &envelope_bytes, Some(0), None)?;
    let envelope_file = archive
        .manifest()
        .files
        .iter()
        .find(|file| file.name == envelope_name)
        .ok_or_else(|| office_envelope_error("added envelope is not tracked"))?;
    if old_envelope_count
        .is_some_and(|old_count| envelope_file.versions.count != old_count.saturating_add(1))
    {
        return Err(office_envelope_error(
            "new office bytes did not produce a distinct envelope version",
        ));
    }
    group.envelope_id = envelope_file.id.clone();

    let mut row_members = BTreeMap::new();
    for zip_member in &envelope.zip.members {
        if let Some(id) = &zip_member.member_id {
            row_members.insert(id.clone(), 0);
        }
    }
    group.version_map.insert(
        0,
        DocumentVersionRow {
            doc_version: 0,
            envelope_index: 0,
            members: row_members,
        },
    );
    for (version, row) in group.version_map.iter_mut().enumerate() {
        row.doc_version = version;
    }

    for (hash, blob) in pending_blobs {
        let blobs = archive.blobs_mut();
        match blobs.get(&hash) {
            Some(existing) if existing != &blob => {
                return Err(WalrusError::OfficeBlob {
                    hash,
                    reason: "content-address collision".to_string(),
                });
            }
            Some(_) => {}
            None => {
                blobs.insert(hash, blob);
            }
        }
    }

    let groups = &mut archive.manifest_mut().groups;
    if let Some(existing) = groups.iter_mut().find(|item| item.id == group.id) {
        *existing = group;
    } else {
        groups.push(group);
    }
    archive.manifest().validate()?;
    Ok(())
}

fn validate_members(
    archive: &Archive,
    group_name: &str,
    parsed: &ParsedOffice,
    known_paths: &HashMap<String, String>,
) -> Result<(Vec<Option<String>>, HashSet<String>)> {
    let mut paths = HashSet::new();
    let mut text_ids = Vec::with_capacity(parsed.members.len());
    let mut changed_ids = HashSet::new();
    for member in &parsed.members {
        if member.path.is_empty()
            || member.path.contains('\0')
            || member.path == "envelope.json"
            || !paths.insert(member.path.as_str())
        {
            return Err(office_zip_error(
                "empty, reserved, or duplicate ZIP member path",
            ));
        }
        let full_name = member_name(group_name, &member.path);
        let known_id = known_paths.get(&member.path);
        if known_id.is_none()
            && archive
                .manifest()
                .files
                .iter()
                .any(|file| file.name == full_name)
        {
            return Err(office_envelope_error(
                "ZIP member path conflicts with an unrelated tracked file",
            ));
        }

        if is_text(&member.content) {
            if let Some(id) = known_id {
                let previous =
                    archive.reconstruct(&FileId::from(id.as_str()), Version::Index(0))?;
                if previous != member.content {
                    changed_ids.insert(id.clone());
                }
            }
            text_ids.push(known_id.cloned());
        } else {
            text_ids.push(None);
        }
    }
    Ok((text_ids, changed_ids))
}

fn existing_member_paths(
    archive: &Archive,
    group: &DocumentGroup,
) -> Result<HashMap<String, String>> {
    let prefix = format!("{}/", group.name);
    let mut paths = HashMap::new();
    for id in &group.member_ids {
        let tracked = archive
            .manifest()
            .file(id)
            .ok_or_else(|| WalrusError::UnknownFile { id: id.clone() })?;
        let path = tracked
            .name
            .strip_prefix(&prefix)
            .filter(|path| !path.is_empty())
            .ok_or_else(|| office_envelope_error("group member name is outside its group"))?;
        if paths.insert(path.to_string(), id.clone()).is_some() {
            return Err(office_envelope_error(
                "multiple group members have the same ZIP path",
            ));
        }
    }
    Ok(paths)
}

fn shift_group_history(group: &mut DocumentGroup, changed_ids: &HashSet<String>) -> Result<()> {
    for row in &mut group.version_map {
        row.envelope_index = row
            .envelope_index
            .checked_add(1)
            .ok_or_else(|| office_envelope_error("envelope version index overflow"))?;
        for id in changed_ids {
            if let Some(index) = row.members.get_mut(id) {
                *index = index
                    .checked_add(1)
                    .ok_or_else(|| office_envelope_error("member version index overflow"))?;
            }
        }
    }
    Ok(())
}

fn insert_blob(blobs: &mut BTreeMap<String, Vec<u8>>, hash: &str, bytes: Vec<u8>) -> Result<()> {
    match blobs.get(hash) {
        Some(existing) if existing != &bytes => Err(WalrusError::OfficeBlob {
            hash: hash.to_string(),
            reason: "content-address collision".to_string(),
        }),
        Some(_) => Ok(()),
        None => {
            blobs.insert(hash.to_string(), bytes);
            Ok(())
        }
    }
}

fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

fn member_name(group_name: &str, path: &str) -> String {
    format!("{group_name}/{path}")
}

fn next_group_id(archive: &Archive) -> String {
    let mut index = 0usize;
    loop {
        let candidate = format!("g{index}");
        if archive
            .manifest()
            .groups
            .iter()
            .all(|group| group.id != candidate)
        {
            return candidate;
        }
        index = index.saturating_add(1);
    }
}
