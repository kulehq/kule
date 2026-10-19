# Kule — notes for Claude

- Design doc (Turkish): https://claude.ai/code/artifact/fd4bda01-5697-42ac-b415-81b15594b8ce
  Its "Kararlar" table is the source of truth for decisions.
- Milestones: M0 engine spike (CoW copy, effects, commit/rollback/undo) ·
  M1 git effects (gitoxide) → sandbox → Cedar policy → proxy (`lonca-net` from Lonca) · M2 Tauri app
  + ACP client · M3 approval inbox, timeline, checkpoints on Lonca · M4
  parallel tasks and measurement. v0 = macOS and Linux; Windows is v1.
- Run cargo with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Tests run on the real filesystem (tempdirs), never on mocks. Every fix gets
  a test that fails without it.
- Chat with the user in Turkish; code, comments and repo docs in English.

## Security principle (standing, applies to every change)

Kule is built on Zero Trust and follows OWASP (Top 10, ASVS, and the Top 10
for LLM and agentic applications). Every change and feature must follow it:

- **Nothing is trusted by default**: the agent, everything it writes or
  prints, its processes, the files Kule opens, imported profiles and themes,
  extensions, MCP servers, other local processes and the network.
- **Deny by default.** Allow-lists over deny-lists; a check that cannot
  decide refuses.
- **Nothing weakens silently.** A weaker mode is refused, or the person opts
  into it knowingly and sees it while it holds.
- **Least privilege** for every process, token, permission and CI job.
- **Every boundary has a test that fails without it**, on real processes and
  the real filesystem.
- **Before a change is done**, walk the OWASP categories it touches: access
  control, injection and XSS, secrets, SSRF, integrity of what is committed
  or installed, supply chain, logging.
- **This repository is public.** No vulnerability details in commits,
  CHANGELOG, issues or docs until the fix has shipped; commit messages say
  what Kule now does, not how it could be abused. Report problems as
  [SECURITY.md](SECURITY.md) says.
