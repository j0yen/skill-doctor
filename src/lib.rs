//! skill-doctor — detect drift between skill text and installed tool surfaces.
//!
//! Public types shared by the `extract`, `check`, and `proposal` modules.
//! See [`crate::extract`] for invocation extraction, [`crate::check`] for
//! manifest-comparison drift detection, and [`crate::proposal`] for the
//! review-queue writer.

#![cfg_attr(not(test), forbid(unsafe_code))]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub mod check;
pub mod extract;
pub mod proposal;

/// A single shell invocation extracted from a skill's markdown body.
///
/// `line` is 1-based, matching what an editor displays. `subcommand` is
/// `None` when the heuristic could not identify one (the invocation may
/// be top-level only).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Invocation {
    /// Path of the skill file the invocation was extracted from.
    pub skill_path: PathBuf,
    /// 1-based line number within `skill_path`.
    pub line: usize,
    /// Binary name (no path prefix).
    pub binary: String,
    /// Subcommand, when the heuristic identified one.
    pub subcommand: Option<String>,
    /// All `--flag` tokens seen on the line, in source order.
    pub flags: Vec<String>,
}

/// Classification of how an invocation diverges from the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DriftKind {
    /// Binary not in manifest.
    BinaryMissing,
    /// Subcommand not in the binary's manifest entry.
    SubcommandUnknown,
    /// Flag not in the resolved scope (subcommand if present, else top-level).
    FlagUnknown,
    /// Binary marked `version_only: true`; flags skipped intentionally.
    SkippedVersionOnly,
}

/// One drift finding: an invocation plus its classification and a free-text detail.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Drift {
    /// The invocation that diverged from the manifest.
    pub invocation: Invocation,
    /// How it diverged.
    pub kind: DriftKind,
    /// Human-readable explanation written into the proposal body.
    pub detail: String,
}
