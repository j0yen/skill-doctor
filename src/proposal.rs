//! Write deduped drift proposals under `~/.claude/skill-doctor/proposals/`.
//!
//! Each unique `(skill_path, line, binary, drift_kind, detail)` tuple
//! produces one `<ULID>.md` file. Re-running `skill-doctor check` does
//! not duplicate proposals — `write_proposal` scans the dir for an
//! existing `content_hash` match and returns that path instead of
//! writing a new file.

use std::fs::{self, File};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use ulid::Ulid;

use crate::{Drift, DriftKind};

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
    /// Hex sha256 of the dedupe tuple; lets re-runs skip already-queued drifts.
    pub content_hash: String,
}

/// Resolve the proposal directory, creating it `0700` if missing.
///
/// # Errors
/// Returns an error when the directory cannot be created.
pub fn ensure_dir(root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(root)
        .with_context(|| format!("creating proposal dir {}", root.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perm = fs::metadata(root)
            .with_context(|| format!("statting {}", root.display()))?
            .permissions();
        perm.set_mode(0o700);
        fs::set_permissions(root, perm)
            .with_context(|| format!("chmodding {}", root.display()))?;
    }
    Ok(root.to_path_buf())
}

/// Stable hex sha256 over the dedupe tuple. Matches AC4's
/// `(skill_path, line, binary, drift_kind, detail)` definition.
#[must_use]
pub fn content_hash(drift: &Drift) -> String {
    let mut h = Sha256::new();
    h.update(drift.invocation.skill_path.as_os_str().as_encoded_bytes());
    h.update(b"\0");
    h.update(drift.invocation.line.to_le_bytes());
    h.update(b"\0");
    h.update(drift.invocation.binary.as_bytes());
    h.update(b"\0");
    h.update(format!("{:?}", drift.kind).as_bytes());
    h.update(b"\0");
    h.update(drift.detail.as_bytes());
    let out = h.finalize();
    let mut hex = String::with_capacity(out.len() * 2);
    for byte in out {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

/// Write one proposal file. If `dir` already contains a `.md` whose
/// frontmatter carries the same `content_hash`, return that existing
/// path instead of writing a new file.
///
/// # Errors
/// Returns an error when the proposal directory cannot be read or the
/// file cannot be written.
pub fn write_proposal(dir: &Path, drift: &Drift) -> Result<PathBuf> {
    let hash = content_hash(drift);

    if let Some(existing) = find_existing(dir, &hash)? {
        return Ok(existing);
    }

    let id = Ulid::new().to_string();
    let created = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .context("formatting RFC-3339 timestamp")?;
    let body = render(drift, &id, &created, &hash);

    let path = dir.join(format!("{id}.md"));
    let tmp = dir.join(format!(".{id}.md.tmp"));
    {
        let mut f = File::create(&tmp)
            .with_context(|| format!("creating tempfile {}", tmp.display()))?;
        f.write_all(body.as_bytes())
            .with_context(|| format!("writing tempfile {}", tmp.display()))?;
        f.sync_all()
            .with_context(|| format!("fsync {}", tmp.display()))?;
    }
    fs::rename(&tmp, &path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(path)
}

fn find_existing(dir: &Path, hash: &str) -> Result<Option<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(anyhow::Error::new(err)
                .context(format!("reading proposal dir {}", dir.display())));
        }
    };
    let needle = format!("content_hash: {hash}");
    for entry in entries {
        let entry = entry.with_context(|| format!("listing entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        let head = read_head(&path, 1024)?;
        if head.lines().any(|l| l.trim() == needle) {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn read_head(path: &Path, max: usize) -> Result<String> {
    use std::io::Read as _;
    let mut f = File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf)
        .with_context(|| format!("reading {}", path.display()))?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn render(drift: &Drift, id: &str, created: &str, hash: &str) -> String {
    let inv = &drift.invocation;
    let kind = format!("{:?}", drift.kind);
    let status_label = match drift.kind {
        DriftKind::BinaryMissing => "missing binary",
        DriftKind::SubcommandUnknown => "unknown subcommand",
        DriftKind::FlagUnknown => "unknown flag",
        DriftKind::SkippedVersionOnly => "version-only tool",
    };
    let mut invocation_line = inv.binary.clone();
    if let Some(sub) = &inv.subcommand {
        invocation_line.push(' ');
        invocation_line.push_str(sub);
    }
    for flag in &inv.flags {
        invocation_line.push(' ');
        invocation_line.push_str(flag);
    }
    let skill = inv.skill_path.display();
    format!(
        "---\n\
         id: {id}\n\
         kind: {kind}\n\
         created: {created}\n\
         status: pending\n\
         content_hash: {hash}\n\
         ---\n\
         \n\
         # Drift ({status_label}) in {skill}:{line}\n\
         \n\
         Invocation:\n\
         \n\
         ```\n\
         {invocation_line}\n\
         ```\n\
         \n\
         {detail}\n",
        id = id,
        kind = kind,
        created = created,
        hash = hash,
        status_label = status_label,
        skill = skill,
        line = inv.line,
        invocation_line = invocation_line,
        detail = drift.detail,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Invocation;

    fn drift(skill: &str, line: usize, binary: &str, detail: &str) -> Drift {
        Drift {
            invocation: Invocation {
                skill_path: PathBuf::from(skill),
                line,
                binary: binary.to_string(),
                subcommand: Some("gc".to_string()),
                flags: vec!["--dry-run".to_string()],
            },
            kind: DriftKind::FlagUnknown,
            detail: detail.to_string(),
        }
    }

    #[test]
    fn content_hash_is_stable_for_same_tuple() {
        let a = drift("skills/self-review/SKILL.md", 74, "pevent", "x");
        let b = drift("skills/self-review/SKILL.md", 74, "pevent", "x");
        assert_eq!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn content_hash_differs_on_any_field_change() {
        let base = drift("skills/a/SKILL.md", 10, "tool", "d");
        let mut other = base.clone();
        other.invocation.line = 11;
        assert_ne!(content_hash(&base), content_hash(&other));

        let mut other = base.clone();
        other.invocation.binary = "tool2".to_string();
        assert_ne!(content_hash(&base), content_hash(&other));

        let mut other = base.clone();
        other.detail = "e".to_string();
        assert_ne!(content_hash(&base), content_hash(&other));

        let mut other = base.clone();
        other.kind = DriftKind::BinaryMissing;
        assert_ne!(content_hash(&base), content_hash(&other));
    }

    #[test]
    fn write_proposal_creates_file_with_frontmatter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = ensure_dir(tmp.path()).expect("ensure_dir");
        let d = drift("skills/x/SKILL.md", 5, "pevent", "explanation");
        let path = write_proposal(&dir, &d).expect("write_proposal");

        assert!(path.exists());
        assert_eq!(path.extension().and_then(|s| s.to_str()), Some("md"));
        let body = fs::read_to_string(&path).expect("read body");
        assert!(body.starts_with("---\nid: "));
        assert!(body.contains("kind: FlagUnknown"));
        assert!(body.contains("status: pending"));
        assert!(body.contains("content_hash: "));
        assert!(body.contains("# Drift (unknown flag) in skills/x/SKILL.md:5"));
        assert!(body.contains("pevent gc --dry-run"));
        assert!(body.contains("explanation"));
    }

    #[test]
    fn write_proposal_dedupes_identical_drift() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = ensure_dir(tmp.path()).expect("ensure_dir");
        let d = drift("skills/x/SKILL.md", 5, "pevent", "explanation");
        let first = write_proposal(&dir, &d).expect("first write");
        let second = write_proposal(&dir, &d).expect("second write");
        assert_eq!(first, second, "re-write should return existing path");

        let count = fs::read_dir(&dir)
            .expect("read_dir")
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("md"))
            .count();
        assert_eq!(count, 1, "only one proposal file should exist");
    }

    #[test]
    fn write_proposal_emits_distinct_files_for_distinct_drifts() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = ensure_dir(tmp.path()).expect("ensure_dir");
        let a = drift("skills/x/SKILL.md", 5, "pevent", "a");
        let b = drift("skills/x/SKILL.md", 5, "pevent", "b");
        let p_a = write_proposal(&dir, &a).expect("write a");
        let p_b = write_proposal(&dir, &b).expect("write b");
        assert_ne!(p_a, p_b);
        assert!(p_a.exists() && p_b.exists());
    }

    #[test]
    fn write_proposal_ignores_non_md_files_for_dedupe() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = ensure_dir(tmp.path()).expect("ensure_dir");
        let d = drift("skills/x/SKILL.md", 5, "pevent", "explanation");
        let hash = content_hash(&d);
        fs::write(dir.join("notes.txt"), format!("content_hash: {hash}\n"))
            .expect("write decoy");
        let path = write_proposal(&dir, &d).expect("write_proposal");
        assert!(path.exists());
    }

    #[test]
    fn write_proposal_renders_invocation_without_subcommand() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = ensure_dir(tmp.path()).expect("ensure_dir");
        let d = Drift {
            invocation: Invocation {
                skill_path: PathBuf::from("skills/y/SKILL.md"),
                line: 3,
                binary: "ctrace".to_string(),
                subcommand: None,
                flags: vec!["--help".to_string()],
            },
            kind: DriftKind::BinaryMissing,
            detail: "ctrace not in manifest".to_string(),
        };
        let path = write_proposal(&dir, &d).expect("write_proposal");
        let body = fs::read_to_string(&path).expect("read body");
        assert!(body.contains("# Drift (missing binary) in skills/y/SKILL.md:3"));
        assert!(body.contains("ctrace --help"));
    }
}
