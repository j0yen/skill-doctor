//! `skill-doctor` CLI entrypoint.
//!
//! v0.1 wires the subcommand surface — `check`, `proposals
//! {list,show,reject,promote}` — without yet implementing the bodies.
//! Each handler emits a deterministic placeholder and exits 0 so the
//! scaffold can be wired into shell scripts and CI ahead of the iter-4+
//! implementation work.

#![cfg_attr(not(test), forbid(unsafe_code))]
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

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

#[allow(clippy::unnecessary_wraps)]
fn run(cli: Cli) -> Result<ExitCode> {
    match cli.cmd {
        Cmd::Check { .. } => {
            println!("0 drift findings, 0 new proposals");
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Proposals { cmd } => match cmd {
            ProposalCmd::List => {
                println!("ID  KIND  SKILL  LINE  STATUS");
                Ok(ExitCode::SUCCESS)
            }
            ProposalCmd::Show { id } => {
                println!("proposal {id}: not yet implemented");
                Ok(ExitCode::SUCCESS)
            }
            ProposalCmd::Reject { id } => {
                println!("proposal {id}: rejection not yet implemented");
                Ok(ExitCode::SUCCESS)
            }
            ProposalCmd::Promote { id } => {
                println!("proposal {id}: promote-as-recommendation not yet implemented");
                Ok(ExitCode::SUCCESS)
            }
        },
    }
}
