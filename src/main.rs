//! `skill-doctor` CLI entrypoint.
//!
//! Wires the subcommand surface onto the library:
//! - `check` walks `skills_root`, extracts invocations, classifies them
//!   against the tool-manifest, and writes deduped drift proposals.
//! - `proposals {list,show,reject,promote}` operate on the on-disk queue.
//!
//! No subcommand auto-edits skill files; `promote` only prints a
//! recommended manual edit (mirrors `recall observe`).

#![cfg_attr(not(test), forbid(unsafe_code))]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};

use skill_doctor::DriftKind;
use skill_doctor::check::{classify, load_manifest};
use skill_doctor::extract::{extract_from, skill_files};
use skill_doctor::proposal::{ensure_dir, write_proposal};

#[derive(Debug, Parser)]
#[command(name = "skill-doctor", version, about = "skill-text vs tool-surface drift checker")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Walk skills, classify drift, write new proposals.
    Check {
        /// Root containing skill dirs (default: `~/.claude/skills`).
        #[arg(long)]
        skills_root: Option<PathBuf>,
        /// tool-manifest JSON path (default: `~/.claude/tool-manifest/manifest.json`).
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Proposal output dir (default: `~/.claude/skill-doctor/proposals`).
        #[arg(long)]
        proposals: Option<PathBuf>,
    },
    /// Manage proposals already on disk.
    Proposals {
        /// Proposal dir (default: `~/.claude/skill-doctor/proposals`).
        #[arg(long)]
        dir: Option<PathBuf>,
        #[command(subcommand)]
        cmd: ProposalCmd,
    },
}

#[derive(Debug, Subcommand)]
enum ProposalCmd {
    /// Show pending proposals as a table.
    List,
    /// Print one proposal by ULID.
    Show { id: String },
    /// Mark a proposal `rejected`.
    Reject { id: String },
    /// Print a recommended fix command for a proposal (does not edit files).
    Promote { id: String },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("skill-doctor: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.cmd {
        Cmd::Check { skills_root, manifest, proposals } => {
            let skills_root = match skills_root {
                Some(p) => p,
                None => home()?.join(".claude/skills"),
            };
            let manifest_path = match manifest {
                Some(p) => p,
                None => home()?.join(".claude/tool-manifest/manifest.json"),
            };
            let proposals_dir = match proposals {
                Some(p) => p,
                None => default_proposals_dir()?,
            };
            cmd_check(&skills_root, &manifest_path, &proposals_dir)
        }
        Cmd::Proposals { dir, cmd } => {
            let dir = match dir {
                Some(p) => p,
                None => default_proposals_dir()?,
            };
            match cmd {
                ProposalCmd::List => cmd_list(&dir),
                ProposalCmd::Show { id } => cmd_show(&dir, &id),
                ProposalCmd::Reject { id } => cmd_reject(&dir, &id),
                ProposalCmd::Promote { id } => cmd_promote(&dir, &id),
            }
        }
    }
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME environment variable is not set")
}

fn default_proposals_dir() -> Result<PathBuf> {
    Ok(home()?.join(".claude/skill-doctor/proposals"))
}

/// `skill-doctor check`: walk skills, classify, write deduped proposals.
///
/// Returns exit code 2 (via the friendly-error path) when the manifest is
/// absent, per AC6; otherwise exit 0 after printing the summary line.
fn cmd_check(skills_root: &Path, manifest_path: &Path, proposals_dir: &Path) -> Result<ExitCode> {
    if !manifest_path.exists() {
        eprintln!(
            "skill-doctor: tool-manifest not found at {}.\n\
             Run `tool-manifest sync` to generate it, then re-run `skill-doctor check`.",
            manifest_path.display()
        );
        return Ok(ExitCode::from(2));
    }

    let manifest = load_manifest(manifest_path)?;
    let dir = ensure_dir(proposals_dir)?;
    let before = count_md(&dir)?;

    let mut findings = 0usize;
    for skill in skill_files(skills_root) {
        let Ok(body) = fs::read_to_string(&skill) else {
            continue;
        };
        for inv in extract_from(&skill, &body) {
            for drift in classify(&inv, &manifest) {
                // version-only skips are informational, not actionable drift.
                if drift.kind == DriftKind::SkippedVersionOnly {
                    continue;
                }
                findings += 1;
                write_proposal(&dir, &drift)?;
            }
        }
    }

    let after = count_md(&dir)?;
    let new = after.saturating_sub(before);
    println!("{findings} drift findings, {new} new proposals");
    Ok(ExitCode::SUCCESS)
}

fn count_md(dir: &Path) -> Result<usize> {
    let mut n = 0usize;
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("listing entry in {}", dir.display()))?;
        if entry.path().extension().is_some_and(|e| e == "md") {
            n += 1;
        }
    }
    Ok(n)
}

/// A proposal's frontmatter plus its located drift, for the `list` table.
struct ProposalView {
    id: String,
    kind: String,
    status: String,
    /// `<skill-path>:<line>` parsed from the body's `# Drift ... in ...` header.
    location: String,
}

fn parse_proposal(path: &Path) -> Result<ProposalView> {
    let body = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut id = String::new();
    let mut kind = String::new();
    let mut status = String::new();

    let mut started = false;
    let mut in_frontmatter = false;
    for line in body.lines() {
        if line.trim() == "---" {
            if started {
                in_frontmatter = false;
            } else {
                started = true;
                in_frontmatter = true;
            }
            continue;
        }
        if in_frontmatter {
            if let Some(v) = line.strip_prefix("id:") {
                v.trim().clone_into(&mut id);
            } else if let Some(v) = line.strip_prefix("kind:") {
                v.trim().clone_into(&mut kind);
            } else if let Some(v) = line.strip_prefix("status:") {
                v.trim().clone_into(&mut status);
            }
        }
    }

    let location = body
        .lines()
        .find(|l| l.starts_with("# Drift"))
        .and_then(|l| l.rsplit(" in ").next())
        .unwrap_or("")
        .to_owned();

    Ok(ProposalView { id, kind, status, location })
}

fn cmd_list(dir: &Path) -> Result<ExitCode> {
    println!("ID\tKIND\tLOCATION\tSTATUS");
    if !dir.exists() {
        return Ok(ExitCode::SUCCESS);
    }
    let mut views = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry.with_context(|| format!("listing entry in {}", dir.display()))?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "md") {
            let view = parse_proposal(&path)?;
            if view.status == "pending" {
                views.push(view);
            }
        }
    }
    views.sort_by(|a, b| a.id.cmp(&b.id));
    for v in &views {
        println!("{}\t{}\t{}\t{}", v.id, v.kind, v.location, v.status);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_show(dir: &Path, id: &str) -> Result<ExitCode> {
    let path = dir.join(format!("{id}.md"));
    if !path.exists() {
        eprintln!("skill-doctor: no proposal `{id}` under {}", dir.display());
        return Ok(ExitCode::from(2));
    }
    let body = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    print!("{body}");
    Ok(ExitCode::SUCCESS)
}

fn cmd_reject(dir: &Path, id: &str) -> Result<ExitCode> {
    let path = dir.join(format!("{id}.md"));
    if !path.exists() {
        eprintln!("skill-doctor: no proposal `{id}` under {}", dir.display());
        return Ok(ExitCode::from(2));
    }
    rewrite_status(&path, "rejected")?;
    println!("proposal {id}: status -> rejected");
    Ok(ExitCode::SUCCESS)
}

fn cmd_promote(dir: &Path, id: &str) -> Result<ExitCode> {
    let path = dir.join(format!("{id}.md"));
    if !path.exists() {
        eprintln!("skill-doctor: no proposal `{id}` under {}", dir.display());
        return Ok(ExitCode::from(2));
    }
    let view = parse_proposal(&path)?;
    println!("# Recommended manual fix for proposal {id} ({})", view.kind);
    println!("# skill-doctor never auto-edits — mirrors `recall observe`.");
    if let Some((skill, line)) = view.location.rsplit_once(':') {
        println!("# Drift located at: {skill}:{line}");
        println!("$EDITOR +{line} {skill}");
    } else {
        println!("# Drift location: {}", view.location);
    }
    println!("# Then re-run `skill-doctor check` to confirm the drift is resolved.");
    rewrite_status(&path, "promoted")?;
    println!("proposal {id}: status -> promoted");
    Ok(ExitCode::SUCCESS)
}

/// Rewrite a proposal's frontmatter `status:` line atomically.
fn rewrite_status(path: &Path, new_status: &str) -> Result<()> {
    let body = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = String::with_capacity(body.len() + 16);
    let mut started = false;
    let mut in_frontmatter = false;
    let mut changed = false;
    for line in body.lines() {
        if line.trim() == "---" {
            if started {
                in_frontmatter = false;
            } else {
                started = true;
                in_frontmatter = true;
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if in_frontmatter && !changed && line.starts_with("status:") {
            out.push_str("status: ");
            out.push_str(new_status);
            out.push('\n');
            changed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    let Some(dir) = path.parent() else {
        anyhow::bail!("proposal path {} has no parent dir", path.display());
    };
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("proposal path has no UTF-8 file name")?;
    let tmp = dir.join(format!(".{file_name}.status.tmp"));
    fs::write(&tmp, out.as_bytes()).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}
