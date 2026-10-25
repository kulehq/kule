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
- on Linux, `/tmp`, which is the task's own. On macOS the sandbox cannot
  move a path, so `/tmp` would be the system's, shared with your other
  programs and tasks: commands do not write there, and `TMPDIR` (and zsh's
  `TMPPREFIX`) point at the task's own.

Everything else is read-only or hidden.

Writes to your **home directory** (`~/.zshrc`, `~/.config`, `git config
--global`) are not refused: they land in the task's
[home layer](home-layer.md) and wait for your review.

## What is hidden

A command cannot read:

- **credentials**: `~/.ssh`, `~/.aws`, `~/.npmrc`, cargo's registry token
  and the like;
- **your own files**: Documents, Desktop, Downloads, Pictures, Movies,
  Music, iCloud Drive and cloud storage folders, and everything else at the
  top of your home but its settings (a name starting with `.`, and
  `~/Library`): your other projects, whether Kule opened them or not, and
  your files there. A link at the top of your home is left as it is;
- **the projects beside the workspace**: the folder your workspace lies in,
  when that folder is in your home (not the home itself, a dot folder or
  `~/Library`);
- **every other project Kule opened**, wherever it lies, in your home or
  outside it, with the projects beside it as for the workspace's own;
- **where apps and tools keep settings and data**: `~/.config`,
  `~/.local/share`, `~/Library/Application Support` and
  `~/Library/Preferences`, an app's sign-in and a new tool's settings
  included. What toolchains, shells and prompts read there stays readable:
  the settings of git, jj, uv, pip, Poetry (not its `auth.toml`), Ruff,
  mise, Go, Flutter, Matplotlib, fish, zsh, Nushell, Starship and direnv,
  and what uv, mise, pnpm, fnm and pipx keep in `~/.local/share`, with man
  pages and fonts;
- **sign-ins of other tools**: GitHub Copilot, Firebase and the tools that
  keep their settings with it, Wrangler, Fly, Doppler, SOPS, Podman's
  `auth.json`, Yarn's and Bun's settings in your home, NuGet's own
  `NuGet.Config`;
- **editors' stores** of signed-in sessions and extension secrets (VS Code,
  its Insiders build, Cursor, Windsurf, VSCodium), and **saved shell
  sessions** (`~/.zsh_sessions`, `~/.bash_sessions`) beside the histories;
- **Kule's home** (`~/.kule`): other tasks, settings, secrets, the signing
  key;
- **other agents' logins and transcripts**.

Inside those hidden folders, a few things stay readable because tools need
them: the workspace's git directory (a worktree's repository next to it),
the folders your `PATH` and SDK variables name, and the folder a program on
your `PATH` leads to when it is a link (Claude Code's native install, uv's
tools). On macOS a program that reads preferences through the system keeps
to the same: a hidden app's preferences stay out of reach that way too.

To let one command read something hidden, run it with `--reveal PATH`
(see [`kule exec`](../cli/exec.md)): what Kule hides inside it stays
hidden.

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
started it. On every system, what an agent's run or `kule exec` left
running in the sandbox ends when it does, whatever became of its parent,
and a run's limits count it while it runs.

Kule says what it ended, by the names the processes ran under, where you
work:

- An agent's run: a note in Live, or in the output of `kule run`:
  `Kule ended <names>, which the agent left running in its sandbox, as the run ended`.
- `kule exec`: `kule: ended <names>, still running in the task's sandbox as the command ended`.
- `kule shell` and the app's terminal (macOS):
  `Kule ended <names>, which the shell left running in its sandbox`.
- A commit, a rollback, an undo, a sync, going back to a step, and applying
  or undoing home changes end what still runs in the task's sandbox before
  Kule writes in the copy. On the command line:
  `kule: Kule ended <name>, still running in the task's sandbox, to <what you asked>`,
  or `kule: Kule ended <N> processes still running in the task's sandbox, to <what you asked>: <names>`.
  In the app, the same words show as a notice and as a note in Live.

Each name is listed once, with how many ran under it (`sleep ×2, node`),
eight names at most and then how many more. A name is cut at 32
characters and shown as plain text.

Servers a task starts listen on **this computer only**, never on your
network. On macOS, where the sandbox cannot tell a server's address, a
task's programs listen only once you allow servers for it, which is weaker
isolation you accept. See [Terminals and servers](../app/terminals-and-servers.md).

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

Kule reads them as macOS's system log streams them, and checks the list
against a refusal of its own made after the commands'. Two things can keep
a refusal out of it, and Kule tells both, without changing the exit code:

- The log had not caught up when Kule stopped waiting for it, on a very busy
  computer: *some refusals may be missing from what is listed*.
- The log dropped some of the kernel's messages while the sandbox was in
  use. macOS does that every few seconds on a busy Mac, for whatever part
  of the system logs a lot, and does not say whose messages it dropped.
  Kule says when, in a quiet grey line: a refusal made just then would not
  be listed.

Either way the sandbox refused everything it refuses; only the list is
short.

The app shows refusals in [Timeline](../app/timeline.md), and a finished
run's line in [Live](../app/live.md) lists them too. Linux reports
Landlock refusals only to the audit log, so they are not listed; the
command still gets a permission error. Kule says so rather than show no
refusals: after an agent's run or a shell that failed, and in Live after
every run, it reads
`Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error`.

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

Without namespaces of its own (that, or a command run with the network
open), a command's seccomp filter refuses by name the calls that reach past
it: `ptrace`, `process_vm_writev`, `keyctl`, `userfaultfd`,
`perf_event_open` and `setns` fail with `EPERM`, whatever the host's
sysctls allow. A debugger or `strace` does not work inside the sandbox
then.

## Related

- [`kule exec`](../cli/exec.md), [`kule reveal`](../cli/reveal.md), [`kule isolation`](../cli/isolation.md)
- [Network and questions](network-and-questions.md)
- [Platforms](../platforms.md)
- [Security model](../security.md)
