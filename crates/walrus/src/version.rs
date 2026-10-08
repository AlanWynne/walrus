//! Version addressing: resolve a [`Version`] selector to a concrete index.

use crate::manifest::Manifest;
use crate::{Result, WalrusError};

/// A way of addressing one version within a Walrus file.
///
/// All selectors resolve to an integer index in `0..=len`, where `0` is the
/// stored base and `len` is the far end of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    /// An explicit index (canonical form).
    Index(usize),
    /// The stored base (index 0).
    Base,
    /// The far end of the log (index == number of records).
    Head,
    /// A named checkpoint from the manifest.
    Label(String),
    /// `n` steps forward from the base (clamped at head).
    ForwardFromBase(usize),
    /// `n` steps back from the head (clamped at base).
    BackFromHead(usize),
}

/// Resolve a selector against the manifest and log length.
pub(crate) fn resolve(manifest: &Manifest, len: usize, version: Version) -> Result<usize> {
    let index = match version {
        Version::Index(i) => i,
        Version::Base => 0,
        Version::Head => len,
        Version::ForwardFromBase(n) => n.min(len),
        Version::BackFromHead(n) => len.saturating_sub(n),
        Version::Label(label) => manifest
            .checkpoints
            .iter()
            .find(|c| c.label == label)
            .map(|c| c.index)
            .ok_or(WalrusError::UnknownCheckpoint(label))?,
    };

    if index > len {
        return Err(WalrusError::VersionRange {
            requested: index,
            max: len,
        });
    }
    Ok(index)
}

// TODO: timestamp resolution (`--at T` -> largest index whose record ts <= T)
// once timestamps are populated by writers. Tracked in FORMAT.md section 5.

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_with_checkpoints() -> Manifest {
        let mut m = Manifest::new_wal(10);
        m.checkpoints = vec![crate::manifest::Checkpoint {
            index: 4,
            label: "milestone".to_string(),
        }];
        m
    }

    #[test]
    fn base_and_head_resolve_to_endpoints() {
        let m = Manifest::new_wal(10);
        assert_eq!(resolve(&m, 10, Version::Base).unwrap(), 0);
        assert_eq!(resolve(&m, 10, Version::Head).unwrap(), 10);
    }

    #[test]
    fn relative_selectors_clamp() {
        let m = Manifest::new_wal(10);
        assert_eq!(resolve(&m, 10, Version::ForwardFromBase(99)).unwrap(), 10);
        assert_eq!(resolve(&m, 10, Version::BackFromHead(99)).unwrap(), 0);
    }

    #[test]
    fn label_resolves_and_rejects_unknown() {
        let m = manifest_with_checkpoints();
        assert_eq!(
            resolve(&m, 10, Version::Label("milestone".into())).unwrap(),
            4
        );
        assert!(resolve(&m, 10, Version::Label("nope".into())).is_err());
    }

    #[test]
    fn out_of_range_index_errors() {
        let m = Manifest::new_wal(3);
        assert!(resolve(&m, 3, Version::Index(4)).is_err());
    }
}
