//! Compare invocations against the `tool-manifest` JSON and classify drift.
//!
//! The manifest schema (per `~/.claude/tool-manifest/manifest.json`) lists
//! each binary's supported subcommands and flags. v0.1 stubs the entrypoint;
//! drift classification lands in iter-5.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::{Drift, Invocation};

/// One binary's surface as recorded in the manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BinaryEntry {
    /// When true, the binary's flag surface is opaque (e.g., `--version`-only); skip flag checks.
    #[serde(default)]
    pub version_only: bool,
    /// Top-level flags accepted without a subcommand.
    #[serde(default)]
    pub flags: Vec<String>,
    /// Subcommand surfaces keyed by subcommand name.
    #[serde(default)]
    pub subcommands: BTreeMap<String, SubcommandEntry>,
}

/// One subcommand's flag set.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubcommandEntry {
    /// Flags accepted under this subcommand.
    #[serde(default)]
    pub flags: Vec<String>,
}

/// The full manifest as parsed from JSON.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// Binary entries keyed by binary name.
    #[serde(default)]
    pub binaries: BTreeMap<String, BinaryEntry>,
}

/// Load and parse the manifest from disk.
///
/// # Errors
/// Returns an error when the file cannot be read or fails JSON parsing.
pub fn load_manifest(path: &Path) -> Result<Manifest> {
    let body =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let m: Manifest = serde_json::from_str(&body)
        .with_context(|| format!("parsing manifest at {}", path.display()))?;
    Ok(m)
}

/// Classify a single invocation against the manifest.
///
/// Returns `None` when no drift is detected. v0.1 returns `None` for all
/// inputs; classification lands in iter-5.
#[allow(clippy::missing_const_for_fn, clippy::must_use_candidate)]
pub fn classify(_invocation: &Invocation, _manifest: &Manifest) -> Option<Drift> {
    None
}
