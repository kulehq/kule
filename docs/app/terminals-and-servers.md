---
title: Terminals and servers
sidebar_label: Terminals and servers
description: Open your own shell in a task's copy, and reach the servers a task runs from your browser.
---

# Terminals and servers

## Terminals

The **Terminal** panel holds your own shells in the task's copy, as many as
you open, one tab each.

| Action | Keys |
| --- | --- |
| Show or hide the terminal | Ctrl+\` |
| Open another terminal | Ctrl+Shift+\` |
| Open a terminal from the task's ⋮ menu (*Open terminal*) | Mod+T |
| Find text in the terminal | ⌘F on macOS, Ctrl+Shift+F elsewhere |
| Open an http or https link | ⌘/Ctrl+click |
| Next, previous, clear or close the terminal | no defaults; give them keys in *Settings → Shortcuts* |

:::warning The terminal is yours, outside the sandbox

A terminal runs **outside** the sandbox, with your own permissions; the
panel says *outside the sandbox* in its corner. What you change in the copy
from there is reviewed and committed like the agent's work, but the commands
themselves can reach anything you can.

:::

- On Linux and Windows, only Ctrl+Shift shortcuts and function keys leave the
  shell, so Ctrl+L and the rest stay the shell's.
- Kule **refuses to open a terminal** while an agent is at work in the
  task, and while the task has changed what would run in the copy by
  itself: git config or hooks (your prompt's `git status` would run them),
  a toolchain pin such as `rust-toolchain.toml` or `.nvmrc`, `.envrc`, or an
  editor's settings, all outside the sandbox. Review and commit, or roll
  them back, first. The same holds for *Open the copy in your editor*. See
  [what runs by itself](../concepts/git.md#what-runs-by-itself-protects-your-terminal-too).

## Servers the task runs

When something in the task's copy listens on a port (the agent's
`npm run dev`, or a server you started in the task's terminal), the task
header shows a **chip** such as `localhost:5173 ↗`. Click it to open the
server in your browser.

Servers a task starts listen on this computer only: nothing on your network
(a café's Wi-Fi, the office LAN) reaches what an agent runs.

- On **Linux**, Kule turns every listening address into loopback, whatever
  the program.
- On **macOS**:
  - Node, Python and Ruby servers are pointed at loopback when they ask for
    every address;
  - for any other program (Go, Rust, Java), Kule **shields** the port while
    the run goes on, and the chip shows 🛡;
  - a server that listens on a network address of its own is stopped, and
    Live says why.

The chips also say what needs your attention:

| Chip | Meaning |
| --- | --- |
| Plain, with ↗ | A server on this computer only. Click to open it. |
| With 🛡 (macOS) | A server whose port Kule shields from the network while the run goes on. |
| **Amber with a warning** | A server the network can still reach, typically one you started yourself in a terminal. Kule leaves yours alone. |
| **Greyed out** (Linux) | A server inside the sandbox's own network, which this computer cannot reach. |

A server the agent started ends with its run. One you started in a terminal
stays.

## From a terminal

[`kule shell TASK`](../cli/shell.md) opens your shell in the copy, outside the
sandbox, with the same refusals.
[`kule path TASK`](../cli/path.md) prints where the copy is, for
`cd "$(kule path TASK)"`, and warns on stderr when something there would
run what the task changed.
