---
title: Kule's security model
sidebar_label: Security model
description: What Kule protects, from whom, where its boundaries are, and how to report a vulnerability.
---

# Kule's security model

Kule is built on Zero Trust and follows OWASP: the Top 10, the Application
Security Verification Standard (ASVS), and the Top 10 for LLM and agentic
applications. This page says what Kule protects, from whom, and where the
boundaries are.

:::warning Reporting a vulnerability

Report it privately, as
[SECURITY.md](https://github.com/kulehq/kule/blob/main/SECURITY.md) says
(the repository's **Security** tab → **Report a vulnerability**; only the
maintainers see it). Never in a public issue, pull request or discussion.

:::

## What is protected

- **Your folders**: nothing an agent does reaches them until the policy or
  you let it through, and what reaches them is what was reviewed.
- **Your home and your other projects**: an agent works in a copy, in a
  sandbox, with the rest of the computer out of its reach as far as the
  platform allows.
- **Your secrets**: API keys and tokens stay with Kule; the agent works
  with placeholders.
- **Your network**: an agent reaches the hosts the policy allows, through
  Kule's proxy, and nothing else.
- **You**: questions, reviews and approvals show what is really there, and
  nothing answers or approves for you.

## Who is not trusted

Nothing is trusted by default:

- **The agent**, everything it writes or prints, every process it starts,
  and every request it makes. A prompt injection in a file the agent reads
  is assumed to succeed; Kule's boundaries are what hold.
- **Files Kule opens** for preview or review, and what the agent puts in
  them: names, contents, links and their targets.
- **Imported profiles and themes**, **extensions** and **MCP servers**: each
  is checked on the way in and approved by you before it runs.
- **Other local users and processes**, and **the network**.

## The boundaries

| Boundary | What holds it |
|---|---|
| Agent ↔ your folder | A copy-on-write copy; effects are scanned, judged by the Cedar policy (errors refuse), and committed only as reviewed. A repository's own policy can only add refusals. |
| Agent ↔ the computer | The sandbox: Seatbelt on macOS; Landlock, seccomp and namespaces on Linux. Writes only in the copy and the task's temporary folders; secrets and credentials hidden; a weaker sandbox is never used without saying so. |
| Agent ↔ the network | Every connection goes through Kule's proxy, which allows the hosts the policy names, checks the address it connects to, and keeps private addresses out of reach. |
| Agent ↔ secrets | Placeholders in the agent's environment; the proxy fills them in on the way out to the secret's own host, and hides them in what comes back. |
| Window ↔ daemon | A socket only your user can open, a token, size limits. |
| Window ↔ untrusted text | Built with DOM calls and `textContent` only; a strict content security policy with no inline scripts; links open only as http(s). |
| Extensions ↔ Kule | Sandboxed frames on their own origin; every call checked against the permissions you approved; network only to approved public hosts. |
| Release ↔ you | Checksums, build provenance and a CycloneDX SBOM for every file, from a pinned toolchain (`rust-toolchain.toml`); on macOS signed ad hoc, never notarized, as its notes say. No job of the release workflow holds a secret, and publishing waits in an environment for a maintainer's approval: nothing is published before it. |

## What Kule does not cover

Kule holds back only the effects it can keep from the world until you
decide: a change to your folder waits for commit, a connection waits at the
proxy. An action with no copy, no diff and no undo, such as a click, a
keystroke or a message sent from an app on your screen, cannot wait. So
Kule does not support it, by design:

- **Inside a task, it is refused.** The sandbox keeps commands, and the MCP
  servers the agent starts, away from the screen, the keyboard, the
  clipboard and other apps, as far as the platform allows.
- **There is no gate on tool calls.** A tool call is the agent describing
  what it is about to do, and Kule does not trust that description. Trusting
  the tool's own report instead would hand that trust to third-party code.
- **Outside a task, Kule protects nothing.** An agent or tool you run
  outside Kule, a screen-driving MCP server included, is not reviewed, even
  while Kule is running.

## For contributors

Every change follows these rules:

- **Deny by default.** Allow-lists over deny-lists; a check that cannot
  decide refuses.
- **Nothing weakens silently.** A weaker mode is refused, or the person
  opts into it knowingly and sees it while it holds.
- **Least privilege** for every process, token, permission and CI job.
- **Every boundary has a test that fails without it**, on real processes and
  the real filesystem, never on mocks.
- **Before a change is done**, walk the OWASP categories it touches: access
  control, injection and XSS, secrets, SSRF, integrity of what is committed
  or installed, supply chain, logging.
- **No vulnerability details in public** (commits, CHANGELOG, issues, docs)
  until the fix has shipped.

Before a release, besides CI, `scripts/release-gate.sh` runs on macOS and
on Linux: the workflows through actionlint and zizmor, the lockfiles
through osv-scanner (accepted advisories in `osv-scanner.toml`, as in
`deny.toml`), the kule binary through `cargo audit bin` (the release
builds carry their dependency tree, with cargo-auditable, and a binary
without one is refused, as cargo audit would read only part of it), and
canary secrets planted in a workspace's env files,
which must be masked in a task and found nowhere but the workspace. It
runs [sandbox-probe](https://github.com/controlplaneio/sandbox-probe) in a
task, through the proxy and with no network, against a home holding
something at every path Kule hides: nothing there, nor Kule's own key, the
task's base or a project beside the workspace, may be read; the workspace
and the home may not be written, no host reached, no secret in the
environment seen. The same scan outside the task must find each of them
first, so a clean result means something. And it installs, builds and
tests real projects in a task (ripgrep from crates.io, commander from npm)
with a home holding a token and settings: the workspace stays as it was,
the token stays masked, and a host the policy does not name is refused.
Each tool runs at a pinned version. None sends a file it scans anywhere;
osv-scanner asks osv.dev about the package names and versions in the
lockfiles.
After the release is built, `scripts/release-gate.sh attest <tag>`
verifies every asset's provenance against the release workflow and the
tag. CI runs the workflow, lockfile, canary, probe and project checks on
every change, and the release workflow runs the binary, probe and project
checks on the binaries it is about to publish, on macOS and Linux.
