//! Compare invocations against the `tool-manifest` JSON and classify drift.
//!
//! The manifest schema mirrors what `tool-manifest sync` writes to
//! `~/.claude/tool-manifest/manifest.json`. Only the fields skill-doctor
//! actually inspects are deserialized; everything else is ignored so a
//! tool-manifest schema bump that adds fields won't break parsing.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::Deserialize;

use crate::{Drift, DriftKind, Invocation};

/// One tool's surface as recorded in the manifest.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolEntry {
    /// When true, the tool's flag surface is opaque (e.g., `--version`-only); flag checks are skipped.
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
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SubcommandEntry {
    /// Flags accepted under this subcommand.
    #[serde(default)]
    pub flags: Vec<String>,
}

/// The full manifest as parsed from JSON.
///
/// The on-disk schema also carries `schema_version`, `generated_at`, and
/// `bin_dirs`. skill-doctor doesn't need them at classify time, so they're
/// dropped by serde rather than carried through.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Manifest {
    /// Tool entries keyed by binary basename.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolEntry>,
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
/// Returns one `Drift` per finding:
/// - `BinaryMissing` once if the binary isn't in the manifest.
/// - `SkippedVersionOnly` once if the binary is marked `version_only`
///   (flag checks are skipped intentionally).
/// - `SubcommandUnknown` once if the invocation names a subcommand
///   the manifest doesn't list (the flag scope is then undefined, so
///   no further flag findings are emitted).
/// - `FlagUnknown` once per flag missing from the resolved scope
///   (subcommand's flag set if a subcommand is present; the tool's
///   top-level flag set otherwise).
#[must_use]
pub fn classify(invocation: &Invocation, manifest: &Manifest) -> Vec<Drift> {
    let Some(tool) = manifest.tools.get(&invocation.binary) else {
        return vec![Drift {
            invocation: invocation.clone(),
            kind: DriftKind::BinaryMissing,
            detail: format!("binary `{}` not in manifest", invocation.binary),
        }];
    };

    if tool.version_only {
        return vec![Drift {
            invocation: invocation.clone(),
            kind: DriftKind::SkippedVersionOnly,
            detail: format!(
                "binary `{}` marked version-only; flag checks skipped",
                invocation.binary
            ),
        }];
    }

    let (scope_flags, scope_label) = match invocation.subcommand.as_deref() {
        Some(sub) => match tool.subcommands.get(sub) {
            Some(entry) => (&entry.flags, format!("`{} {sub}`", invocation.binary)),
            None => {
                return vec![Drift {
                    invocation: invocation.clone(),
                    kind: DriftKind::SubcommandUnknown,
                    detail: format!(
                        "subcommand `{} {sub}` not in manifest",
                        invocation.binary
                    ),
                }];
            }
        },
        None => (&tool.flags, format!("`{}`", invocation.binary)),
    };

    invocation
        .flags
        .iter()
        .filter(|flag| !scope_flags.iter().any(|f| f == *flag))
        .map(|flag| Drift {
            invocation: invocation.clone(),
            kind: DriftKind::FlagUnknown,
            detail: format!("flag `{flag}` not supported by {scope_label}"),
        })
        .collect()
}

#[cfg(test)]
#[allow(
    clippy::indexing_slicing,
    clippy::missing_panics_doc,
    clippy::expect_used
)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn inv(binary: &str, sub: Option<&str>, flags: &[&str]) -> Invocation {
        Invocation {
            skill_path: PathBuf::from("SKILL.md"),
            line: 1,
            binary: binary.to_owned(),
            subcommand: sub.map(str::to_owned),
            flags: flags.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn manifest_with(tools: &[(&str, ToolEntry)]) -> Manifest {
        Manifest {
            tools: tools
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        }
    }

    fn entry(version_only: bool, flags: &[&str], subs: &[(&str, &[&str])]) -> ToolEntry {
        ToolEntry {
            version_only,
            flags: flags.iter().map(|s| (*s).to_owned()).collect(),
            subcommands: subs
                .iter()
                .map(|(name, fs)| {
                    (
                        (*name).to_owned(),
                        SubcommandEntry {
                            flags: fs.iter().map(|s| (*s).to_owned()).collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    #[test]
    fn binary_missing_when_tool_absent() {
        let m = manifest_with(&[]);
        let drifts = classify(&inv("ghost", None, &[]), &m);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::BinaryMissing);
        assert!(drifts[0].detail.contains("ghost"));
    }

    #[test]
    fn version_only_short_circuits_flag_check() {
        let m = manifest_with(&[("opaque", entry(true, &[], &[]))]);
        let drifts = classify(&inv("opaque", None, &["--anything"]), &m);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::SkippedVersionOnly);
    }

    #[test]
    fn subcommand_unknown_when_not_in_manifest() {
        let m = manifest_with(&[("pevent", entry(false, &[], &[("ls", &["--all"])]))]);
        let drifts = classify(&inv("pevent", Some("gc"), &["--dry-run"]), &m);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::SubcommandUnknown);
        assert!(drifts[0].detail.contains("pevent gc"));
    }

    #[test]
    fn flag_unknown_under_subcommand_scope() {
        let m = manifest_with(&[(
            "pevent",
            entry(false, &[], &[("gc", &["--older-than", "--help"])]),
        )]);
        let drifts = classify(&inv("pevent", Some("gc"), &["--older-than", "--dry-run"]), &m);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::FlagUnknown);
        assert!(drifts[0].detail.contains("--dry-run"));
        assert!(drifts[0].detail.contains("pevent gc"));
    }

    #[test]
    fn flag_unknown_under_top_level_scope() {
        let m = manifest_with(&[("bpolicy", entry(false, &["--help", "--version"], &[]))]);
        let drifts = classify(&inv("bpolicy", None, &["--format"]), &m);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::FlagUnknown);
        assert!(drifts[0].detail.contains("`bpolicy`"));
    }

    #[test]
    fn multiple_unknown_flags_yield_separate_drifts() {
        let m = manifest_with(&[("recall", entry(false, &["--help"], &[]))]);
        let drifts = classify(&inv("recall", None, &["--foo", "--bar"]), &m);
        assert_eq!(drifts.len(), 2);
        assert!(drifts.iter().all(|d| d.kind == DriftKind::FlagUnknown));
        let details: Vec<&str> = drifts.iter().map(|d| d.detail.as_str()).collect();
        assert!(details.iter().any(|d| d.contains("--foo")));
        assert!(details.iter().any(|d| d.contains("--bar")));
    }

    #[test]
    fn no_drift_when_all_flags_known() {
        let m = manifest_with(&[(
            "wm-push",
            entry(false, &[], &[("", &[])]),
        )]);
        // Empty subcommand name is contrived; simulate a clean top-level call.
        let m2 = manifest_with(&[("wm-push", entry(false, &["--slug", "--help"], &[]))]);
        let drifts = classify(&inv("wm-push", None, &["--slug", "--help"]), &m2);
        assert!(drifts.is_empty());
        // Sanity check that the contrived `m` above also classifies cleanly
        // when called with no flags / no sub.
        let drifts2 = classify(&inv("wm-push", None, &[]), &m);
        assert!(drifts2.is_empty());
    }

    #[test]
    fn manifest_deserializes_real_tool_manifest_shape() {
        // tool-manifest writes extra top-level fields and per-tool extras;
        // we drop them via #[serde(default)] + ignored unknown fields.
        let json = r#"{
            "schema_version": 1,
            "generated_at": "2026-05-28T15:00:00Z",
            "bin_dirs": ["/home/jsy/.local/bin"],
            "tools": {
                "pevent": {
                    "path": "/home/jsy/.local/bin/pevent",
                    "version": "0.3.1",
                    "version_only": false,
                    "flags": ["--help"],
                    "subcommands": {
                        "gc": { "flags": ["--older-than", "--help"] }
                    }
                }
            }
        }"#;
        let m: Manifest = serde_json::from_str(json).expect("parse");
        assert!(m.tools.contains_key("pevent"));
        let pev = &m.tools["pevent"];
        assert!(!pev.version_only);
        assert!(pev.subcommands.contains_key("gc"));
        assert_eq!(pev.subcommands["gc"].flags.len(), 2);
    }
}
