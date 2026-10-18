# Changes

## Unreleased

### Preview

- A file picked in Files opens large in the main area's new Preview tab
  (`Mod+4`), as it is now in the task's copy:
  - text and code with line numbers; JSON or XML packed onto a line or two
    can be shown formatted;
  - Markdown laid out, or as its source;
  - PNG, JPEG, GIF and WebP pictures, fitted or at 100%.
- Text shows up to 1 MB of a file. A picture is shown only when its bytes
  prove it to be one of a sane size, the same check Review uses.

### Extensions

- Extensions add commands, panels, themes, settings, notes on a review,
  agent profiles and MCP servers. They are written in JavaScript and shared
  as `.kulext` files (`kule ext new | dev | pack | info | install | approve
  | enable | disable | remove | mcp`, or Settings → Extensions).
- Approval is pinned by digest:
  - an extension asks for permissions from a closed list;
  - it runs only once approved as it stands, and waits again after any
    change;
  - the approval screen names what is new since last time.
- Its code runs in sandboxed frames served from loopback, which reach
  nothing but Kule. Kule answers each call only if the approved permissions
  cover it:
  - `net.fetch` reaches the approved hosts over https only, and never an
    address on the computer or its network;
  - keys pressed in a frame only move the view.
- A `.kulext` is never unpacked. Each entry's path, kind and inflated size
  are checked.
- Agent profiles and MCP servers from an extension are each approved on
  their own. MCP servers are started by the agent inside its sandbox.
- For extension authors (a preview: the format and the SDK may still
  change before 1.0):
  - ES modules work in frames;
  - every frame's console output and errors show in the extension's card;
  - a folder under development has Reload;
  - the guide covers language and tools, lifecycle, debugging, sharing,
    updating and limits.
- Hardened after an OWASP review:
  - frames may come from the extension server's port only;
  - WebRTC is off in the window;
  - files are read without following links;
  - folders under development may not sit where agents can write;
  - a frame can only show panels;
  - MCP servers start one exact registry package;
  - `kule ext` goes through the daemon and asks before approving;
  - the publisher `kule` is reserved;
  - a safe mode, `--no-extensions`, is added.
- Daemon protocol 23. The new `TaskChanged` notice says when a task is
  created, committed, rolled back, undone or removed.

## 0.2.0

The first release: the macOS app (universal, signed and notarized), Linux
`.deb` and `.rpm` packages, and the `kule` command-line tool inside each.

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
