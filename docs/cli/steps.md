---
title: kule steps
sidebar_label: kule steps
description: List the steps of a task's uncommitted work, one per agent tool call, with the files each changed.
---

# kule steps

List the steps of the task's uncommitted work: what each tool call changed.

## Synopsis

```sh
kule steps [OPTIONS] <TASK>
```

## Description

While an agent works (see [kule run](run.md)), every tool call it makes, such
as editing a file or running a command, becomes a **step**. Kule keeps what
the files held before each step, so you can see the agent's work piece by
piece and, in the app, go back to any step or fork a new task from it.

`kule steps` lists the steps since the last commit, oldest first, and the
files each changed. [kule status](status.md) shows the sum of all of them.

Steps start over after a commit, a rollback, an undo or a
[sync](sync.md). Commands you run with `kule exec` or in `kule shell` are not
steps; their changes still show in `kule status`.

It only reads; nothing changes.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**After an agent run** (the titles are the agent's own):

```text
$ kule steps 3422f830
  1. Edit src/main.rs
       ~ src/main.rs
  2. Write tests/greet.rs
       + tests/greet.rs
  3. cargo test
       + target/debug/app
       ~ Cargo.lock
```

**After going back to a step in the app:** the going-back is a step of its
own, and the files it put back are marked `↺`.

**No agent has run since the last commit:** `kule steps` prints nothing.

## Output

| Line | Meaning |
| --- | --- |
| `N. <title>` | The step's number and the tool call's title, as the agent named it. |
| `+ path`, `~ path`, `- path` | A file the step created, modified or deleted. |
| `↺ path` | A file put back as it was, by going back to a step. |

## In the app

The **Timeline** panel (Mod+3) lists the steps, with *Go back here* and *Fork
from here* on each. Live shows each step as it happens. See
[Timeline](../app/timeline.md).

## See also

- [kule run](run.md), [kule status](status.md)
- [Timeline](../app/timeline.md)
