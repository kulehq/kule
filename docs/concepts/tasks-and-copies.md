---
title: Tasks and copies
sidebar_label: Tasks and copies
description: What a task is, how its copy-on-write copy is made, what it costs on disk, and how to name, order, fork and delete tasks.
---

# Tasks and copies

## What a task is

A **task** is one piece of work on one folder. When you make one, Kule:

1. gives it an id of 8 hex characters, like `1a2b3c4d`;
2. makes a **copy** of the folder (the **workspace**) under Kule's home;
3. keeps, beside the copy, everything the task needs: temporary files,
   package caches, a home layer, steps and checkpoints.

The agent works only in the copy. The workspace is only read, until you
commit, [sync](sync-and-merging.md) or undo.

```sh
kule new ~/code/app
```

```text
task 1a2b3c4d · copy at ~/.kule/tasks/1a2b3c4d/w/app
```

In the app: **New task…** (Mod+N) and pick a folder.

Kule needs **no git**. Any folder works: code, a Markdown or Obsidian vault,
photos, downloads, CSV files, an installer you do not trust yet. When the
folder is a git repository, Kule also understands `.git` (see
[Git repositories](git.md)).

## How the copy is made

| Where | How | Disk space |
| --- | --- | --- |
| macOS (APFS) | One APFS clone of the folder | Almost none until files change |
| Linux, Btrfs or XFS with reflink | File by file, with reflinks (`FICLONE`) | Only what changes |
| Linux, ext4 and others without reflinks | A plain copy | The folder's full size, `.git` and `node_modules` included |

:::note Disk tools count clones in full

Finder, `du` and disk-usage tools count every clone at its full size, so
`~/.kule` can look like many gigabytes. Look at the **free space** (`df -h`,
or System Settings → General → Storage) to see what is really used. On
Btrfs, `btrfs filesystem du` tells the truth.

:::

Clones are made only on the same volume. A folder on another disk is
copied in full.

## What a task keeps

Everything is under `~/.kule/tasks/<id>/`:

| Folder | What it is |
| --- | --- |
| `w/<folder name>` | The copy the agent works in. |
| `base/` | The folder as it was at the last sync, used to merge. |
| `tmp/` | The task's temporary files. `tmp/caches/` holds its own package caches (npm, cargo, pip, Go and others). |
| `home/` | The [home layer](home-layer.md): what the task's commands see as your home. |
| `steps-store/`, `checkpoints/` | What going back to a step and undo need. |

On macOS, the task's npm and cargo caches start as clones of yours, so a
task's first install does not download again. On Linux they start empty.
A clone of large caches takes a few seconds, so the app makes it in the
background as soon as a task is made or forked. A command, terminal or run
that comes before it is done waits for it and says so: *Kule is still
preparing this task's package caches*. On the command line, the task's
first command makes them itself and says *preparing this task's package
caches*.

What really takes space: files the agent changes or creates (build output
too), packages a task downloads, and the old contents that steps and
checkpoints keep.

## Ignored paths

Paths the workspace's `.gitignore` excludes (build output, caches, `.env`)
stay in the copy. A commit leaves them behind unless you ask:
`kule commit --include-ignored`, or *Also commit these* in the app. The
ignore rules come from the workspace, never from the copy.

## Several tasks

One agent works in a task at a time. To run agents side by side, make more
tasks, on the same folder or on others.

Tasks on the same folder never see each other's work. Kule tells you
**where they meet** while both are still going: the same function, section
or key, or the same file when it cannot look inside. `kule status` lists it
(`task 5e6f7a8b changes them too: task.rs · method Task::commit`), and the
app marks the task and shows it under Review. See
[Sync and merging](sync-and-merging.md) for what happens when both commit.

## Organising tasks

| What | Command line | App |
| --- | --- | --- |
| List tasks, in the app's order | [`kule list`](../cli/list.md) | The Tasks panel |
| Name a task (`Downloads – march invoices`) | [`kule name`](../cli/name.md) | ⋮ menu → *Rename…* (F2) |
| Colour a task | [`kule color`](../cli/color.md) | ⋮ menu → swatches |
| Move a task in the list | – | Drag, or Alt+Shift+↑/↓ |
| Put a task in a group | [`kule group put`](../cli/group.md) | Drag onto a group, or *Group…* |
| Fork a task | – | ⋮ menu → *Fork task* (Mod+Shift+D), or Timeline → *Fork from here* |
| Delete a task | [`kule rm`](../cli/rm.md) | ⋮ menu → *Delete* |

**Forking** makes a new task on the same folder that starts from this
task's copy (as it is now, or as it was after a step). The fork takes the
task's name with "(fork)", its colour, group, budget and secret stand-ins,
and has no steps or commits of its own. Use it to try another direction
without losing this one.

**Deleting** a task never touches your folder. The task moves to
`~/.kule/trash`, which Kule empties in the background; then its space comes
back.

## Related

- [`kule new`](../cli/new.md), [`kule list`](../cli/list.md), [`kule rm`](../cli/rm.md), [`kule path`](../cli/path.md)
- [The window](../app/window.md)
- [Groups](groups.md)
