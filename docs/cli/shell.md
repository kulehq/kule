---
title: kule shell
sidebar_label: kule shell
description: Open your own shell in the task's copy, outside the sandbox, to look around or make changes by hand.
---

# kule shell

Open your shell in the task's copy, outside the sandbox. What you change
there is reviewed and committed like the agent's work.

## Synopsis

```sh
kule shell [OPTIONS] <TASK>
```

## Description

`kule shell` starts your login shell (`$SHELL`, or `/bin/sh`) with its working
directory in the task's copy. Use it to look at the agent's work, run the
app the agent built, or fix something by hand before you commit.

This shell is **yours**: it runs outside the sandbox, with your own access to
files, network and credentials. It is a convenience, not a place to run
something you do not trust; use [kule exec](exec.md) for that.

What you change in the copy is part of the task's changes, like the
agent's: `kule status` lists it and `kule commit` applies it.

`KULE_TASK` holds the task's id inside the shell. When you `exit`, Kule
prints the copy's changes, and `kule shell` exits with the shell's exit code.

**Kule refuses to open the shell while the task has changed the copy's git
config or hooks.** Your prompt's `git status`, or any git command you type,
would run them outside the sandbox. Review those changes with
[kule status](status.md), then commit or roll them back first.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Look around in the copy:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, outside the sandbox; exit to come back
$ ls
Cargo.toml  README.md  src
$ exit
no changes
```

**Fix something by hand, then commit it:**

```text
$ kule shell 3422f830
kule: in task 3422f830's copy, outside the sandbox; exit to come back
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

**Refused after a git config change:**

```text
$ kule shell 3422f830
kule: the copy's git config changed, and git there would run it outside the sandbox; review it, then commit or roll it back before opening a terminal
```

## Output

| Line | Meaning |
| --- | --- |
| `kule: in task <id>'s copy, outside the sandbox; exit to come back` | On stderr, before the shell starts. |
| the change list after `exit` | The task's changes so far, as [kule status](status.md) shows them. |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: the copy's git config changed, …` / `… hooks …` | Run `kule status TASK`, then `kule commit` (if you trust the change; a new hook is always denied) or `kule rollback`. |
| `kule: could not start <shell>` | `$SHELL` names a program that does not exist. |

## In the app

*Open terminal* in the task's ⋮ menu (Mod+T), or the **Terminal** panel
(Ctrl+\`). See [Terminals and servers](../app/terminals-and-servers.md).

## See also

- [kule path](path.md): just the path, for `cd` or your editor.
- [kule exec](exec.md): a command inside the sandbox.
