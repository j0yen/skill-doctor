//! AC1: cargo build --release green on fresh clone; cargo test --release --lib green.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//!
//! This integration test exercises the public library surface to confirm the
//! crate compiles and the core types are accessible.  The actual "cargo build
//! --release" and "cargo test --lib" gates are enforced by the CI harness that
//! runs all tests; a non-compiling crate would fail this file before reaching
//! the assertions.

use skill_doctor::check::Manifest;
use skill_doctor::{Drift, DriftKind, Invocation};

/// Round-trip an Invocation through serde_json to confirm the public API is
/// reachable and the types implement Serialize + Deserialize.
#[test]
fn invocation_serde_roundtrip() {
    let inv = Invocation {
        skill_path: std::path::PathBuf::from("/tmp/SKILL.md"),
        line: 42,
        binary: "recall".to_owned(),
        subcommand: Some("write".to_owned()),
        flags: vec!["--kind".to_owned(), "--ttl".to_owned()],
    };
    let json = serde_json::to_string(&inv).expect("serialize");
    let back: Invocation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(inv, back);
}

/// Confirm the Drift type and DriftKind variants are accessible and comparable.
#[test]
fn drift_kind_variants_accessible() {
    let inv = Invocation {
        skill_path: std::path::PathBuf::from("/tmp/SKILL.md"),
        line: 1,
        binary: "pevent".to_owned(),
        subcommand: None,
        flags: vec![],
    };
    let drift = Drift {
        invocation: inv,
        kind: DriftKind::BinaryMissing,
        detail: "not in manifest".to_owned(),
    };
    // All four variants must be constructible and equatable.
    assert_eq!(drift.kind, DriftKind::BinaryMissing);
    assert_ne!(drift.kind, DriftKind::FlagUnknown);
    assert_ne!(drift.kind, DriftKind::SubcommandUnknown);
    assert_ne!(drift.kind, DriftKind::SkippedVersionOnly);
}

/// Confirm a default Manifest can be constructed (lib API stability).
#[test]
fn manifest_default_is_empty() {
    let m: Manifest = Manifest::default();
    assert!(m.tools.is_empty());
    assert!(m.system_bins.is_empty());
}
