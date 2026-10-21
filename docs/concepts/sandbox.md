---
title: The sandbox
sidebar_label: Sandbox
description: What a command or agent in a task can and cannot do - where it may write, what is hidden from it, its devices, caches and processes - and how to adjust it.
---

# The sandbox

Every command a task runs, whether you start it with
[`kule exec`](../cli/exec.md) or in the task's
[terminal](../app/terminals-and-servers.md), or an agent runs it, runs
inside the operating system's sandbox:

- **macOS**: Seatbelt, the system's own sandbox.
- **Linux**: Landlock, seccomp and namespaces.

If the sandbox cannot be set up, the command does **not** run. Kule never
falls back to less isolation without telling you (see
[Weaker isolation on Linux](#weaker-isolation-on-linux)).

## Where a command may write

Only to:

- the task's **copy**;
- the task's own `TMPDIR` and package caches;
- `/tmp` (on Linux each task has its own; on macOS tasks share the system's
  `/tmp`, because the macOS sandbox cannot move a path).

Everything else is read-only or hidden.

Writes to your **home directory** (`~/.zshrc`, `~/.config`, `git config
--global`) are not refused: they land in the task's
[home layer](home-layer.md) and wait for your review.

## What is hidden

A command cannot read:

- **credentials**: `~/.ssh`, `~/.aws`, `~/.npmrc`, cargo's registry token
  and the like;
- **your own files**: Documents, Desktop, Downloads, Pictures, Movies,
  Music, iCloud Drive and cloud storage folders;
- **the projects beside the workspace**: the folder your workspace lies in,
  when that folder is in your home (not the home itself, a dot folder or
  `~/Library`);
- **Kule's home** (`~/.kule`): other tasks, settings, secrets, the signing
  key;
- **other agents' logins and transcripts**.

Inside those hidden folders, a few things stay readable because tools need
them: the workspace's git directory (a worktree's repository next to it)
and the folders your `PATH` and SDK variables name.

## What a command gets from its environment

Only what tools need to find themselves is passed on. Tokens and agent
sockets in your environment stay out. To pass a variable on purpose:

```sh
kule exec TASK --pass-env MY_SETTING -- ./build.sh
```

For an API key, use a [secret](secrets.md) instead: the command gets a
placeholder, never the value.

## Projects a workspace builds from

A project often builds from one next to it: `path = "../shared"` in
`Cargo.toml`, `file:../ui` in `package.json`, `..\Shared\Shared.csproj`.
Those folders are hidden by default. To open one, **read-only**, to the
folder's tasks:

```sh
kule reveal add ../shared
```

Kule suggests the ones the folder's own manifests name (`kule reveal list`),
but only you open one. In the app: *Settings → Rules*, or the header's
"builds from" chip. The copy sits next to links to the projects you opened,
so `../shared` works from the copy as from the folder. See
[`kule reveal`](../cli/reveal.md).

## Package caches

npm, cargo, pip, uv, Go, NuGet, Yarn and pnpm get **caches of the task's
own**, so nothing a command leaves in a cache reaches a build you run later.
Your own caches stay read-only. On macOS the cargo and npm caches start as
clones of yours.

## Devices (Linux)

A command gets devices by name: `/dev/null`, `zero`, `full`, `tty`, its own
terminals, `random` and `urandom` to read, and the GPU's unless the GPU is
turned off. It never gets `/dev` as a whole, where a camera, microphone or
USB device would be. An agent that needs more names it under `devices` in
its profile; Kule refuses those that reach disks, memory, keyboard and
mouse, consoles and terminals.

## Processes and servers

On Linux, commands get processes of their own: a command sees and signals
no other program, and what it leaves running ends with it. A server started
in the background lives as long as the agent or terminal command that
started it.

Servers a task starts listen on **this computer only**, never on your
network. See [Terminals and servers](../app/terminals-and-servers.md).

## Programs that need the screen

A program that needs the window server cannot run in a task (LibreOffice,
even with `--headless`, is one). Live says so when it happens, and the agent
is told once.

## Adjusting the sandbox for one command

[`kule exec`](../cli/exec.md) takes these options:

| Option | Effect |
| --- | --- |
| `--allow-write PATH` | Also let the command write under `PATH` (repeatable). |
| `--hide PATH` | Also hide `PATH` (repeatable). |
| `--reveal PATH` | Let it read a path hidden by default, read-only (repeatable). |
| `--pass-env NAME` | Pass one environment variable on (repeatable). |
| `--no-network` | Shut TCP, localhost included. |
| `--open-network` | Connect anywhere, bypassing the proxy and policy. |
| `--no-sandbox` | No sandbox at all: the command can write anywhere you can. |

Agents get their sandbox from their **profile** (what state and login it
may use, its hosts, its keys). See [Agents](../using-agents.md).

## What the sandbox refused

On macOS, what the sandbox refused is printed after the command and kept in
the task's `sandbox.jsonl`:

```text
sandbox: refused file-read-data /Users/me/.config/gh/config.yml (gh)
sandbox: refused file-read-data /Users/me/.ssh/id_ed25519 (ssh)
```

The app shows refusals in [Timeline](../app/timeline.md). Linux reports
Landlock refusals only to the audit log, so they are not listed; the
command still gets a permission error.

## Weaker isolation on Linux

Full isolation on Linux needs unprivileged user namespaces, which Ubuntu
24.04 refuses by default. Without them, **Kule runs no agent** until you
accept weaker isolation, knowingly:

```sh
kule isolation                     # what is missing
kule isolation --accept-weaker on  # run with what this computer gives
```

In the app: *Settings → Policy*, *Run agents with what this computer
gives*. The title bar then says **Weaker isolation** while it holds. See
[`kule isolation`](../cli/isolation.md) and [Platforms](../platforms.md).

## Related

- [`kule exec`](../cli/exec.md), [`kule reveal`](../cli/reveal.md), [`kule isolation`](../cli/isolation.md)
- [Network and questions](network-and-questions.md)
- [Platforms](../platforms.md)
- [Security model](../security.md)
