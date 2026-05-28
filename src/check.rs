//! Compare invocations against the `tool-manifest` JSON and classify drift.
//!
//! The manifest schema mirrors what `tool-manifest sync` writes to
//! `~/.claude/tool-manifest/manifest.json`. Only the fields skill-doctor
//! actually inspects are deserialized; everything else is ignored so a
//! tool-manifest schema bump that adds fields won't break parsing.

use std::collections::{BTreeMap, BTreeSet};
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
    /// System executables resolvable on `PATH` outside the local-bin dir.
    ///
    /// Not part of the on-disk schema (`#[serde(skip)]`); the caller fills
    /// this in after load via [`system_binaries`]. A binary absent from
    /// `tools` but present here is a stock system tool the manifest never
    /// claims to track, so it is NOT drift — see [`classify`]. Empty by
    /// default, which keeps `BinaryMissing` firing for any unknown binary
    /// (the behaviour unit tests assert directly).
    #[serde(skip)]
    pub system_bins: BTreeSet<String>,
}

/// Collect executable basenames found on `PATH`, excluding the local-bin dir.
///
/// `~/.local/bin` is where this machine's custom tools live, and the
/// `tool-manifest` is meant to cover exactly those — so a local tool missing
/// from the manifest is genuine drift and must NOT be suppressed. Everything
/// else on `PATH` (`/usr/bin`, `/bin`, …) is stock system tooling the manifest
/// makes no claim about; listing those names lets [`classify`] silence the
/// `BinaryMissing` false positives that otherwise flood AC7 (`git`, `sed`,
/// `cargo`, `jq`, …).
#[must_use]
pub fn system_binaries() -> BTreeSet<String> {
    use std::os::unix::fs::PermissionsExt as _;

    let local_bin = std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/bin"));
    let Some(path) = std::env::var_os("PATH") else {
        return BTreeSet::new();
    };

    let mut names = BTreeSet::new();
    for dir in std::env::split_paths(&path) {
        // Skip the local-bin dir: those tools are the manifest's job to track.
        if local_bin.as_deref() == Some(dir.as_path()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            // Regular file (or symlink to one) with any execute bit set.
            if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
                if let Some(name) = entry.file_name().to_str() {
                    names.insert(name.to_owned());
                }
            }
        }
    }
    names
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
        // A binary the manifest doesn't track is only drift if it isn't a
        // stock system tool. `git`/`sed`/`cargo`/… resolve on PATH and are
        // out of the manifest's scope, so flagging them is a false positive
        // (AC7). A name resolvable nowhere — e.g. a stale `~/.local/bin`
        // entry from self-review's bootstrap list — stays `BinaryMissing`.
        if manifest.system_bins.contains(&invocation.binary) {
            return Vec::new();
        }
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
            system_bins: BTreeSet::new(),
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
    fn system_binary_absent_from_manifest_is_not_drift() {
        // `git` isn't in the manifest, but it's a stock system tool, so the
        // caller will have listed it in `system_bins` — no BinaryMissing.
        let mut m = manifest_with(&[]);
        m.system_bins.insert("git".to_owned());
        assert!(classify(&inv("git", Some("status"), &["--short"]), &m).is_empty());
        // A name resolvable nowhere still flags (the bootstrap-symlink case).
        let drifts = classify(&inv("missing-local-tool", None, &[]), &m);
        assert_eq!(drifts.len(), 1);
        assert_eq!(drifts[0].kind, DriftKind::BinaryMissing);
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
