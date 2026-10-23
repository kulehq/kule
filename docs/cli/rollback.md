---
title: kule rollback
sidebar_label: kule rollback
description: Throw away a task's uncommitted changes, so the copy matches the folder again.
---

# kule rollback

Throw the task's uncommitted changes away.

## Synopsis

```sh
kule rollback [OPTIONS] <TASK>
```

## Description

`kule rollback` puts the task's copy back to match the workspace as of the
last commit or sync. Everything the agent, `kule exec` or you changed in the
copy since then is gone: created files are removed, modified and deleted
ones come back, moves are undone, and git changes in the copy's `.git` are
reverted.

The workspace is not touched; there is nothing to take out of it, since
uncommitted changes never reached it.

Steps start over. The task itself stays, ready for the next run.

Use it when you do not like the work, or after trying something you do not
trust in [kule exec](exec.md). To take back changes that were already
**committed**, use [kule undo](undo.md) instead.

:::warning

A rollback cannot be undone. If part of the work is worth keeping, commit
that first, or copy it out of the copy (`kule path TASK`).

:::

Writes caught in the task's home layer are not part of a rollback; use
[kule home](home.md) `discard` for those.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Throw the work away:**

```text
$ kule status 3efe1ca8
2 file changes: +1 ~0 -1
  - README.md
      - section App
  + scratch.txt
policy: passes on its own
$ kule rollback 3efe1ca8
rolled back 2 changes
$ kule status 3efe1ca8
no changes
```

**Try an installer, then forget it:**

```sh
kule exec 3422f830 -- sh ./install.sh
kule status 3422f830
kule rollback 3422f830
```

**Nothing to roll back:**

```text
$ kule rollback 3efe1ca8
rolled back 0 changes
```

## Output

`rolled back N changes`, where `N` counts every path put back, including
those inside moved folders and `.git`.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: an agent is at work in task <id> …` | Wait for the run to end, or stop it. |
| `kule: task <id> is busy: another kule command holds its lock` | Wait for the other command. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then roll back` | Close the task's terminal (the app's tab, or `exit` in `kule shell`), then roll back. What still runs in the task's sandbox is ended first, so nothing changes the copy while it is put back. |

## In the app

**Roll back** under Review (Mod+Shift+Backspace, press twice). See
[Review](../app/review.md).

## See also

- [kule commit](commit.md), [kule undo](undo.md)
- [Review, commit and undo](../concepts/review-commit-undo.md)
