//! Document-group accessors and version alignment for [`super::Archive`].

use super::Archive;
#[cfg(feature = "office")]
use super::ArchiveFile;
use crate::manifest_group::{DocumentGroup, DocumentVersionRow};
use crate::version;
use crate::{Result, Version, WalrusError};

impl Archive {
    #[cfg(feature = "office")]
    pub(crate) fn transaction<T>(
        &mut self,
        operation: impl FnOnce(&mut Archive) -> Result<T>,
    ) -> Result<T> {
        let mut staged = Archive {
            manifest: self.manifest.clone(),
            files: self
                .files
                .iter()
                .map(|file| ArchiveFile {
                    id: file.id.clone(),
                    base: file.base.clone(),
                    header: file.header.clone(),
                    records: file.records.clone(),
                })
                .collect(),
            blobs: self.blobs.clone(),
        };
        let result = operation(&mut staged)?;
        *self = staged;
        Ok(result)
    }

    /// Iterate the logical document groups in manifest order.
    pub fn groups(&self) -> impl Iterator<Item = &DocumentGroup> {
        self.manifest.groups()
    }

    /// Look up a document group by id or logical name.
    pub fn group(&self, selector: &str) -> Option<&DocumentGroup> {
        self.manifest.group(selector)
    }

    /// Resolve a version selector to a document group's alignment row.
    ///
    /// Index, current, original, and relative selectors address document
    /// versions directly. Labels and timestamps resolve through the group's
    /// envelope file, then map its file index back to a document version.
    ///
    /// # Errors
    /// Returns [`WalrusError::UnknownDocumentGroup`] for an unknown group,
    /// [`WalrusError::UnknownFile`] when the envelope file is unavailable,
    /// the standard version-selection errors, or
    /// [`WalrusError::DocumentGroupVersionNotFound`] when an envelope index
    /// does not correspond to a retained document version.
    pub fn resolve_group_version(
        &self,
        selector: &str,
        requested: Version,
    ) -> Result<&DocumentVersionRow> {
        let group = self
            .group(selector)
            .ok_or_else(|| WalrusError::UnknownDocumentGroup {
                selector: selector.to_string(),
            })?;

        match requested {
            Version::Label(_) | Version::Timestamp(_) => {
                let envelope = self.manifest.file(&group.envelope_id).ok_or_else(|| {
                    WalrusError::UnknownFile {
                        id: group.envelope_id.clone(),
                    }
                })?;
                let archive_file = self
                    .files
                    .iter()
                    .find(|file| file.id == group.envelope_id)
                    .ok_or_else(|| WalrusError::UnknownFile {
                        id: group.envelope_id.clone(),
                    })?;
                let envelope_index = version::resolve_file(
                    envelope,
                    &archive_file.records,
                    envelope.versions.count,
                    requested,
                )?;
                group
                    .version_map
                    .iter()
                    .find(|row| row.envelope_index == envelope_index)
                    .ok_or_else(|| WalrusError::DocumentGroupVersionNotFound {
                        group_id: group.id.clone(),
                        envelope_index,
                    })
            }
            requested => {
                let max = group.version_map.len().saturating_sub(1);
                let doc_version = version::resolve_standalone(max, &[], &[], requested)?;
                group.version_row(doc_version)
            }
        }
    }
}
