---
title: kule isolation
sidebar_label: kule isolation
description: Show what this computer cannot give a sandboxed command, and accept or refuse running with less.
---

# kule isolation

Show what this computer cannot give a sandboxed command, and choose whether
commands may run with less.

## Synopsis

```sh
kule isolation [OPTIONS]
```

## Description

Kule never runs a command **less isolated than it means to** without your
say. On some computers part of the [sandbox](../concepts/sandbox.md) is not
available. The common case is Linux without unprivileged user namespaces
(Ubuntu 24.04 refuses them by default), which Kule uses to give each command
a network, a `/tmp`, processes and shared memory of its own.

While anything is missing and you have not accepted it, Kule refuses to run
commands and agents, and says what is missing. `kule isolation` shows the
gaps, and `--accept-weaker on` lets commands run with what the computer
gives. Your choice is kept in Kule's home (`isolation.json`).

On macOS the one gap is the task's servers, where the `servers`
[limit](limits.md) lets them listen: the macOS sandbox sees the port a
server listens on but not its address. Kule points Node, Python and Ruby
servers at this computer, and keeps the others off your network from
outside the sandbox while an agent runs in the app; a server of another
program started by `kule exec`, `kule run` or a terminal could listen on
your network. `kule isolation` shows it when every task's limits
allow servers; a task or group that allows them on its own is told when it
runs.

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--accept-weaker` | `on` or `off` | `off` | `on` runs commands with what this computer gives; `off` refuses them while anything is missing. |
| `--home` | path | `~/.kule` (or `KULE_HOME`) | Kule's home. See [global options](index.md). |
| `-h`, `--help` | | | Print help. |

## Examples

A computer that gives everything (macOS, or Linux with user namespaces and a
recent Landlock):

```console
$ kule isolation
this computer gives a sandboxed command all of its isolation
```

A Linux computer without unprivileged user namespaces:

```console
$ kule isolation
this computer cannot give a sandboxed command:
  - shared memory of its own: it could open the System V segments and queues of your other programs
  - a network of its own: only the proxy's port is checked, on any address, and datagrams pass
  - agents' settings kept read-only while it runs: they are put back after it instead
  - a /tmp of its own: it shares /tmp with your other programs and tasks
  - processes of its own: what it leaves running is found by a guess, which one could slip
not accepted: commands are refused (kule isolation --accept-weaker on)
```

The exact lines depend on the kernel: which namespaces it allows and which
Landlock version it has.

Accept it:

```console
$ kule isolation --accept-weaker on
this computer cannot give a sandboxed command:
  ...
accepted: commands run with what it gives
```

Go back to refusing:

```console
$ kule isolation --accept-weaker off
```

## Better: give the computer what Kule needs

On Ubuntu 24.04 and later, allow unprivileged user namespaces instead of
accepting less:

```sh
sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0
```

To keep it after a reboot, put the same setting in a file under
`/etc/sysctl.d/`. Network rules also need Linux 6.7 or later (Landlock ABI 4).
See [Platforms](../platforms.md).

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: maybe: on or off` | `--accept-weaker` takes `on` or `off`. |
| `this computer cannot give the command …. Kule does not run it less isolated without your say: accept weaker isolation in Settings → Policy, or with kule isolation --accept-weaker on` (from `exec` or `run`) | Allow user namespaces as shown above, or accept weaker isolation knowingly. |

## In the app

*Settings → Policy* lists what is missing, and no agent runs until you tick
**Run agents with what this computer gives**. While that holds, the title bar
says **Weaker isolation**.

## See also

- [Sandbox](../concepts/sandbox.md), [Platforms](../platforms.md), [Troubleshooting](../troubleshooting.md)
