//! Codex-style patch parsing and application.

use crate::{Filesystem, FilesystemError};

/// Parsed Codex-style patch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    operations: Vec<PatchOperation>,
}

/// Summary of paths changed by a patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PatchApplyReport {
    /// Added files.
    pub added: Vec<crate::VirtualPath>,
    /// Updated files.
    pub updated: Vec<crate::VirtualPath>,
    /// Deleted files.
    pub deleted: Vec<crate::VirtualPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PatchOperation {}

pub(crate) async fn apply_patch_using_filesystem<F>(
    _filesystem: &F,
    _patch: &Patch,
) -> Result<PatchApplyReport, FilesystemError>
where
    F: Filesystem + ?Sized,
{
    Ok(PatchApplyReport::default())
}
