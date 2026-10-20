---
title: kule daemon
sidebar_label: kule daemon
description: Run the Kule daemon, which owns agent runs and the questions they wait on, in the foreground.
---

# kule daemon

Run the Kule **daemon** in the foreground. It owns agent runs and the
questions they wait on, for the desktop app.

## Synopsis

```sh
kule daemon [OPTIONS]
```

## Description

The desktop app is a window onto the daemon. The daemon owns the tasks, the
agents' runs and the questions they wait on, so:

- closing the window stops nothing: a run goes on, and its questions wait
  for their time;
- reopening the window shows what happened meanwhile.

**You normally never run it yourself.** The app starts it when none is
running (detached, writing its output to `kuled.log` in Kule's home). Run it
by hand to watch its output while you debug, or to keep it running for a
session.

How it behaves:

- **One per home.** It takes a lock (`kuled.lock`); a second daemon for the
  same home refuses to start: `a Kule daemon is already running for this
  home`.
- **Only you can talk to it.** It listens on `kuled.sock` in Kule's home. The
  home is made readable by you alone, the socket checks the peer's user id,
  and every request must carry a fresh token from `kuled.token`, which no
  sandbox can read.
- **It exits when idle**: after `--idle-secs` (ten minutes by default) with no
  client connected, no agent at work and no question waiting.
- On start, it empties `trash/`, where deleted tasks wait to be removed.

Some `kule` commands (for example `kule ext install`) go through the daemon
when one is running, so the app hears about the change at once.

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--idle-secs` | seconds | `600` | Exit after this many seconds with no client, no agent at work and no question waiting. |
| `--home` | path | `~/.kule` (or `KULE_HOME`) | Kule's home. See [global options](index.md). |
| `-h`, `--help` | | | Print help. |

## Examples

Run it in the foreground for the default home:

```console
$ kule daemon
kule: daemon for /Users/you/.kule
```

It prints that line on standard error and then stays quiet until it exits.

Keep it up for an hour of idle time:

```console
$ kule daemon --idle-secs 3600
```

A second home, for experiments, with its own daemon:

```console
$ KULE_HOME=~/kule-test kule daemon
kule: daemon for /Users/you/kule-test
```

Read what an app-started daemon wrote:

```sh
tail -f ~/.kule/kuled.log
```

## Errors and fixes

| Message | What to do |
| --- | --- |
| `a Kule daemon is already running for this home` | One is running already (the app's, perhaps). Use it, or stop it first. |
| `socket path …/kuled.sock is longer than 100 bytes; use a shorter KULE_HOME` | Unix sockets have a short path limit. Put Kule's home on a shorter path. |
| `cannot reach the Kule daemon at …/kuled.sock: …` (from the app or a command) | No daemon is running for that home, or it just exited. The app starts one again on its own. |

The daemon exits with status 2 on an error.

## In the app

The app starts and stops the daemon for you. The window can be closed while
agents work.

## See also

- [The window](../app/window.md)
- [Files and settings](../reference/files.md): `kuled.sock`, `kuled.lock`, `kuled.token`, `kuled.log`.
- [Environment variables](../reference/environment.md): `KULE_HOME`.
