//! AC8: Proposal queue path created with mode 0700.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::indexing_slicing,
    clippy::panic,
)]
//!
//! After the first run of `ensure_dir`, the directory's Unix permissions must
//! have mode 0700 (owner rwx only, no group/other bits).

use std::fs;

#[cfg(unix)]
#[test]
fn proposals_dir_created_with_mode_0700() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().expect("tempdir");
    let proposals_dir = tmp.path().join("proposals");
    assert!(!proposals_dir.exists(), "dir must not pre-exist");

    skill_doctor::proposal::ensure_dir(&proposals_dir).expect("ensure_dir");

    assert!(proposals_dir.exists(), "dir must be created");
    let meta = fs::metadata(&proposals_dir).expect("stat proposals dir");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o700,
        "proposals dir must have mode 0700, got 0o{mode:o}"
    );
}

/// Calling ensure_dir on an existing dir that already has mode 0700 must
/// succeed and leave the mode unchanged.
#[cfg(unix)]
#[test]
fn ensure_dir_idempotent_when_already_0700() {
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().expect("tempdir");
    let proposals_dir = tmp.path().join("proposals");

    // First call creates the dir
    skill_doctor::proposal::ensure_dir(&proposals_dir).expect("first ensure_dir");
    // Second call must not error
    skill_doctor::proposal::ensure_dir(&proposals_dir).expect("second ensure_dir");

    let meta = fs::metadata(&proposals_dir).expect("stat");
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "mode must still be 0700, got 0o{mode:o}");
}
