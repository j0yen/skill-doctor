//! Write deduped drift proposals under `~/.claude/skill-doctor/proposals/`.
//!
//! Each unique `(skill_path, line, binary, drift_kind, detail)` tuple
//! produces one `<ULID>.md` file. v0.1 ships the dir-creation helper and
//! types; the file writer + content-hash dedupe land in iter-6.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::Drift;

/// Proposal lifecycle states stored in frontmatter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    /// Newly written, not yet reviewed.
    Pending,
    /// User decided the skill text is right and the manifest is wrong.
    Rejected,
    /// User asked for the recommended-fix output; not auto-applied.
    Promoted,
}

/// Frontmatter for a single proposal file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalMeta {
    /// ULID for the proposal; also the basename of the file.
    pub id: String,
    /// String form of the drift kind for human readability.
    pub kind: String,
    /// RFC-3339 UTC timestamp of when the proposal was written.
    pub created: String,
    /// Lifecycle state.
    pub status: ProposalStatus,
}

/// Resolve the proposal directory, creating it `0700` if missing.
///
/// # Errors
/// Returns an error when the directory cannot be created.
pub fn ensure_dir(root: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("creating proposal dir {}", root.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perm = std::fs::metadata(root)
            .with_context(|| format!("statting {}", root.display()))?
            .permissions();
        perm.set_mode(0o700);
        std::fs::set_permissions(root, perm)
            .with_context(|| format!("chmodding {}", root.display()))?;
    }
    Ok(root.to_path_buf())
}

/// Write one proposal file. v0.1 stub; iter-6 lands the body.
///
/// # Errors
/// Returns an error when the proposal file cannot be written.
#[allow(clippy::unnecessary_wraps)]
pub fn write_proposal(_dir: &Path, _drift: &Drift) -> Result<PathBuf> {
    Ok(PathBuf::new())
}
