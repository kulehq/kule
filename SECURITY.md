# Security

Kule runs AI agents on your folders, so a hole in it is a hole in what you
trust it with. Reports are welcome and taken seriously.

## Reporting a vulnerability

Report it privately, never in a public issue, pull request or discussion:

- On GitHub: the repository's **Security** tab → **Report a vulnerability**.
  Only the maintainers see it.

Say what you found, the version or commit, the platform (macOS or Linux, and
the kernel on Linux), and how to reproduce it. You will get an answer within
a week. A fix is made in private and released, and the advisory is
published once the release is out, crediting you unless you prefer not.

## What counts

Anything that lets an agent, a file Kule opens, an imported profile or
theme, an extension or MCP server, another local user, or the network do
more than Kule says it can: reach outside the task's copy or the sandbox,
get a change into your folder without the policy or you letting it through,
read a secret, or reach a host the policy refuses. See
[docs/security.md](docs/security.md) for the boundaries Kule draws.

## Supported versions

The latest release. Fixes are not backported.
