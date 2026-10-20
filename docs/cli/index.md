---
title: CLI reference
sidebar_label: Overview
description: The kule command, its global options, how to name a task, and every command on one page.
---

# CLI reference

`kule` is Kule's command-line tool. It works on the same tasks as the
desktop app: a task you make with `kule new` shows up in the app, and a task
you make in the app can be committed with `kule commit`. Everything lives in
Kule's home folder (`~/.kule` unless you choose another), and both the app
and the CLI read and write it.

If you have not installed it yet, see [Install](../getting-started/install.md).
To learn the ideas first, read [How Kule works](../concepts/how-kule-works.md).

## The loop in one block

Every command fits somewhere in this loop:

```sh
kule new ~/code/app                     # 1. make a task: a copy-on-write copy of the folder
kule run 3422f830 "Fix the failing test" # 2a. let an agent work in the copy
kule exec 3422f830 -- npm test           # 2b. or run a command there yourself, in the sandbox
kule status 3422f830                     # 3. see what changed and what the policy says
kule commit 3422f830                     # 4a. apply it to the real folder (a checkpoint is kept)
kule rollback 3422f830                   # 4b. or throw it away
kule undo 3422f830                       # 5. take the last commit back out of the folder
```

Nothing reaches your real folder (the *workspace*) before step 4a.

## Global options

These work on every command and subcommand.

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Where Kule keeps tasks, settings, secrets and policy. Also set by the `KULE_HOME` environment variable. The flag wins over the variable. |
| `-h`, `--help` | | | Print help for the command. |
| `-V`, `--version` | | | Print Kule's version (top level only). |

`kule help COMMAND` prints the same help as `kule COMMAND --help`, and
`kule help group put` works for subcommands.

Use a separate home to experiment without touching your real tasks:

```sh
export KULE_HOME=/tmp/kule-try
kule new ~/code/app
```

:::note

The app and the CLI only see the same tasks when they use the same home. If
you set `KULE_HOME` in a terminal, the app (started from the Finder or a
launcher) still uses `~/.kule`.

:::

## Naming a task

Every task has an id of eight letters and digits, printed when you make it:

```text
$ kule new ~/code/app
task 3422f830 · copy at ~/.kule/tasks/3422f830/w/app
```

Wherever a command takes `TASK`, pass this **full id**. Prefixes and task
names are not accepted:

```text
$ kule status 3422
kule: no task 3422
```

`kule list` prints every id with its folder, name, colour and group. In
scripts, keep the id from `kule new`:

```sh
TASK=$(kule new ~/code/app | awk '{print $2}')
```

## Every command

### Tasks

| Command | What it does |
| --- | --- |
| [`kule new`](new.md) | Make a task: a copy-on-write copy of a folder. |
| [`kule list`](list.md) | List tasks, in the order the app shows them. |
| [`kule name`](name.md) | Give a task a name, or take it away. |
| [`kule color`](color.md) | Mark a task with a colour, or take it away. |
| [`kule rm`](rm.md) | Delete a task and its copy. The folder is not touched. |

### Working in a task

| Command | What it does |
| --- | --- |
| [`kule run`](run.md) | Give an agent (Claude Code, Codex, Gemini, …) a prompt; it works in the copy, in the sandbox. |
| [`kule exec`](exec.md) | Run one command in the copy, in the sandbox, then show what it changed. |
| [`kule shell`](shell.md) | Open your own shell in the copy, outside the sandbox. |
| [`kule path`](path.md) | Print where the copy, or a file in it, is on disk. |
| [`kule open`](open.md) | Open a document, picture or video of the copy with its app. |

### Review and history

| Command | What it does |
| --- | --- |
| [`kule status`](status.md) | Show the uncommitted changes and the policy's ruling on them. |
| [`kule steps`](steps.md) | List the agent's tool calls and what each changed. |
| [`kule commit`](commit.md) | Apply the changes to the folder, keeping a checkpoint. |
| [`kule rollback`](rollback.md) | Throw the uncommitted changes away. |
| [`kule undo`](undo.md) | Take the newest commit back out of the folder. |
| [`kule sync`](sync.md) | Bring what the folder changed meanwhile into the copy. |
| [`kule history`](history.md) | Show the task's signed history of commits and undos, checked. |

### Cost and limits

| Command | What it does |
| --- | --- |
| [`kule budget`](budget.md) | Show or set what one task's agent runs may cost. |
| [`kule daily-budget`](daily-budget.md) | Show or set what every task may cost together in any 24 hours. |
| [`kule limits`](limits.md) | Show or set CPU, memory, disk, GPU and time limits on agent runs. |

### Groups

| Command | What it does |
| --- | --- |
| [`kule group`](group.md) | Make and manage groups of tasks, each with a colour, a policy, a budget and limits. |

### Policy and access

| Command | What it does |
| --- | --- |
| [`kule policy`](policy.md) | Print the policy that holds, or switch between `balanced` and `careful`. |
| [`kule rules`](rules.md) | List, add or forget what you told Kule not to ask about again. |
| [`kule reveal`](reveal.md) | Let a folder's tasks read a project beside it (read-only). |
| [`kule isolation`](isolation.md) | Show what this computer cannot give the sandbox, and accept it or not. |

### Home and secrets

| Command | What it does |
| --- | --- |
| [`kule home`](home.md) | Review, apply or discard what a task's commands wrote in your home folder. |
| [`kule secrets`](secrets.md) | Keep API keys that commands use without ever seeing them. |

### Agents and extensions

| Command | What it does |
| --- | --- |
| [`kule agents`](kule-agents.md) | List, inspect, add, try and approve the agents Kule can run. |
| [`kule ext`](ext.md) | Make, pack, install, approve and manage extensions. |

### Daemon

| Command | What it does |
| --- | --- |
| [`kule daemon`](daemon.md) | Run the background service the app talks to. The app starts it for you. |

## Scripting with kule

**Exit codes.** Kule uses a small, fixed set:

| Code | Meaning |
| --- | --- |
| `0` | It worked. |
| `2` | Kule refused or failed. The reason is on stderr, after `kule:`; sometimes a `hint:` line follows. Bad arguments also exit with `2` (from the argument parser). |
| `3` | `kule commit` only: the policy held the changes back (they wait for approval, or are denied). Nothing was committed. |
| the command's own | `kule exec` and `kule shell` exit with the exit code of the command or shell they ran, so `kule exec TASK -- npm test && kule commit TASK` does what it reads. |

**Machine-readable output.** `kule status TASK --json` prints the changes, the
policy's ruling, what the agents spent and more as one JSON object; see
[kule status](status.md). Other commands print text meant for
people, one fact per line.

**Errors go to stderr, results to stdout.** `kule path` prints only the path
on stdout, so `cd "$(kule path TASK)"` works even when it also prints a
warning.

**Colour.** Output is coloured only on a terminal; piped output is plain.

## See also

- [Quick start](../getting-started/quick-start.md): the loop with real output.
- [Files and settings](../reference/files.md): what is inside Kule's home.
- [Environment variables](../reference/environment.md).
