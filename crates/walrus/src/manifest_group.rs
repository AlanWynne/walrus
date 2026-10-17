//! Optional document-group metadata from FORMAT.md section 8.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::manifest::Manifest;

/// Office container family used to select its ZIP packaging conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfficeKind {
    /// OpenDocument Format ZIP container.
    Odf,
    /// Office Open XML / OPC ZIP container.
    Ooxml,
}

/// One aligned version of a grouped document and its tracked text members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentVersionRow {
    /// Document version index; zero is current and higher values are older.
    pub doc_version: usize,
    /// Version index of the tracked envelope-recipe file.
    pub envelope_index: usize,
    /// Tracked text member id to that member's own version index.
    pub members: BTreeMap<String, usize>,
}

/// Logical office document made up of independently tracked text files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentGroup {
    /// Stable, archive-unique group id.
    pub id: String,
    /// User-visible logical document path.
    pub name: String,
    /// ODF or OOXML packaging convention.
    pub kind: OfficeKind,
    /// Union of tracked text member ids across the group's history.
    pub member_ids: Vec<String>,
    /// Tracked file id containing the versioned envelope recipe.
    pub envelope_id: String,
    /// Alignment rows ordered from current to oldest retained document version.
    pub version_map: Vec<DocumentVersionRow>,
}

impl DocumentGroup {
    /// Resolve a document version index to its alignment row.
    ///
    /// # Errors
    /// Returns [`crate::WalrusError::VersionRange`] when the requested document
    /// version is outside the retained group history.
    pub fn version_row(&self, doc_version: usize) -> crate::Result<&DocumentVersionRow> {
        self.version_map
            .get(doc_version)
            .ok_or_else(|| crate::WalrusError::VersionRange {
                requested: doc_version,
                max: self.version_map.len().saturating_sub(1),
            })
    }
}

pub(crate) fn validate_groups(manifest: &Manifest) -> crate::Result<()> {
    let mut group_ids = HashSet::<String>::new();
    for group in &manifest.groups {
        validate_group(manifest, group, &mut group_ids)?;
    }
    Ok(())
}

fn validate_group(
    manifest: &Manifest,
    group: &DocumentGroup,
    group_ids: &mut HashSet<String>,
) -> crate::Result<()> {
    validate_identity(group, group_ids)?;
    let envelope = manifest
        .file(&group.envelope_id)
        .ok_or_else(|| group_error(&group.id, "envelope_id does not reference a tracked file"))?;
    let member_ids = validate_member_ids(manifest, group)?;
    let seen_members = validate_version_map(manifest, group, envelope, &member_ids)?;
    if seen_members != member_ids {
        return invalid_group(
            &group.id,
            "member_ids contains a file absent from version_map",
        );
    }
    Ok(())
}

fn validate_identity(group: &DocumentGroup, group_ids: &mut HashSet<String>) -> crate::Result<()> {
    if group.id.is_empty() || group.name.is_empty() {
        return invalid_group(&group.id, "id and name must be non-empty");
    }
    if !group_ids.insert(group.id.clone()) {
        return invalid_group(&group.id, "group id is duplicated");
    }
    Ok(())
}

fn validate_member_ids(
    manifest: &Manifest,
    group: &DocumentGroup,
) -> crate::Result<HashSet<String>> {
    let mut member_ids = HashSet::new();
    for member_id in &group.member_ids {
        if member_id == &group.envelope_id || !member_ids.insert(member_id.clone()) {
            return invalid_group(&group.id, "member_ids contains the envelope or a duplicate");
        }
        if manifest.file(member_id).is_none() {
            return invalid_group(&group.id, "member_ids references a missing tracked file");
        }
    }
    Ok(member_ids)
}

fn validate_version_map(
    manifest: &Manifest,
    group: &DocumentGroup,
    envelope: &crate::TrackedFile,
    member_ids: &HashSet<String>,
) -> crate::Result<HashSet<String>> {
    if group.version_map.is_empty() {
        return invalid_group(&group.id, "version_map must include document version zero");
    }

    let mut seen_members = HashSet::new();
    for (expected_version, row) in group.version_map.iter().enumerate() {
        validate_version_row(manifest, group, envelope, member_ids, expected_version, row)?;
        seen_members.extend(row.members.keys().cloned());
    }
    Ok(seen_members)
}

fn validate_version_row(
    manifest: &Manifest,
    group: &DocumentGroup,
    envelope: &crate::TrackedFile,
    member_ids: &HashSet<String>,
    expected_version: usize,
    row: &DocumentVersionRow,
) -> crate::Result<()> {
    if row.doc_version != expected_version {
        return invalid_group(
            &group.id,
            "version_map document versions must be contiguous",
        );
    }
    if row.envelope_index > envelope.versions.count
        || (expected_version == 0 && row.envelope_index != 0)
    {
        return invalid_group(
            &group.id,
            "envelope_index is outside its valid version range",
        );
    }
    for (member_id, index) in &row.members {
        if !member_ids.contains(member_id) {
            return invalid_group(&group.id, "version_map references an id outside member_ids");
        }
        let member = manifest.file(member_id).ok_or_else(|| {
            group_error(&group.id, "version_map references a missing tracked file")
        })?;
        if *index > member.versions.count || (expected_version == 0 && *index != 0) {
            return invalid_group(&group.id, "member index is outside its valid version range");
        }
    }
    Ok(())
}

fn invalid_group<T>(id: &str, reason: &str) -> crate::Result<T> {
    Err(group_error(id, reason))
}

fn group_error(id: &str, reason: &str) -> crate::WalrusError {
    crate::WalrusError::Manifest(format!("document group '{id}': {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_row_preserves_member_indices_and_membership() {
        let group: DocumentGroup = serde_json::from_value(serde_json::json!({
            "id": "g0",
            "name": "report.odt",
            "kind": "odf",
            "member_ids": ["content", "image"],
            "envelope_id": "envelope",
            "version_map": [
                {
                    "doc_version": 0,
                    "envelope_index": 0,
                    "members": { "content": 0, "image": 0 }
                },
                {
                    "doc_version": 1,
                    "envelope_index": 1,
                    "members": { "content": 2 }
                }
            ]
        }))
        .expect("parse group");

        let old = group
            .version_row(1)
            .expect("resolve older document version");
        assert_eq!(old.envelope_index, 1);
        assert_eq!(old.members.get("content"), Some(&2));
        assert!(!old.members.contains_key("image"));
    }
}
