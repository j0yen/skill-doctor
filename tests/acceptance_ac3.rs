//! AC3: Proposals written under the proposals dir with the schema defined in
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//! the PRD proposal output section.
//!
//! Verifies frontmatter structure and body shape of generated .md files.

use std::fs;

use skill_doctor::DriftKind;
use skill_doctor::Invocation;
use skill_doctor::proposal::{ensure_dir, write_proposal};
use skill_doctor::Drift;

fn make_drift(binary: &str, kind: DriftKind, detail: &str) -> Drift {
    Drift {
        invocation: Invocation {
            skill_path: std::path::PathBuf::from("/skills/example/SKILL.md"),
            line: 12,
            binary: binary.to_owned(),
            subcommand: Some("check".to_owned()),
            flags: vec!["--verbose".to_owned()],
        },
        kind,
        detail: detail.to_owned(),
    }
}

/// Proposal file must start with YAML frontmatter delimited by `---` lines.
#[test]
fn proposal_has_yaml_frontmatter() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");
    let drift = make_drift("mytool", DriftKind::FlagUnknown, "flag not in manifest");
    let path = write_proposal(&dir, &drift).expect("write_proposal");
    let body = fs::read_to_string(&path).expect("read");

    // Must begin with `---`
    assert!(body.starts_with("---\n"), "expected frontmatter start marker");

    // Frontmatter must close with a second `---`
    let close_pos = body[4..].find("---").expect("expected frontmatter close");
    let frontmatter = &body[4..4 + close_pos];

    // Required frontmatter keys: id, kind, created, status, content_hash
    assert!(frontmatter.contains("id:"), "missing 'id' key");
    assert!(frontmatter.contains("kind:"), "missing 'kind' key");
    assert!(frontmatter.contains("created:"), "missing 'created' key");
    assert!(frontmatter.contains("status:"), "missing 'status' key");
    assert!(frontmatter.contains("content_hash:"), "missing 'content_hash' key");
}

/// status must be `pending` for a freshly written proposal.
#[test]
fn proposal_status_is_pending_on_creation() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");
    let drift = make_drift("othertool", DriftKind::BinaryMissing, "binary absent");
    let path = write_proposal(&dir, &drift).expect("write_proposal");
    let body = fs::read_to_string(&path).expect("read");
    assert!(body.contains("status: pending"), "expected status: pending");
}

/// Body (after frontmatter) must contain a `# Drift ...` heading and invocation code block.
#[test]
fn proposal_body_has_drift_heading_and_code_block() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");
    let drift = make_drift("atool", DriftKind::SubcommandUnknown, "subcommand not found");
    let path = write_proposal(&dir, &drift).expect("write_proposal");
    let body = fs::read_to_string(&path).expect("read");

    assert!(body.contains("# Drift"), "expected a # Drift heading in body");
    // Code block showing the invocation must be present
    assert!(body.contains("```\n"), "expected a fenced code block");
    // The invocation binary must appear in the code block
    assert!(body.contains("atool"), "expected binary name in body");
}

/// Proposal filename must be `<ULID>.md` (uppercase alphanumeric, 26 chars).
#[test]
fn proposal_filename_is_ulid_md() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");
    let drift = make_drift("bintool", DriftKind::FlagUnknown, "unknown flag");
    let path = write_proposal(&dir, &drift).expect("write_proposal");

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("file stem");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .expect("extension");

    assert_eq!(ext, "md", "proposal must have .md extension");
    // ULIDs are 26 characters of [0-9A-Z]
    assert_eq!(stem.len(), 26, "ULID stem must be 26 chars, got {stem:?}");
    assert!(
        stem.chars().all(|c| c.is_ascii_alphanumeric()),
        "ULID stem must be alphanumeric, got {stem:?}"
    );
}

/// The `id` field in frontmatter must match the filename stem.
#[test]
fn proposal_frontmatter_id_matches_filename() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");
    let drift = make_drift("chktool", DriftKind::FlagUnknown, "extra flag");
    let path = write_proposal(&dir, &drift).expect("write_proposal");

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("stem")
        .to_owned();
    let body = fs::read_to_string(&path).expect("read");

    let id_line = body
        .lines()
        .find(|l| l.starts_with("id:"))
        .expect("id line in frontmatter");
    let id_val = id_line.strip_prefix("id:").unwrap().trim();
    assert_eq!(id_val, stem, "frontmatter id must match filename stem");
}
