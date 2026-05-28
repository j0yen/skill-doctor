//! Walk skill markdown files and extract shell-shaped invocations.
//!
//! Two extraction surfaces: fenced ``` ``` blocks (`bash` / `sh` / `shell`)
//! and inline backtick code in prose. Each candidate is split into
//! [`Invocation`] records using the convention `binary [subcommand]
//! [flags-and-args...]`.
//!
//! v0.1 implements walking + types only. Block-level extraction follows
//! in iter-4.

use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::WalkDir;

use crate::Invocation;

/// Walk `skills_root` and return the SKILL.md (case-insensitive) under each
/// immediate subdirectory.
pub fn skill_files(skills_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in WalkDir::new(skills_root)
        .min_depth(2)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name == "skill.md" {
            out.push(entry.into_path());
        }
    }
    out
}

/// Extract invocations from a single skill file's raw text.
///
/// Returns an empty vec in v0.1; the parser lands in iter-4.
#[allow(clippy::missing_const_for_fn, clippy::must_use_candidate)]
pub fn extract_from(_skill_path: &Path, _body: &str) -> Vec<Invocation> {
    Vec::new()
}
