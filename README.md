# skill-doctor

A Rust CLI that walks `~/.claude/skills/*/SKILL.md`, extracts shell
invocations from fenced bash blocks and backtick inline code,
cross-references each `--flag` and subcommand against the
[`tool-manifest`](https://github.com/j0yen/tool-manifest) JSON, and
parks drift proposals at `~/.claude/skill-doctor/proposals/<ULID>.md`
for the user to review. Same review-gated pattern as `recall observe`
and `recall-doctor-claims`; **no auto-edit**.

## Why this exists

Drift between skill text and installed tooling has been observed
repeatedly on this machine (e.g. a skill citing `pevent gc --dry-run`
when the binary no longer takes that flag). `tool-manifest` produces
the ground-truth manifest of what flags and subcommands each binary
actually supports; `skill-doctor` is the consumer that uses that ground
truth to find drift.

A proposal queue is used instead of auto-edit because edits to skill
prose need human review — false positives are inevitable when parsing
shell out of Markdown. Decoupling detection from application also lets
the same checker feed multiple downstream consumers.

## Usage

```sh
# Detect drift across the live skill set (reads the tool-manifest JSON).
skill-doctor check

# Override the skill root, manifest path, or proposals dir.
skill-doctor check --skills-root ~/.claude/skills \
                   --manifest ~/.claude/tool-manifest/manifest.json \
                   --proposals ~/.claude/skill-doctor/proposals

# Review the queue.
skill-doctor proposals list
skill-doctor proposals show <ULID>
skill-doctor proposals reject <ULID>
skill-doctor proposals promote <ULID>
```

Each proposal is a Markdown file with YAML frontmatter
(`status: pending|rejected|promoted`, a content hash, and the source
location). Re-running `check` is idempotent: pending proposals are
deduped by a content hash over `(skill_path, line, binary, drift_kind,
detail)`, so the queue does not grow on repeated runs. The proposals
directory is created mode `0700`.

`check` requires `~/.claude/tool-manifest/manifest.json`; if it is
absent the command prints a friendly pointer to `tool-manifest sync`
and exits `2`. `skill-doctor` never shells out to other binaries in its
hot path — it reads the manifest JSON directly.

## Install

```sh
git clone https://github.com/j0yen/skill-doctor
cd skill-doctor
cargo build --release
install -Dm755 target/release/skill-doctor ~/.local/bin/skill-doctor
```

Or via the wintermute `bootstrap/install.sh`.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
