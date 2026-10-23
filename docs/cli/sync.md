---
title: kule sync
sidebar_label: kule sync
description: Bring what the real folder changed since the task's last sync into its copy, merging files both changed.
---

# kule sync

Bring what the workspace changed since the task's last sync into its copy.

## Synopsis

```sh
kule sync [OPTIONS] <TASK>
```

## Description

A task's copy does not follow the workspace by itself. The agent works on the
folder **as it was** when the task was made, or when it was last synced. If
you edit a file in the real folder meanwhile, or another task commits there,
the copy does not see it. [kule status](status.md) lists these files under
*the workspace changed since the task's copy*.

`kule sync` brings them in:

- **A file only the workspace changed** comes in as it is.
- **A file the task changed too** is merged by function, method, section or
  key, and stays the task's change (so it shows in `kule status`).
- **When anything does not merge,** nothing comes in at all, and Kule names
  what both changed. Your copy stays as it was.
- **Commits, branches and staged files** of the folder's git repository come
  in too. A branch both moved does not.
- **Settings files** such as `.env` come in masked, like the rest of the copy
  (see [Secrets](../concepts/secrets.md)).

The workspace is only read. After a sync, steps start over, as after a
commit, and the agent hears what came in at the start of its next turn.

Kule refuses a sync while an agent is at work, and while a terminal is
open on the task (the app's, or [kule shell](shell.md)); programs still
running in the task's sandbox are stopped first, and Kule names them.

See [Sync and merging](../concepts/sync-and-merging.md) for how the merge
works.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Nothing to bring in:**

```text
$ kule sync 3efe1ca8
the copy has everything the workspace changed
```

**A file only the folder changed, and one both changed, merged:**

```text
$ kule status 3efe1ca8
…
the workspace changed since the task's copy (kule sync 3efe1ca8 brings it in):
  README.md
  src/main.rs (the task changed it too)
$ kule sync 3efe1ca8
  < README.md
  ~ src/main.rs (merged with the task's change)
```

**The folder's new git commits:**

```text
$ kule sync 3efe1ca8
  < the repository (commits, branches, the index)
```

**Both changed the same function:**

```text
$ kule sync 3efe1ca8
kule: the folder and the task both changed: src/main.rs (both changed function greet); nothing was brought in
hint: commit or roll back the task's changes there first, or leave the copy as it is
```

## Output

| Line | Meaning |
| --- | --- |
| `< path` | Brought in as the workspace has it. |
| `~ path (merged with the task's change)` | Both changed it, in different places; merged. |
| `< the repository (commits, branches, the index)` | The folder's git state came in. |
| `= path (the task's own kept: both changed it)` | Kept as the task has it. |
| `the copy has everything the workspace changed` | Nothing to do. |
| `kule: Kule ended <name>, still running in the task's sandbox, to sync` | On stderr: what still ran in the task's sandbox, which Kule ended before it wrote in the copy. For more than one: `kule: Kule ended <N> processes still running in the task's sandbox, to sync: <names>`. See [Processes and servers](../concepts/sandbox.md#processes-and-servers). |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: the folder and the task both changed: <paths>; nothing was brought in` | Both sides changed the same function, section or key. Commit or roll back the task's change there first, or leave the copy as it is and resolve it at commit time. |
| `kule: an agent is at work in task <id> …` | Wait for the run to end. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then sync` | Close the task's terminal (the app's tab, or `exit` in `kule shell`), then sync. |

## In the app

*Bring into the copy* under Review's *Changed in the folder*, in the task's
⋮ menu, or Mod+Shift+G. See [Review](../app/review.md).

## See also

- [Sync and merging](../concepts/sync-and-merging.md)
- [kule status](status.md), [kule commit](commit.md)
