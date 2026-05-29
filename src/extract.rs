//! Walk skill markdown files and extract shell-shaped invocations.
//!
//! Two extraction surfaces: fenced ``` ``` blocks (`bash` / `sh` / `shell` /
//! `zsh`) and inline backtick code in prose. Each candidate is split into
//! [`Invocation`] records using the convention `binary [subcommand]
//! [flags-and-args...]`. Single-line invocations only — heredocs and
//! backslash-continued lines are out of scope for v0.1 per PRD-skill-doctor.

use std::iter::Peekable;
use std::path::{Path, PathBuf};
use std::str::SplitWhitespace;

use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use walkdir::WalkDir;

use crate::Invocation;

const SHELL_LANGS: &[&str] = &["bash", "sh", "shell", "zsh"];

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
/// Walks the markdown AST and emits one [`Invocation`] per command-shaped
/// line found in fenced shell blocks or inline `` `code` `` spans. Returns
/// candidates only — manifest-aware drift classification happens in
/// [`crate::check`].
#[must_use]
pub fn extract_from(skill_path: &Path, body: &str) -> Vec<Invocation> {
    let line_starts = compute_line_starts(body);
    let mut out = Vec::new();
    let mut in_shell_fence = false;

    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => {
                in_shell_fence = SHELL_LANGS
                    .iter()
                    .any(|l| lang.as_ref().eq_ignore_ascii_case(l));
            }
            Event::End(TagEnd::CodeBlock) => {
                in_shell_fence = false;
            }
            Event::Text(text) if in_shell_fence => {
                let mut local_offset = 0usize;
                for raw_line in text.split_inclusive('\n') {
                    let doc_offset = range.start + local_offset;
                    let line_no = offset_to_line(&line_starts, doc_offset);
                    if let Some(inv) = parse_invocation(skill_path, line_no, raw_line) {
                        out.push(inv);
                    }
                    local_offset += raw_line.len();
                }
            }
            Event::Code(text) => {
                let line_no = offset_to_line(&line_starts, range.start);
                if let Some(inv) = parse_invocation(skill_path, line_no, text.as_ref()) {
                    // Inline prose backticks are overwhelmingly names, not
                    // commands. Only keep command-shaped spans — those with a
                    // subcommand or a flag. A bare `tool` mention isn't a
                    // verifiable invocation and was the dominant AC7
                    // false-positive source (88% BinaryMissing over-harvest).
                    // Fenced shell blocks keep emitting bare-binary lines.
                    if inv.subcommand.is_some() || !inv.flags.is_empty() {
                        out.push(inv);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn compute_line_starts(s: &str) -> Vec<usize> {
    let mut v = Vec::with_capacity(s.len() / 40 + 1);
    v.push(0);
    for (i, b) in s.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

fn offset_to_line(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(i) => i + 1,
        Err(i) => i,
    }
}

fn parse_invocation(skill_path: &Path, line: usize, raw_line: &str) -> Option<Invocation> {
    let trimmed = raw_line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let mut tokens = trimmed.split_whitespace().peekable();
    skip_shell_prefixes(&mut tokens);

    let first = tokens.next()?;
    let binary = extract_binary_name(first)?;
    if !is_valid_binary_name(&binary) {
        return None;
    }

    let mut subcommand = None;
    if let Some(&peek) = tokens.peek() {
        if !peek.starts_with('-') && is_valid_subcommand(peek) {
            subcommand = Some(peek.to_owned());
            tokens.next();
        }
    }

    let mut flags = Vec::new();
    for tok in tokens {
        if let Some(flag) = normalize_flag(tok) {
            flags.push(flag);
        }
    }

    Some(Invocation {
        skill_path: skill_path.to_path_buf(),
        line,
        binary,
        subcommand,
        flags,
    })
}

fn skip_shell_prefixes(tokens: &mut Peekable<SplitWhitespace<'_>>) {
    loop {
        let Some(&peek) = tokens.peek() else { return };
        if matches!(peek, "sudo" | "time" | "nice" | "taskset" | "exec" | "command") {
            tokens.next();
            continue;
        }
        if peek == "env" {
            tokens.next();
            while let Some(&t) = tokens.peek() {
                if looks_like_var_assign(t) {
                    tokens.next();
                } else {
                    break;
                }
            }
            continue;
        }
        if looks_like_var_assign(peek) {
            tokens.next();
            continue;
        }
        return;
    }
}

fn looks_like_var_assign(tok: &str) -> bool {
    let Some(eq) = tok.find('=') else { return false };
    let name = &tok[..eq];
    if name.is_empty() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
}

fn extract_binary_name(token: &str) -> Option<String> {
    let last = token.rsplit('/').next()?;
    if last.is_empty() {
        None
    } else {
        Some(last.to_owned())
    }
}

// Shell keywords + common builtins that look like binaries to a naive
// tokenizer but aren't drift-checkable. Skipping them keeps the
// false-positive rate down (PRD AC7: <=30% FP).
const SHELL_NOISE: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "in", "while", "until", "do", "done", "case",
    "esac", "echo", "printf", "cd", "pwd", "exit", "return", "set", "unset", "export", "source",
    "alias", "unalias", "function", "let", "local", "test", "true", "false", "trap", "read",
    "shift", "eval", "wait", "kill", "break", "continue", "declare", "readonly", "type", "which",
    "where", "command", "builtin", "help", "history",
];

fn is_valid_binary_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if SHELL_NOISE.contains(&name) {
        return false;
    }
    let first = name.chars().next();
    if !first.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    // Real binary names use lowercase letters, digits, and dashes. Reject the
    // three dominant inline-prose false-positive shapes (PRD AC7, iter-12):
    //   * dotted     -> filenames (`metrics.json`, `notes.md`) & dotted ids
    //   * underscore -> snake_case identifiers / JSON keys (`build_auto`)
    //   * ALLCAPS    -> placeholders (`ULID`, `RUST_LOG`)
    // None of the ~/.local/bin toolkit (recall, ctrace, wm-push, txn-edit, …)
    // or stock PATH tools (git, jq, sed) use these shapes, so this costs no
    // real positive while killing the BinaryMissing over-harvest.
    if name.contains('.') || name.contains('_') {
        return false;
    }
    let has_letter = name.chars().any(|c| c.is_ascii_alphabetic());
    if has_letter
        && name
            .chars()
            .filter(char::is_ascii_alphabetic)
            .all(|c| c.is_ascii_uppercase())
    {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_valid_subcommand(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if !token
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return false;
    }
    token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn normalize_flag(token: &str) -> Option<String> {
    if !token.starts_with('-') || token == "-" || token == "--" {
        return None;
    }
    let flag = token.split('=').next()?;
    if flag.starts_with("--") && flag.len() < 3 {
        return None;
    }
    let body = flag.trim_start_matches('-');
    if body.is_empty()
        || !body.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    Some(flag.to_owned())
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::missing_panics_doc)]
mod tests {
    use super::*;
    use std::path::Path;

    fn p() -> &'static Path {
        Path::new("/tmp/SKILL.md")
    }

    #[test]
    fn fenced_bash_block_yields_invocation() {
        let md = "Intro prose.\n\n```bash\npevent gc --older-than 7d --dry-run\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        let inv = &out[0];
        assert_eq!(inv.binary, "pevent");
        assert_eq!(inv.subcommand.as_deref(), Some("gc"));
        assert_eq!(inv.flags, vec!["--older-than", "--dry-run"]);
        assert_eq!(inv.line, 4);
    }

    #[test]
    fn unfenced_text_is_ignored() {
        let md = "Just text mentioning pevent gc --dry-run inline without backticks.\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn inline_backtick_is_captured() {
        let md = "Run `bpolicy status --format=json` to check.\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "bpolicy");
        assert_eq!(out[0].subcommand.as_deref(), Some("status"));
        assert_eq!(out[0].flags, vec!["--format"]);
    }

    #[test]
    fn path_prefix_is_stripped() {
        let md = "```sh\n~/.local/bin/ctrace ls --since 1h\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "ctrace");
        assert_eq!(out[0].subcommand.as_deref(), Some("ls"));
    }

    #[test]
    fn env_prefix_is_stripped() {
        let md = "```bash\nenv RUST_LOG=debug FOO=bar recall write --kind project\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "recall");
        assert_eq!(out[0].subcommand.as_deref(), Some("write"));
        assert_eq!(out[0].flags, vec!["--kind"]);
    }

    #[test]
    fn bare_var_assign_prefix_is_stripped() {
        let md = "```sh\nRUST_LOG=info my-tool serve --port 8080\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "my-tool");
        assert_eq!(out[0].subcommand.as_deref(), Some("serve"));
    }

    #[test]
    fn sudo_prefix_is_stripped() {
        let md = "```bash\nsudo systemctl restart foo\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "systemctl");
        assert_eq!(out[0].subcommand.as_deref(), Some("restart"));
    }

    #[test]
    fn comment_lines_skipped() {
        let md = "```bash\n# this is a comment\npevent ls\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "pevent");
        assert_eq!(out[0].subcommand.as_deref(), Some("ls"));
    }

    #[test]
    fn shell_keyword_lines_skipped() {
        let md = "```bash\nif true; then echo hi; fi\nfor x in 1 2; do echo $x; done\n```\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn empty_inline_code_skipped() {
        let md = "Try `` and `--` and `if`.\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn non_shell_fence_skipped() {
        let md = "```rust\nfn main() { pevent::gc(); }\n```\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn flag_with_equals_normalizes() {
        let md = "```sh\nmy-tool sub --flag=val --other=42\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].flags, vec!["--flag", "--other"]);
    }

    #[test]
    fn multiple_fenced_lines_each_yield() {
        let md = "```bash\npevent ls\npevent gc --dry-run\nbpolicy status\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].binary, "pevent");
        assert_eq!(out[0].subcommand.as_deref(), Some("ls"));
        assert_eq!(out[0].line, 2);
        assert_eq!(out[1].binary, "pevent");
        assert_eq!(out[1].subcommand.as_deref(), Some("gc"));
        assert_eq!(out[1].line, 3);
        assert_eq!(out[2].binary, "bpolicy");
        assert_eq!(out[2].line, 4);
    }

    #[test]
    fn short_flag_captured() {
        let md = "```sh\nrg -i pattern src/\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "rg");
        assert_eq!(out[0].flags, vec!["-i"]);
    }

    #[test]
    fn inline_filename_is_rejected() {
        // Filenames in prose backticks were a top AC7 false positive.
        let md = "Receipts land in `metrics.json` and notes go to `notes.md`.\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn inline_allcaps_placeholder_is_rejected() {
        let md = "Promote it with the `ULID` and set `RUST_LOG`.\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn inline_snake_case_identifier_is_rejected() {
        // JSON keys / config identifiers in prose are not invocations.
        let md = "The `build_auto` field and `output_repo_path` are read.\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn inline_bare_tool_mention_is_rejected() {
        // A bare `tool` mention has no subcommand or flag: not a verifiable
        // invocation, even though the binary name is well-formed.
        let md = "We rely on `recall` and `ctrace` throughout.\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn inline_command_shaped_span_still_captured() {
        // Regression guard: a real inline invocation (sub + flag) survives.
        let md = "Run `bpolicy status --format=json` to check.\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "bpolicy");
    }

    #[test]
    fn fenced_bare_binary_still_captured() {
        // The command-shape gate is inline-only; fenced shell lines keep
        // emitting bare-binary invocations.
        let md = "```bash\nrecall\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "recall");
        assert!(out[0].subcommand.is_none());
    }

    #[test]
    fn fenced_identifier_shapes_are_rejected() {
        // Even inside a shell fence, filename/identifier/placeholder shapes
        // are not valid binary names.
        let md = "```bash\nmetrics.json\nbuild_auto\nULID\n```\n";
        assert!(extract_from(p(), md).is_empty());
    }

    #[test]
    fn lone_dash_dash_is_not_a_flag() {
        let md = "```sh\ngit log -- file.txt\n```\n";
        let out = extract_from(p(), md);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].binary, "git");
        assert_eq!(out[0].subcommand.as_deref(), Some("log"));
        assert!(out[0].flags.is_empty());
    }
}
