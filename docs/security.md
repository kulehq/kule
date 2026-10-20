# Kule's security model

Kule is built on Zero Trust and follows OWASP: the Top 10, the Application
Security Verification Standard (ASVS), and the Top 10 for LLM and agentic
applications. This page says what Kule protects, from whom, and where the
boundaries are. To report a problem, see [SECURITY.md](../SECURITY.md).

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
| Release ↔ you | Checksums, build provenance and a CycloneDX SBOM for every file, from a pinned toolchain (`rust-toolchain.toml`); on macOS signed and notarized where the release has an Apple certificate, else signed ad hoc, as its notes say. The builds see no secret: signing is a job of its own, in an environment that waits for a maintainer's approval, and nothing is published before it. |

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
`deny.toml`), the built binary through `cargo audit bin` (built with
cargo-auditable), and canary secrets planted in a workspace's env files,
which must be masked in a task and found nowhere but the workspace. Each
tool runs at a pinned version, and none sends what it scans anywhere.
After the release is built, `scripts/release-gate.sh attest <tag>`
verifies every asset's provenance against the release workflow and the
tag. CI runs the workflow and canary checks on every change.
