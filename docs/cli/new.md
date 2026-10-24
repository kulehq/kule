---
title: kule new
sidebar_label: kule new
description: Make a task, a copy-on-write copy of a folder that agents and commands work in.
---

# kule new

Make a task: a copy-on-write copy of a folder, where agents and commands
work without touching the real folder.

## Synopsis

```sh
kule new [OPTIONS] <WORKSPACE>
```

## Description

A **task** is Kule's unit of work. `kule new` takes a folder (the
**workspace**) and makes a copy of it inside Kule's home, under
`<home>/tasks/<id>/w/<folder name>`. Everything you do with the task happens
in that copy: `kule run` and `kule exec` change it, `kule status` compares it
with the workspace, and only `kule commit` writes the changes back.

The copy is made with **reflinks** where the filesystem has them: APFS
clones on macOS, `FICLONE` on Btrfs and XFS on Linux. A reflinked copy shares
its disk blocks with the original, so it is quick even for a large folder
and takes no space until files change. On a filesystem without reflinks
(ext4, for example) Kule falls back to a plain copy, which takes the folder's
full size. See [Tasks and copies](../concepts/tasks-and-copies.md).

The folder can be anything: a git repository, a notes vault, a folder of
photos or invoices. Kule needs no git. When the folder is a git repository,
Kule also understands its `.git` (see [Git](../concepts/git.md)).

When the task is made, Kule also:

- masks values in the copy's `.env` files that look like secrets (see
  [Secrets](../concepts/secrets.md));
- looks at the folder's manifests (`Cargo.toml`, `package.json`, `.csproj`,
  …) for projects beside it that it builds from, such as `../shared`. Kule
  hides the projects next to a workspace from the sandbox, so it tells you
  about each one and how to open it, read-only, with
  [`kule reveal add`](reveal.md).

You can make as many tasks as you like, on the same folder or on others.
Tasks never see each other's work; Kule tells you where they change the same
things.

`kule new` does not touch the workspace.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `WORKSPACE` | yes | The folder to copy. A relative path is taken from the current directory. Links are followed to the real folder. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Make a task on a repository:**

```text
$ kule new ~/code/app
task 3422f830 · copy at ~/.kule/tasks/3422f830/w/app
```

**Make a task on a folder that is not a repository:**

```text
$ kule new ~/notes
task 40ac70b8 · copy at ~/.kule/tasks/40ac70b8/w/notes
```

**Keep the id in a script:**

```sh
TASK=$(kule new ~/code/app | awk '{print $2}')
kule exec "$TASK" -- npm test
```

**A folder that builds from a project beside it:**

```text
$ kule new ~/code/web
task 5e6f7a8b · copy at ~/.kule/tasks/5e6f7a8b/w/web
builds from /Users/me/code/shared, which its tasks cannot read: kule reveal add '/Users/me/code/shared'
```

## Output

| Line | Meaning |
| --- | --- |
| `task <id> · copy at <path>` | The new task's id (use it with every other command) and where its copy is. |
| `builds from <path>, which its tasks cannot read: kule reveal add '<path>'` | A project the folder's manifests name, hidden from the sandbox. Run the command shown to let this folder's tasks read it. |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: <path>: No such file or directory (os error 2)` | The folder does not exist. Check the path. |
| `kule: <path> is not a directory` | Pass a folder, not a file. |
| `kule: Kule's home <home> is inside the workspace, so the copy would contain itself` | You asked for a copy of a folder that holds Kule's home (your whole home folder, for example). Pick a folder inside it, or move Kule's home with `--home`. |
| `kule: Kule's home <~/.kule> is inside the workspace, though this task's home is another` | The folder holds Kule's home where it is by default, even with `--home` or `KULE_HOME` pointing elsewhere: its tasks and secrets would be copied in. Pick a folder inside it. |
| `kule: <folder> holds more than 1000000 files and folders, more than Kule reads; …` | Pick a smaller folder. A file over 8 GiB is refused the same way when Kule hashes it for a review. |

## In the app

Click **New task…** (Mod+N) and pick a folder. With no task yet, the first-run
screen asks for one. See [Your first task in the app](../getting-started/first-task-in-the-app.md).

## See also

- [Tasks and copies](../concepts/tasks-and-copies.md)
- [kule list](list.md), [kule rm](rm.md)
- [kule reveal](reveal.md)
