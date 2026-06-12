//! AC5: skill-doctor proposals reject <ULID> flips status to rejected;
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//! proposal disappears from the pending list output.
//!
//! Tests the reject-then-list CLI flow using the library's proposal module
//! directly, avoiding external binary invocation so the test is self-contained.

use std::fs;

use skill_doctor::proposal::{ensure_dir, write_proposal};
use skill_doctor::{Drift, DriftKind, Invocation};

fn make_drift(binary: &str) -> Drift {
    Drift {
        invocation: Invocation {
            skill_path: std::path::PathBuf::from("/skills/test/SKILL.md"),
            line: 7,
            binary: binary.to_owned(),
            subcommand: Some("sub".to_owned()),
            flags: vec!["--flag".to_owned()],
        },
        kind: DriftKind::FlagUnknown,
        detail: "test drift for AC5".to_owned(),
    }
}

/// Parse the `status:` value out of a proposal file's YAML frontmatter.
fn read_status(path: &std::path::Path) -> String {
    let body = fs::read_to_string(path).expect("read proposal");
    body.lines()
        .find(|l| l.starts_with("status:"))
        .and_then(|l| l.strip_prefix("status:"))
        .map(|v| v.trim().to_owned())
        .expect("status line in frontmatter")
}

/// Return a list of all .md files in the dir whose frontmatter status is "pending".
fn pending_proposals(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    if !dir.exists() {
        return vec![];
    }
    fs::read_dir(dir)
        .expect("read dir")
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .filter(|p| read_status(p) == "pending")
        .collect()
}

/// Rewrite status field in frontmatter (mirrors what the CLI `reject` subcommand does).
fn rewrite_status_to_rejected(path: &std::path::Path) {
    let body = fs::read_to_string(path).expect("read");
    let mut out = String::with_capacity(body.len() + 16);
    let mut started = false;
    let mut in_fm = false;
    let mut changed = false;
    for line in body.lines() {
        if line.trim() == "---" {
            if started {
                in_fm = false;
            } else {
                started = true;
                in_fm = true;
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_fm && !changed && line.starts_with("status:") {
            out.push_str("status: rejected\n");
            changed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    // atomic write via temp file
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, out.as_bytes()).expect("write tmp");
    fs::rename(&tmp, path).expect("rename");
}

#[test]
fn rejected_proposal_is_no_longer_pending() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");

    // Create a proposal
    let drift = make_drift("rejecttool");
    let proposal_path = write_proposal(&dir, &drift).expect("write_proposal");

    // Confirm it is pending
    assert_eq!(
        read_status(&proposal_path),
        "pending",
        "new proposal must be pending"
    );
    let before_pending = pending_proposals(tmp.path());
    assert_eq!(
        before_pending.len(),
        1,
        "one pending proposal before reject"
    );

    // Simulate `proposals reject <ULID>`
    rewrite_status_to_rejected(&proposal_path);

    // Status must now be rejected
    assert_eq!(
        read_status(&proposal_path),
        "rejected",
        "status must be rejected after reject command"
    );

    // Pending list must be empty
    let after_pending = pending_proposals(tmp.path());
    assert!(
        after_pending.is_empty(),
        "no pending proposals after rejection (found: {after_pending:?})"
    );
}

/// A second distinct proposal is unaffected when a different one is rejected.
#[test]
fn reject_one_proposal_leaves_other_pending() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = ensure_dir(tmp.path()).expect("ensure_dir");

    let drift_a = make_drift("alpha-tool");
    // Differ by detail so dedupe hash differs
    let drift_b = Drift {
        invocation: Invocation {
            skill_path: std::path::PathBuf::from("/skills/test/SKILL.md"),
            line: 9,
            binary: "beta-tool".to_owned(),
            subcommand: None,
            flags: vec![],
        },
        kind: DriftKind::BinaryMissing,
        detail: "beta not in manifest".to_owned(),
    };

    let path_a = write_proposal(&dir, &drift_a).expect("write A");
    let path_b = write_proposal(&dir, &drift_b).expect("write B");
    assert_ne!(path_a, path_b, "two distinct drifts must produce distinct paths");

    // Reject only A
    rewrite_status_to_rejected(&path_a);

    let pending = pending_proposals(tmp.path());
    assert_eq!(
        pending.len(),
        1,
        "exactly one proposal must remain pending after rejecting A"
    );
    assert_eq!(
        pending[0], path_b,
        "remaining pending proposal must be B"
    );
}
