---
title: kule home
sidebar_label: kule home
description: Review what a task's commands wrote in your home directory, then apply, discard or undo it.
---

# kule home

Review what a task's commands wrote in your **home directory**
(`~/.zshrc`, `~/.config`, new files at the top of `~`), then apply it,
discard it or take an apply back.

## Synopsis

```sh
kule home [OPTIONS] <TASK> [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-home-task-list) | List the writes caught for review (the default). |
| [`diff`](#kule-home-task-diff) | Show what one change would do to your file, line by line. |
| [`apply`](#kule-home-task-apply) | Take changes to your real home. |
| [`discard`](#kule-home-task-discard) | Throw changes away. |
| [`undo`](#kule-home-task-undo) | Take the last apply back. |

Note the order: the task comes **before** the subcommand
(`kule home 1046fb30 diff ~/.zshrc`).

## Description

Agents and tools often write in your home: `git config --global`, a line
added to `~/.zshrc`, a tool's settings under `~/.config`. In Kule these writes
are not refused, and they do not reach your home either. A task's commands
see a **home layer** (`<task>/home`) as their `HOME`: the small files at the
top of your home and `~/.config` are copies there, and everything else is a
link to your own. What a command writes in the layer is **caught for
review**. See [Home layer](../concepts/home-layer.md).

- A home change is never part of a `kule commit`. You decide on it here.
- `apply` first checks that your file is still as it was when the layer was
  made, and keeps what it replaces in the task's `home-kept/` folder.
- Files that shells and tools read and run as they start (`.zshrc`,
  `.profile`, `.gitconfig`, `~/.config/fish`, `~/.config/autostart`, systemd
  user units, …) are marked *runs every time a shell or tool starts*. Look at
  their diff before you apply them.
- While changes wait, the layer stays as it is from run to run. Once none
  wait, the next run starts from your home as it is then.
- Settings in the layer that look like secrets (`export GITHUB_TOKEN=…`) are
  masked for the task's commands; an apply puts your value back. Search
  paths (`export PATH="$HOME/.cargo/bin:$PATH"`) stay as they are, so a
  shell in the task finds your tools.

### Writing paths

Give paths **as `list` prints them**, like `~/.zshrc` or `~/.config/demo/conf`.
Any of these name the same file: the shell may turn `~` into your full home
path, and Kule takes that too. A path outside your home, or one with `..`
in it, is refused.

```sh
kule home 1046fb30 diff ~/.zshrc             # the shell expands ~; fine
kule home 1046fb30 diff '~/.zshrc'           # quoted; fine
kule home 1046fb30 diff .zshrc               # relative to your home; fine
kule home 1046fb30 diff /Users/you/.zshrc    # your full home path; fine
```

All subcommands take `--home` (Kule's home, not yours; see
[global options](index.md)).

## kule home TASK list

```sh
kule home <TASK> list [OPTIONS]
```

```console
$ kule home 1046fb30
in your home, caught for review (3; `kule home 1046fb30 apply PATH` takes one there):
  + ~/.config/demo/
  + ~/.config/demo/conf
  ~ ~/.zshrc  (runs every time a shell or tool starts)
```

`+` is new, `~` changed, `-` deleted. A trailing `/` is a folder, and `->`
shows where a new link points. With nothing caught:
`nothing written in your home`.

`kule exec`, `kule status` and the app's Review list the same.

## kule home TASK diff

```sh
kule home <TASK> diff [OPTIONS] <PATH>
```

```console
$ kule home 1046fb30 diff ~/.zshrc
--- ~/.zshrc (yours)
+++ ~/.zshrc (the task's)
@@ line 226 @@
 export GITHUB_TOKEN=‹hidden 3fa9c2›
 . "$HOME/.cargo/env"
+export FOO=1
```

What looks like a secret is hidden on both sides: the value of a setting
named like a key, token or password, a secret Kule keeps, and anything
shaped like a token. Each shows as `‹hidden …›`, the same mark wherever
the same value is in that diff, so a changed secret still reads as
changed. The marks differ from one diff to the next.

A file that is not text says `not text, so not shown`; a very large one
`too large to show`. A long diff ends with `(cut here: the change is longer)`.

## kule home TASK apply

```sh
kule home <TASK> apply [OPTIONS] [PATHS]...
```

| Argument / option | Meaning |
| --- | --- |
| `PATHS...` | The paths to apply, as listed. |
| `--all` | Every change caught for this task. |

```console
$ kule home 1046fb30 apply '~/.config/demo' '~/.config/demo/conf'
applied ~/.config/demo
applied ~/.config/demo/conf
$ kule home 1046fb30 apply '~/.gitconfig'
applied ~/.gitconfig
what they replaced is kept in ~/.kule/tasks/1046fb30/home-kept/00000001792506123456
```

The last line shows only when the apply replaced or removed something of
yours. Each apply gets its own folder there, named by when it happened.

When your file changed since the layer was made, the apply is refused, so
nothing of yours is overwritten unseen.

## kule home TASK discard

```sh
kule home <TASK> discard [OPTIONS] [PATHS]...
```

| Argument / option | Meaning |
| --- | --- |
| `PATHS...` | The paths to discard, as listed. |
| `--all` | Every change caught for this task. |

```console
$ kule home 1046fb30 discard --all
discarded 3 changes
```

## kule home TASK undo

```sh
kule home <TASK> undo [OPTIONS]
```

Takes the last apply back, if you have not changed those paths since. The
changes then wait for review again.

```console
$ kule home 1046fb30 undo
undid ~/.config/demo/conf
undid ~/.config/demo
the changes wait for review again
```

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: name the paths (as listed, like ~/.zshrc), or --all` | `apply` and `discard` need paths or `--all`. |
| `kule: /etc/hosts is not in your home (/Users/you)` | Only paths in your home are caught. Name one as `list` prints it. |
| `kule: name a path inside your home, as listed, like ~/.zshrc (not ../x)` | The path leaves your home, or names the home itself. |
| `kule: ~/.bashrc has no change waiting` | Nothing the task wrote is waiting there; `kule home TASK` lists what is. |
| `nothing to apply at those paths` | The paths have no waiting change; check `kule home TASK`. |
| `kule: nothing applied from this task's home to undo` | There is no apply to take back. |

## In the app

Review lists **Writes in your home** apart from the commit, each with *Show*
(the diff), *Apply*, *Discard* and *Undo*. A file that runs at every start is
marked and opens its diff right away.

## See also

- [Home layer](../concepts/home-layer.md)
- [`kule status`](status.md), [`kule exec`](exec.md)
