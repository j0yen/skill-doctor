//! AC4: Re-running skill-doctor check does not create duplicate proposals for
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//! the same (skill_path, line, binary, drift_kind, detail) tuple.
//!
//! Run the extract→classify→write pipeline twice against the same fixture;
//! assert that the proposal count does not increase on the second run.

use std::fs;
use std::path::Path;

use skill_doctor::check::{classify, load_manifest, system_binaries};
use skill_doctor::extract::{extract_from, skill_files};
use skill_doctor::proposal::{ensure_dir, write_proposal};
use skill_doctor::DriftKind;

fn write_fixture_skill(skills_root: &Path) {
    let skill_dir = skills_root.join("deduped-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        "# deduped-skill\n\n\
         ```bash\n\
         localtool frobnicate --explode\n\
         ```\n",
    )
    .unwrap();
}

fn write_fixture_manifest(path: &Path) {
    // localtool exists in manifest; "frobnicate" is not a listed subcommand.
    let json = r#"{
        "schema_version": 1,
        "generated_at": "2026-01-01T00:00:00Z",
        "bin_dirs": ["/tmp/fake-local-bin"],
        "tools": {
            "localtool": {
                "version_only": false,
                "flags": ["--help"],
                "subcommands": {
                    "run": { "flags": ["--help"] }
                }
            }
        }
    }"#;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, json).unwrap();
}

fn run_check(skills_root: &Path, manifest_path: &Path, proposals_dir: &Path) -> usize {
    let mut manifest = load_manifest(manifest_path).expect("load manifest");
    manifest.system_bins = system_binaries();
    let dir = ensure_dir(proposals_dir).expect("ensure_dir");
    let mut findings = 0usize;
    for skill in skill_files(skills_root) {
        let body = fs::read_to_string(&skill).expect("read skill");
        for inv in extract_from(&skill, &body) {
            for drift in classify(&inv, &manifest) {
                if drift.kind == DriftKind::SkippedVersionOnly {
                    continue;
                }
                findings += 1;
                write_proposal(&dir, &drift).expect("write proposal");
            }
        }
    }
    findings
}

fn count_proposals(proposals_dir: &Path) -> usize {
    fs::read_dir(proposals_dir)
        .expect("read proposals dir")
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .count()
}

#[test]
fn second_check_run_does_not_add_proposals() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let skills_root = tmp.path().join("skills");
    let manifest_path = tmp.path().join("manifest").join("manifest.json");
    let proposals_dir = tmp.path().join("proposals");

    fs::create_dir_all(&skills_root).unwrap();
    write_fixture_skill(&skills_root);
    write_fixture_manifest(&manifest_path);

    // First run
    let first_findings = run_check(&skills_root, &manifest_path, &proposals_dir);
    let first_count = count_proposals(&proposals_dir);
    assert!(
        first_findings > 0,
        "first run must produce at least one finding"
    );

    // Second run — identical inputs
    let second_findings = run_check(&skills_root, &manifest_path, &proposals_dir);
    let second_count = count_proposals(&proposals_dir);

    assert!(
        second_findings > 0,
        "second run must still encounter the same drift"
    );
    assert_eq!(
        first_count, second_count,
        "proposal count must not increase on second identical run (got {first_count} -> {second_count})"
    );
}
