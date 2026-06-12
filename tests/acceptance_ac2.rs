//! AC2: skill-doctor check against fixture skills produces drift proposals
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//! matching known drift cases.
//!
//! Uses fixture skill files (not the live ~/.claude/skills/) and a fixture
//! manifest.json to exercise the full extract→classify pipeline.  The four
//! known drift patterns from the PRD motivation are each exercised by one
//! fixture skill.

use std::fs;
use std::path::Path;

/// Drift case 1: skill mentions a subcommand that is not in the manifest.
fn write_skill_unknown_sub(dir: &Path) {
    let skill_dir = dir.join("some-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "# some-skill\n\n\
         Use this to do things.\n\n\
         ```bash\n\
         mytool frobnicate --verbose\n\
         ```\n",
    )
    .unwrap();
}

/// Drift case 2: skill mentions a flag not in the manifest for a known subcommand.
fn write_skill_unknown_flag(dir: &Path) {
    let skill_dir = dir.join("other-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "# other-skill\n\n\
         ```bash\n\
         mytool sync --delete\n\
         ```\n",
    )
    .unwrap();
}

/// Drift case 3: skill mentions a completely missing binary.
fn write_skill_missing_binary(dir: &Path) {
    let skill_dir = dir.join("third-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "# third-skill\n\n\
         ```bash\n\
         phantomtool run --now\n\
         ```\n",
    )
    .unwrap();
}

/// Drift case 4: inline backtick mention of an unknown flag on known binary.
fn write_skill_inline_unknown_flag(dir: &Path) {
    let skill_dir = dir.join("fourth-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "# fourth-skill\n\n\
         Run `mytool sync --purge` to clean up.\n",
    )
    .unwrap();
}

fn write_manifest(path: &Path) {
    // mytool has a "sync" subcommand with "--verbose" flag only.
    // "frobnicate" is not a listed subcommand.
    // "--delete" and "--purge" are not in the sync flag list.
    let json = r#"{
        "schema_version": 1,
        "generated_at": "2026-01-01T00:00:00Z",
        "bin_dirs": ["/tmp/fake-local-bin"],
        "tools": {
            "mytool": {
                "version_only": false,
                "flags": ["--help", "--version"],
                "subcommands": {
                    "sync": {
                        "flags": ["--verbose", "--help"]
                    }
                }
            }
        }
    }"#;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, json).unwrap();
}

#[test]
fn check_against_fixtures_produces_drift_for_four_known_patterns() {
    use skill_doctor::check::{classify, load_manifest, system_binaries};
    use skill_doctor::extract::{extract_from, skill_files};
    use skill_doctor::proposal::{ensure_dir, write_proposal};

    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_root = tmp.path().join("skills");
    let manifest_path = tmp.path().join("manifest").join("manifest.json");
    let proposals_dir = tmp.path().join("proposals");

    fs::create_dir_all(&skills_root).unwrap();
    write_skill_unknown_sub(&skills_root);
    write_skill_unknown_flag(&skills_root);
    write_skill_missing_binary(&skills_root);
    write_skill_inline_unknown_flag(&skills_root);

    write_manifest(&manifest_path);

    let mut manifest = load_manifest(&manifest_path).expect("load manifest");
    manifest.system_bins = system_binaries();
    let dir = ensure_dir(&proposals_dir).expect("ensure_dir");

    let mut findings = 0usize;
    for skill in skill_files(&skills_root) {
        let body = fs::read_to_string(&skill).expect("read skill");
        for inv in extract_from(&skill, &body) {
            for drift in classify(&inv, &manifest) {
                use skill_doctor::DriftKind;
                if drift.kind == DriftKind::SkippedVersionOnly {
                    continue;
                }
                findings += 1;
                write_proposal(&dir, &drift).expect("write proposal");
            }
        }
    }

    // We expect at least 4 drift findings (one per fixture).
    assert!(
        findings >= 4,
        "expected >=4 drift findings from 4 fixture skills, got {findings}"
    );

    // Proposal files should have been written.
    let proposal_count = fs::read_dir(&proposals_dir)
        .expect("read proposals dir")
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .count();
    assert!(
        proposal_count >= 4,
        "expected >=4 proposal files, got {proposal_count}"
    );
}
