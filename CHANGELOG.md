# Changes

## 0.2.0

The first release: the macOS app (universal, signed and notarized), a Linux
`.deb`, and the `kule` command-line tool inside both.

### Tasks on a copy of any folder

- A task is a copy-on-write copy of a folder: code, notes, photos or data.
  Everything an agent does there is one uncommitted transaction; commit
  applies it with a checkpoint, rollback throws it away, undo reverts the
  last commit. Checkpoints live in a Lonca store, and every commit and undo
  is an entry in a signed, hash-chained history.
- Review shows what changed inside each file (methods, sections, keys), a
  picture before and after, moves told apart from deletions, and where two
  tasks on one folder meet. A file two tasks changed merges method by
  method; only the same parts changed twice conflict.
- Writes in your home directory (`~/.zshrc`, `git config --global`) are
  caught for review, with a line diff, a mark on files that run at every
  start, and undo.

### A sandbox and a policy

- Commands run in Seatbelt on macOS and Landlock (with a network namespace
  of their own) on Linux: they write only to the copy, credentials are
  hidden, and the network goes through the task's proxy.
- A Cedar policy decides what passes, what waits for you and what never
  passes. Choose Kule's balanced or careful one, or write your own; a
  repository can add refusals. What you approve can be remembered.
- Secrets reach the hosts they are for and never the agent: it sees a
  placeholder, and the proxy fills in the value, in headers and bodies.
  The built-in agents' API keys go the same way.

### Agents

- Claude Code, Codex and Gemini over the Agent Client Protocol, each with
  a profile that opens only its own state and login. Add any other ACP
  agent from the registry or by hand; it runs once you approve what it
  opens, and a trial suggests what it needs.
- Costs as the agents report them, with a budget per task and a daily one
  for every task together, and a notice at 80%.

### The app

- Live stream, approval inbox, review and timeline, around a daemon that
  keeps the runs going when the window is closed.
- Tasks with names, colours and an order of your own; a ⋮ menu on each.
- Settings for agents, the policy, remembered rules, secrets and the
  budget, and a first-run screen that checks the agents and asks how
  careful to be.
