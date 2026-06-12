//! AC9: Crate published to github.com/j0yen/skill-doctor under dual
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//! MIT+Apache-2.0 license with README citing drift vision and tool-manifest.
//!
//! This test verifies the in-repo structural requirements (license files
//! present, Cargo.toml license field correct, README content) that are
//! necessary pre-conditions for a valid publish.  The GitHub push itself is
//! verified by CI, not here.

use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is set by cargo when running tests.
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
}

/// LICENSE-MIT must be present.
#[test]
fn license_mit_file_exists() {
    let path = repo_root().join("LICENSE-MIT");
    assert!(
        path.exists(),
        "LICENSE-MIT must be present at {}, got: not found",
        path.display()
    );
}

/// LICENSE-APACHE must be present.
#[test]
fn license_apache_file_exists() {
    let path = repo_root().join("LICENSE-APACHE");
    assert!(
        path.exists(),
        "LICENSE-APACHE must be present at {}, got: not found",
        path.display()
    );
}

/// Cargo.toml must declare `license = "MIT OR Apache-2.0"`.
#[test]
fn cargo_toml_declares_dual_license() {
    let path = repo_root().join("Cargo.toml");
    let content = fs::read_to_string(&path).expect("read Cargo.toml");
    assert!(
        content.contains("MIT OR Apache-2.0"),
        "Cargo.toml must declare 'MIT OR Apache-2.0' license, content snippet: {:?}",
        &content[..content.len().min(500)]
    );
}

/// README.md must mention drift and tool-manifest.
#[test]
fn readme_mentions_drift_and_tool_manifest() {
    let path = repo_root().join("README.md");
    let content = fs::read_to_string(&path).expect("read README.md");
    assert!(
        content.to_lowercase().contains("drift"),
        "README must mention drift, got length {} chars",
        content.len()
    );
    assert!(
        content.to_lowercase().contains("tool-manifest"),
        "README must mention tool-manifest"
    );
}
