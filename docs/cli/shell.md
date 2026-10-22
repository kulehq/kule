---
title: kule shell
sidebar_label: kule shell
description: Open your own shell in the task's copy, in its sandbox, to look around or make changes by hand.
---

# kule shell

Open your shell in the task's copy, in the task's sandbox. What you change
there is reviewed and committed like the agent's work.

## Synopsis

```sh
kule shell [OPTIONS] <TASK>
```

## Description

`kule shell` starts your shell (`$SHELL`, or `/bin/sh`) with its working
directory in the task's copy. Use it to look at the agent's work, run the
app the agent built, or fix something by hand before you commit.

The shell runs **inside the task's sandbox**, as [kule exec](exec.md) runs
a command:

- it writes only to the copy and the task's tmp;
- its home is the task's home layer: what it writes in `~` is caught for
  review with [kule home](home.md);
- its network goes through the task's proxy, judged by the policy; a host
  the policy would ask about is refused, since the command line has nobody
  to ask;
- the agents' own state and credentials are hidden from it.

The shell runs on a terminal of its own; Kule passes your keys and its
output through. Your own terminal is never handed to the sandbox.

What you change in the copy is part of the task's changes, like the
agent's: `kule status` lists it and `kule commit` applies it.

`KULE_TASK` holds the task's id inside the shell. When you `exit`, Kule
prints the copy's changes, the connections the shell made, home writes and
what the sandbox refused, as `kule exec` does, and `kule shell` exits with
the shell's exit code.

**While the shell is open**, no agent starts work in the task (`kule run`
and `kule exec` are refused too), and commit, sync and applying home
changes wait until it exits. Kule refuses to open the shell while an agent
is at work in the task.

### Outside the sandbox

With `--outside`, the shell runs outside the sandbox, as you, with your own
access to files, network and credentials. It is a convenience, not a place
to run something you do not trust.

- It is refused while an agent is at work in the task, and while the task
  has changed what would run in the copy by itself: git config or hooks
  (your prompt's `git status`, or any git command you type, would run
  them), or a file such as a toolchain pin or `.envrc` that your prompt
  reads on entering the folder. See
  [what runs by itself](../concepts/git.md#what-runs-by-itself-protects-your-terminal-too).
  Review those changes with [kule status](status.md), then commit or roll
  them back first.
- While the copy holds changes not committed, Kule lists them on stderr
  (up to 20) and asks before it opens the shell. Without a terminal to ask
  on, it refuses unless you pass `--yes`; the changes are listed all the
  same.

Like the sandboxed shell, it counts as a terminal open on the task until
it exits.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--outside` | | | Open the shell outside the sandbox, as you. |
| `--yes` | | | With `--outside`: open without asking. The changes not committed are still listed. |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Look around in the copy:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, in its sandbox; exit to come back
$ ls
Cargo.toml  README.md  src
$ exit
no changes
```

**Fix something by hand, then commit it:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, in its sandbox; exit to come back
$ sed -i '' 's/hi /hello /' src/main.rs
$ exit
1 file changes: +0 ~1 -0
  ~ src/main.rs
      ~ function greet
$ kule commit 3422f830
committed 1 changes · checkpoint 2
```

**Use a different shell:**

```sh
SHELL=/bin/bash kule shell 3422f830
```

**Outside the sandbox, while the copy holds changes:**

```text
$ kule shell 3422f830 --outside
kule: the copy holds changes not committed, which your shell, its prompt and the tools you run there may run as you, outside the sandbox:
  src/main.rs
  NOTES.md
kule: open your shell there outside the sandbox? [y/N] y
kule: in task 3422f830's copy, outside the sandbox; exit to come back
```

**Refused outside the sandbox after a git config change:**

```text
$ kule shell 3422f830 --outside
kule: the copy's git config changed, and git there would run it outside the sandbox; review it, then commit or roll it back before opening a terminal
```

**Commit waits while the shell is open:**

```text
$ kule commit 3422f830
kule: a terminal is open on task 3422f830's copy (held by process 48213); close it first, then commit
```

## Output

| Line | Meaning |
| --- | --- |
| `kule: in task <id>'s copy, in its sandbox; exit to come back` | On stderr, before the shell starts. |
| `kule: in task <id>'s copy, outside the sandbox; exit to come back` | The same, with `--outside`. |
| the change list after `exit` | The task's changes so far, as [kule status](status.md) shows them. |
| `net:`, home and `sandbox:` lines after `exit` | The connections the shell made, its home writes and what the sandbox refused, as [kule exec](exec.md) prints them. Not with `--outside`. |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: an agent is at work in task <id> …` | Wait for the agent's run to end. |
| `kule: the copy's git config changed, …` / `… hooks …` | With `--outside` only. Run `kule status TASK`, then `kule commit` (if you trust the change; a new hook is always denied) or `kule rollback`. Or open the shell in the sandbox, without `--outside`. |
| `kule: review them first, or pass --yes to open your shell there anyway` | With `--outside`, the copy holds changes not committed and there is no terminal to ask on. Look at them with [kule status](status.md), or pass `--yes`. |
| `kule: not opened` | You answered no. |
| `kule: a terminal is open on task <id>'s copy …; close it first, then commit` | From `kule commit`, `kule sync` or `kule home apply` while the shell is open. `exit` the shell first. |
| `kule: could not start <shell>` | `$SHELL` names a program that does not exist. |

## In the app

The **Terminal** panel (Ctrl+\`) runs your shell in the sandbox, like
`kule shell`. *Open in terminal app* in the task's ⋮ menu (Mod+T) opens the
copy in your terminal app, outside the sandbox, like `--outside`. See
[Terminals and servers](../app/terminals-and-servers.md).

## See also

- [kule closed](closed.md): after you close what the app opened the copy in.
- [kule path](path.md): just the path, for `cd` or your editor.
- [kule exec](exec.md): one command inside the sandbox.
