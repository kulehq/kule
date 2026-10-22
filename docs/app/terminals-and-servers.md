---
title: Terminals and servers
sidebar_label: Terminals and servers
description: Open your own shell in a task's copy, in its sandbox, and reach the servers a task runs from your browser.
---

# Terminals and servers

## Terminals

The **Terminal** panel holds your own shells in the task's copy, as many as
you open, one tab each.

| Action | Keys |
| --- | --- |
| Show or hide the terminal | Ctrl+\` |
| Open another terminal | Ctrl+Shift+\` |
| Open the copy in your terminal app, outside the sandbox (*Open in terminal app* in the task's ⋮ menu) | Mod+T |
| Find text in the terminal | ⌘F on macOS, Ctrl+Shift+F elsewhere |
| Open an http or https link | ⌘/Ctrl+click |
| Next, previous, clear or close the terminal | no defaults; give them keys in *Settings → Shortcuts* |

:::note The terminal runs in the task's sandbox

A terminal runs your login shell (`$SHELL -l`) in the copy, **inside the
task's sandbox**, as [`kule exec`](../cli/exec.md) runs a command; the
panel says *in the sandbox* in its corner. Job control and Ctrl+C work as
usual.

- It writes only to the copy and the task's tmp.
- Its home is the task's home layer: what it writes in `~` is caught for
  review under **Home**.
- Its network goes through the task's proxy, judged by the policy. A host
  the policy would ask you about is refused, since nobody is asked from a
  terminal.
- The agents' own state and credentials are hidden from it.

What you change in the copy from there is reviewed and committed like the
agent's work.

:::

- On Linux and Windows, only Ctrl+Shift shortcuts and function keys leave the
  shell, so Ctrl+L and the rest stay the shell's.
- The first terminal in a task can take a few seconds to open, while the
  task's package caches are made.
- Kule **refuses to open a terminal** while an agent is at work in the
  task.
- **While a terminal is open on a task**, no agent run starts in it, and
  **Commit**, **Sync** and applying home changes wait. Kule says *a terminal
  is open on task …'s copy; close it first*, rather than end what runs
  there, so nothing you have not saved is lost. Close the terminal's tab (or
  type `exit`), then go on. [`kule shell`](../cli/shell.md) counts as a
  terminal too.

### Outside the sandbox

*Open in terminal app* (Mod+T) opens the copy in your own terminal app,
and *Open the copy in your editor* (⇧⌘O, Ctrl+Shift+O elsewhere) in your
editor. Both run **outside** the sandbox, as you, so Kule asks first:

- They are refused while an agent is at work in the task, and while the
  task has changed what would run in the copy by itself: git config or
  hooks (your prompt's `git status` would run them), a toolchain pin such
  as `rust-toolchain.toml` or `.nvmrc`, `.envrc`, or an editor's settings.
  Review and commit, or roll them back, first. See
  [what runs by itself](../concepts/git.md#what-runs-by-itself-protects-your-terminal-too).
- While the copy holds changes not committed, Kule lists the changed files
  and asks whether to open the copy there. Look at them in Review first if
  you can, then choose *Open in …* or *Cancel*. It asks every time.
- Kule remembers that it opened the copy in that app. It cannot see the app
  close, so **before the next run in the task, it asks you to close it**:
  close the app, then choose *I closed it, start the run*. From the command
  line, run [`kule closed TASK`](../cli/closed.md).

The terminal app is the one [`KULE_TERMINAL`](../reference/environment.md)
names, or your system's.

## Servers the task runs

When something in the task's copy listens on a port (the agent's
`npm run dev`, or a server you started in the task's terminal), the task
header shows a **chip** such as `localhost:5173 ↗`. Click it to open the
server in your browser.

Servers the agent starts listen on this computer only: nothing on your
network (a café's Wi-Fi, the office LAN) reaches what an agent runs.
Servers you start in the task's terminal run in the same sandbox, with
one difference on macOS, below.

- On **Linux**, Kule turns every listening address into loopback, whatever
  the program.
- On **macOS**, the sandbox sees the port a server listens on but never its
  address, so it cannot hold one to this computer. While a task's network
  goes through the proxy, its programs **do not listen at all** unless you
  allow servers for it: the `servers` [limit](../concepts/budgets-and-limits.md),
  in Settings → Limits or with `kule limits --task ID servers=on`. A refused
  server is listed after the command, with how to allow it. Allowing servers
  is weaker isolation, which you accept under Policy (see
  [`kule isolation`](../cli/isolation.md)). While they are allowed:
  - Node, Python and Ruby servers are pointed at loopback when they ask for
    every address;
  - for any other program (Go, Rust, Java), Kule **shields** the port while
    the agent's run goes on, and the chip shows 🛡. No run goes on while a
    terminal is open, so such a server started in the task's terminal is
    not shielded: if the network can reach it, its chip is amber;
  - a server that listens on a network address of its own is stopped, and
    Live says why.

The chips also say what needs your attention:

| Chip | Meaning |
| --- | --- |
| Plain, with ↗ | A server on this computer only. Click to open it. |
| With 🛡 (macOS) | A server whose port Kule shields from the network while the agent's run goes on. |
| **Amber with a warning** | A server the network can still reach, typically one you started yourself: outside Kule's sandbox (in your terminal app, for example), or on macOS a Go, Rust or Java server from the task's terminal. Kule leaves yours alone. |
| **Greyed out** (Linux) | A server inside the sandbox's own network, which this computer cannot reach. |

A server the agent started ends with its run. One you started in the task's
terminal ends when that terminal closes.

## From a terminal

[`kule shell TASK`](../cli/shell.md) opens your shell in the copy, in the
task's sandbox, like the panel's terminal; `kule shell TASK --outside` opens
it outside the sandbox, with the same refusals and question as *Open in
terminal app*. After you close a terminal app or editor that Kule's window
opened the copy in, run
[`kule closed TASK`](../cli/closed.md).
[`kule path TASK`](../cli/path.md) prints where the copy is, for
`cd "$(kule path TASK)"`, and warns on stderr when something there would
run what the task changed.
