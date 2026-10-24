---
title: kule rm
sidebar_label: kule rm
description: Delete a task, its copy and everything Kule kept for it, without touching the folder.
---

# kule rm

Delete a task and its copy. The workspace is not touched.

## Synopsis

```sh
kule rm [OPTIONS] <TASK>
```

## Description

`kule rm` deletes everything Kule keeps for the task under
`<home>/tasks/<id>/`: the copy, its uncommitted changes, steps, checkpoints,
logs, the home layer and the task's own package caches.

Your real folder (the workspace) is never touched, and changes you already
committed stay there. **Uncommitted changes in the copy are lost**, and so are
the checkpoints, so after `kule rm` you can no longer [undo](undo.md) the
task's commits with Kule. If you might want them, run `kule status` first and
commit what you want to keep.

Kule first moves the task to `<home>/trash`, then empties the trash right
away. The daemon empties it again whenever it starts, so a deletion that was
cut short is finished later. The disk space comes back once the trash is
empty.

A task an agent is working in cannot be deleted; stop the run first.

A task whose record (`task.json`) cannot be read can be deleted too, by its
id. While it is there, no command or run starts in any task, since Kule
cannot tell which folder that task hides from the others' commands; see
[Troubleshooting](../troubleshooting.md#nothing-starts-a-tasks-record-cannot-be-read).

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Delete a task:**

```text
$ kule rm 40ac70b8
removed task 40ac70b8
```

**Check for uncommitted work first:**

```sh
kule status 40ac70b8 && kule rm 40ac70b8
```

**Delete every task on one folder:**

```sh
kule list | awk '$2 == "/Users/me/code/app" {print $1}' | xargs -n1 kule rm
```

## Output

`removed task <id>` when it is gone.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: no task <id>` | Use the full id from `kule list`. |
| `kule: an agent is at work in task <id> (process <pid>); wait until it is done` | Wait for the run to end, or stop it (Stop in the app). |
| `kule: task <id> is busy: another kule command holds its lock` | Another `kule` command (a commit, say) is using the task. Try again when it is done. |

## In the app

*Delete…* in the task's ⋮ menu, or Mod+Backspace (press twice).

## See also

- [kule new](new.md)
- [Tasks and copies](../concepts/tasks-and-copies.md): what a task takes on disk.
