---
title: kule exec
sidebar_label: kule exec
description: Run one command in the task's copy, inside the sandbox and behind the task's proxy, then list what it changed.
---

# kule exec

Run a command inside the task's copy, in the sandbox, then show what it
changed.

## Synopsis

```sh
kule exec [OPTIONS] <TASK> -- <COMMAND>...
```

## Description

`kule exec` is `kule run` without the agent: you name the command. It is how
you build, test or try something in a task, and how you run a script or an
installer you do not trust yet to see what it would change.

The command runs in the task's copy (its working directory is the copy),
inside the operating system's sandbox: Seatbelt on macOS, Landlock and
namespaces on Linux. By default:

- **It can write** only to the copy, the task's own `TMPDIR`, its package
  caches, and on Linux `/tmp`, which is the task's own (on macOS `/tmp` is
  the system's, so it stays closed).
- **It listens** for connections on this computer only. On macOS, where the
  sandbox cannot tell a server's address, it does not listen at all while
  its network goes through the proxy, unless the task's `servers`
  [limit](limits.md) allows it.
- **It cannot read** your credentials (`~/.ssh`, `~/.aws`, `~/.npmrc`,
  cargo's registry token and many more), your own folders (Documents,
  Desktop, Downloads, Pictures, cloud storage), Kule's home, or the projects
  beside the workspace.
- **Its home** is the task's home layer: what it writes in `~/.zshrc` or
  `~/.config` is caught for review with [kule home](home.md).
- **Its network** goes through the task's proxy. Package registries pass;
  anything else the policy does not let through is refused, since the CLI
  has nobody to ask. Every connection is logged.
- **Its environment** holds only what tools need to find themselves.
  Tokens and agent sockets in your environment are left out.
- `KULE_TASK` holds the task's id.

If the sandbox cannot be set up, the command does not run.

When the command ends, Kule prints the copy's changes (all of them, not only
this command's), the connections it made, home writes, and on macOS what the
sandbox refused. `kule exec` exits with the command's own exit code.

`exec` does not make steps; steps are an agent's tool calls. Commit and
rollback are refused while an `exec` runs in the task.

See [The sandbox](../concepts/sandbox.md) for the full list of what is
hidden and why.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |
| `COMMAND...` | yes | The program and its arguments, after `--`. It is run directly, not through a shell; use `sh -c '…'` for pipes, `&&` or globs. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--allow-write` | path | | Also let the command write under this path. Repeatable. |
| `--hide` | path | | Also hide this path from the command. Repeatable. |
| `--reveal` | path | | Let the command read a path that is hidden by default, such as `~/Library/Keychains` for an agent's own login, or a project beside the workspace (read-only). Repeatable. Kule's home and the task's own folder are never revealed. |
| `--pass-env` | name | | Pass this environment variable on to the command. Repeatable. |
| `--no-network` | | | Shut TCP networking for the command, localhost included. |
| `--open-network` | | | Let the command connect anywhere, bypassing the proxy and the policy. |
| `--no-sandbox` | | | Run without the sandbox. The command can write anywhere you can and reach any host; the options above mean nothing then. |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

:::warning

`--reveal`, `--allow-write`, `--open-network` and `--no-sandbox` open holes
that Kule closes on purpose. Use them for a command you trust, and as
narrowly as you can: `--reveal` one folder rather than its parent.

:::

## Examples

**Run the tests:**

```sh
kule exec 3422f830 -- cargo test
```

**See what a command changed:**

```text
$ kule exec 3efe1ca8 -- sh -c 'echo notes > NOTES.md; ./update-main.sh'
2 file changes: +1 ~1 -0
  + NOTES.md
  ~ src/main.rs
      ~ function main
      + function wave
```

**What the sandbox refuses (macOS):**

```text
$ kule exec 3efe1ca8 -- sh -c 'ls ~/.ssh; echo hi > ~/notes.txt'
ls: /Users/me/.ssh: Operation not permitted
sh: /Users/me/notes.txt: Operation not permitted
sandbox: refused file-read-metadata /Users/me/.ssh (ls)
sandbox: refused file-write-create /Users/me/notes.txt (bash)
```

**The network goes through the proxy:**

```text
$ kule exec 3efe1ca8 -- sh -c 'curl -sI https://registry.npmjs.org/ >/dev/null; curl -sI https://example.com/'
net: registry.npmjs.org:443 · tunneled (package-registries)
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
```

To let a host through every time, see [kule rules allow](rules.md).

**No network at all:**

```sh
kule exec 3422f830 --no-network -- ./build.sh
```

**Try an installer you do not trust yet:**

```sh
kule exec 3422f830 -- sh ./install.sh
kule status 3422f830     # what it would have changed
kule home 3422f830       # what it tried to write in your home
kule rollback 3422f830   # throw it all away
```

**Build against a project beside the folder:**

```sh
kule exec 3422f830 --reveal ../shared -- npm run build
```

To open it for every task on the folder, use [kule reveal add](reveal.md).

**Run a headless agent CLI yourself** (instead of `kule run`):

```sh
kule exec "$TASK" --reveal ~/Library/Keychains \
  --allow-write ~/.claude --allow-write ~/.claude.json \
  -- claude -p "Fix the failing test, run cargo test." \
     --allowedTools "Bash,Read,Edit,Write"
```

## Output

| Line | Meaning |
| --- | --- |
| the command's own output | Printed as it runs. |
| `N file changes: +a ~m -d` and the list | Everything the copy holds that the workspace does not, as [kule status](status.md) shows it. |
| `N ignored by the workspace, staying in the copy …` | Paths the workspace's `.gitignore` excludes (build output, `.env`). A commit leaves them behind unless you pass `--include-ignored`. |
| `git:` | Repository changes. See [Git](../concepts/git.md). |
| `net: <host>:<port> · tunneled (<rule>)` | A connection the policy let through, and the rule that did. |
| `net: <host>:<port> · refused, …` | A connection the policy did not let through. |
| `sandbox: refused <operation> <path> (<program>)` | What the sandbox refused, on macOS. They are also kept in the task's `sandbox.jsonl`. |
| `kule: running without the sandbox` | You passed `--no-sandbox`. |
| `kule: network open, bypassing the proxy and the policy` | You passed `--open-network`. |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `Operation not permitted` from the command | The sandbox refused it. Check the `sandbox:` lines; if the command really needs that path, use `--reveal` or `--allow-write` for it. |
| a `net: … refused` line and a failed download | Let the host through with [kule rules allow](rules.md), or use the app, where the connection waits for your answer. |
| `kule: could not run <program>` | The program is not on the `PATH` the command sees, or is not executable. |
| `kule: an agent is at work in task <id> …` | Wait for the agent's run to end. |
| `kule: a terminal is open on task <id>'s copy …; close it first, then start work` | A terminal on the task (the app's, or `kule shell`) is open. Close it first. |
| `kule: Kule opened task <id>'s copy in <app> outside the sandbox; …` | Close that terminal app or editor, then run [kule closed](closed.md) `TASK`. |

On Linux without unprivileged user namespaces, see
[kule isolation](isolation.md).

## In the app

The app has no field for a single command. Open the task's **Terminal**
(Ctrl+\`) to run commands yourself: it runs in the task's sandbox, like
`kule exec` and [kule shell](shell.md). `kule shell --outside` and the app's
*Open in terminal app* open the copy outside the sandbox.

## See also

- [The sandbox](../concepts/sandbox.md), [Network and questions](../concepts/network-and-questions.md)
- [kule run](run.md), [kule status](status.md), [kule reveal](reveal.md), [kule rules](rules.md)
