<img src="brand/readme-header.png" alt="Kule: a control room for AI agents that work on your folders" width="100%">

# Kule

[![ci](https://github.com/kulehq/kule/actions/workflows/ci.yml/badge.svg)](https://github.com/kulehq/kule/actions/workflows/ci.yml)
[![docs](https://img.shields.io/badge/docs-kulehq.github.io%2Fkule-e5482b)](https://kulehq.github.io/kule/)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)
![status](https://img.shields.io/badge/status-preview-orange)

**Kule is a control room for AI agents that work on your folders.** You
point an agent (Claude Code, Codex, Gemini or any agent that speaks
[ACP](https://agentclientprotocol.com)) at a folder. Kule gives it a
copy-on-write copy of that folder inside a sandbox, watches what every
command really changed, and lets nothing reach the real folder until a
policy, or you, says so. Every commit can be undone.

It works on any folder: code, but also notes, documents, photos and data.

📖 **Documentation: [kulehq.github.io/kule](https://kulehq.github.io/kule/)**
· [Türkçe](https://kulehq.github.io/kule/tr/)
· [Install](https://kulehq.github.io/kule/docs/getting-started/install)
· [Quick start](https://kulehq.github.io/kule/docs/getting-started/quick-start)
· [CLI reference](https://kulehq.github.io/kule/docs/cli)

> [!NOTE]
> **Status: preview (`0.2.0-preview.2`).** macOS 13+ and Linux. Windows is
> not supported yet. The latest preview release is
> [0.2.0-preview.2](https://github.com/kulehq/kule/releases/tag/v0.2.0-preview.2); or build it from source (below). Expect changes between previews; see [CHANGELOG.md](CHANGELOG.md).

## Why Kule

AI agents run commands with your permissions, on your real files. Kule puts
a boundary between the agent and everything you care about:

| | What Kule does |
| --- | --- |
| **Your folder** | The agent works in a copy. Its changes reach the folder only through a commit, which keeps a checkpoint so `undo` puts the folder back. |
| **Your computer** | Every command runs in the OS sandbox (Seatbelt on macOS, Landlock and namespaces on Linux). It can write only to the copy; `~/.ssh`, `~/.aws`, your Documents and the projects beside the folder are hidden. |
| **What reaches the folder** | A [Cedar](https://www.cedarpolicy.com) policy judges the *effects* of the work, not the command text: ordinary edits pass, manifests, lockfiles, CI files and mass deletes wait for you, git hooks are denied. |
| **Your network** | Every connection goes through the task's proxy. Package registries pass, the agent reaches its own API, anything else waits for your answer. Every connection is logged. |
| **Your secrets** | The agent sees a placeholder. The proxy puts your API key into requests to the hosts you named, and nowhere else. |
| **Your home** | A write to `~/.zshrc`, `~/.gitconfig` or `~/.config` lands in the task's home layer for you to review, not in your home. |
| **Your budget and machine** | Cost budgets per task, per group and per day; CPU, memory, disk, GPU and time limits per run; a Stop button. |

## How it works

```
   your folder ──(copy-on-write)──▶ task copy ◀── agent, in the sandbox
        ▲                               │          (network via the task's proxy)
        │                               ▼
     commit ◀── policy + you ◀── effects: files, git refs, hosts, home writes
        │
      undo  (every commit keeps a checkpoint; the history is signed)
```

The loop is always the same: **make a task → give the agent a prompt →
watch → review → commit or roll back**. Read
[How Kule works](https://kulehq.github.io/kule/docs/concepts/how-kule-works)
for the details.

## Quick start

Build from source (Rust is pinned by `rust-toolchain.toml`; `rustup`
installs it on first build):

```sh
git clone https://github.com/kulehq/kule && cd kule

# macOS: Xcode Command Line Tools. Linux (Debian/Ubuntu):
#   sudo apt-get install -y libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev

cargo build --release -p kule -p kule-desktop
export PATH="$PWD/target/release:$PATH"   # puts `kule` on your PATH for this shell
kule --version
```

Try the loop on a scratch folder, without an agent:

```sh
mkdir -p ~/kule-demo && echo "hello" > ~/kule-demo/notes.md

kule new ~/kule-demo                         # prints the task id, e.g. 1a2b3c4d
kule exec 1a2b3c4d -- sh -c 'echo "more" >> notes.md'   # runs in the copy, in the sandbox
kule status 1a2b3c4d                         # what changed, and the policy's verdict
kule commit 1a2b3c4d                         # apply it to ~/kule-demo, keeping a checkpoint
kule undo 1a2b3c4d                           # put ~/kule-demo back
```

Then with an agent (install and sign in to it as usual first; Claude Code
and Codex need Node.js for `npx`):

```sh
kule run 1a2b3c4d --agent claude-code "Turn notes.md into a checklist"
kule steps 1a2b3c4d      # every tool call the agent made, and what it changed
kule status 1a2b3c4d
kule commit 1a2b3c4d     # or: kule rollback 1a2b3c4d
```

Or start the desktop app, which does all of this with a review screen,
live output and approval cards:

```sh
./target/release/kule-desktop       # or: cargo run -p kule-desktop
```

The full walkthrough is in the
[quick start](https://kulehq.github.io/kule/docs/getting-started/quick-start)
and [your first task in the app](https://kulehq.github.io/kule/docs/getting-started/first-task-in-the-app).

## Commands at a glance

Every command takes `--home <DIR>` (or `KULE_HOME`, default `~/.kule`) and
`--help`. Each one has its own page in the
[CLI reference](https://kulehq.github.io/kule/docs/cli).

| Area | Commands |
| --- | --- |
| Tasks | `new` · `list` · `name` · `color` · `rm` |
| Working in a task | `run` (an agent) · `exec` (a command) · `shell` · `path` · `open` |
| Review and history | `status` · `steps` · `commit` · `rollback` · `undo` · `sync` · `history` |
| Cost and limits | `budget` · `daily-budget` · `limits` |
| Groups | `group list\|new\|rename\|color\|policy\|budget\|put\|delete` |
| Policy and access | `policy` · `rules` · `reveal` · `isolation` |
| Home and secrets | `home` · `secrets` |
| Agents and extensions | `agents` · `ext` |
| Background | `daemon` (the app starts it for you) |

## Install

- **From source:** as in the quick start. Binaries land in
  `target/release/`: `kule` (the CLI) and `kule-desktop` (the app).
- **Preview releases:** the
  [releases page](https://github.com/kulehq/kule/releases) carries a
  universal macOS `.dmg` and Linux `.deb`/`.rpm`, each with `SHA256SUMS`
  and a build provenance attestation. The macOS app is signed ad hoc, not
  notarized; the [install guide](https://kulehq.github.io/kule/docs/getting-started/install)
  says how to check and open them.

## Platform support

| | macOS 13+ | Linux (x86_64) | Windows |
| --- | --- | --- | --- |
| Sandbox | Seatbelt | Landlock + user namespaces | not yet |
| Copy-on-write copies | APFS clones | Btrfs/XFS reflinks; full copy on ext4 | – |
| Sandbox refusals listed | yes | no (the command still gets `EPERM`) | – |

Linux needs unprivileged user namespaces for full isolation (Ubuntu 24.04
restricts them by default). See
[Platforms](https://kulehq.github.io/kule/docs/platforms) and
[Troubleshooting](https://kulehq.github.io/kule/docs/troubleshooting).

## Developing Kule

The workspace is plain Cargo:

| Crate | What it is |
| --- | --- |
| `crates/kule-engine` | The transactional engine: task copies, effect sets, policy, sandbox, proxy, commit, rollback, undo. |
| `crates/kule-entities` | What changed inside a file: functions, methods, sections, keys. |
| `crates/kule-view` | How a file of a task's copy is shown (text, Markdown, pictures, CSV, Excel, Word). |
| `crates/kule-acp` | The ACP client: runs an agent in the sandbox and turns what it says into events. |
| `crates/kule-ext` | Extensions: manifest, `.kulext` packages, approvals. |
| `crates/kule-daemon` | The daemon behind a Unix socket, shared by the app and the CLI. |
| `crates/kule-cli` | The `kule` command. |
| `crates/kule-desktop` | The Tauri desktop app (UI in `crates/kule-desktop/ui`). |

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked        # tests run on the real filesystem and real processes
```

The documentation site lives in [`website/`](website) and is built from the
Markdown in [`docs/`](docs):

```sh
cd website && npm ci --ignore-scripts && npm start        # English at localhost:3000
npm start -- --locale tr                                  # Turkish
```

Two rules hold for every change: Kule is built on **Zero Trust** (nothing
the agent produces is trusted, every boundary is denied by default and has
a test that fails without it), and the repository is public, so security
problems are reported privately (below), never in an issue.

## Security

Report a vulnerability privately as [SECURITY.md](SECURITY.md) says. The
security model is described in
[Security](https://kulehq.github.io/kule/docs/security).

## Sponsors

Kule is built by one person, in the open. If it keeps your folders safe,
you can help it go on through
[GitHub Sponsors](https://github.com/sponsors/selcukgural): monthly or once.
Sponsors who choose it are listed here.

## License

[Apache-2.0](LICENSE)
