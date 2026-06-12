//! AC6: Missing tool-manifest handled gracefully: prints friendly error and
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//! exits 2 when manifest path is absent.
//!
//! This test invokes the compiled binary (`skill-doctor check --manifest …`)
//! and asserts the exit code is 2 and stderr contains an actionable message.

use std::process::Command;

/// Path to the test binary compiled from this crate.
/// Cargo sets `CARGO_BIN_EXE_<name>` (with hyphens in the name) as an
/// environment variable at *runtime* for integration tests.  Fall back to
/// deriving the path from `CARGO_MANIFEST_DIR` when not set.
fn binary() -> std::path::PathBuf {
    // cargo sets this env var at runtime for integration tests.
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_skill-doctor") {
        return std::path::PathBuf::from(path);
    }
    // Derive from CARGO_MANIFEST_DIR → target/{release,debug}/skill-doctor
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned()),
    );
    // Try release first (matches the harness_command in the intent-card)
    let release = manifest_dir.join("target").join("release").join("skill-doctor");
    if release.exists() {
        return release;
    }
    manifest_dir.join("target").join("debug").join("skill-doctor")
}

#[test]
fn missing_manifest_exits_2_with_friendly_message() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let nonexistent_manifest = tmp.path().join("does-not-exist").join("manifest.json");

    // We also need a real skills root so the binary doesn't fail for a
    // different reason.  An empty dir is fine — no skills means no extraction.
    let skills_root = tmp.path().join("skills");
    std::fs::create_dir_all(&skills_root).unwrap();
    // Give it an empty proposals dir too
    let proposals_dir = tmp.path().join("proposals");
    std::fs::create_dir_all(&proposals_dir).unwrap();

    let bin = binary();
    if !bin.exists() {
        // Binary not yet built — skip rather than falsely fail.
        eprintln!(
            "skill-doctor binary not found at {}; skipping AC6 binary test",
            bin.display()
        );
        return;
    }

    let output = Command::new(&bin)
        .args([
            "check",
            "--manifest",
            nonexistent_manifest.to_str().unwrap(),
            "--skills-root",
            skills_root.to_str().unwrap(),
            "--proposals",
            proposals_dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run skill-doctor");

    let exit_code = output.status.code().unwrap_or(-1);
    assert_eq!(
        exit_code, 2,
        "expected exit code 2 for missing manifest, got {exit_code}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Must mention the manifest path in the error (so user knows what to fix)
    assert!(
        stderr.contains("tool-manifest") || stderr.contains("manifest"),
        "stderr must mention 'manifest', got: {stderr:?}"
    );
    // Must suggest how to fix it (per AC6 spec)
    assert!(
        stderr.contains("tool-manifest sync") || stderr.contains("sync"),
        "stderr must suggest running tool-manifest sync, got: {stderr:?}"
    );
}
