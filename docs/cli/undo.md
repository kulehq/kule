---
title: kule undo
sidebar_label: kule undo
description: Revert a task's newest commit in the real folder and the copy, from the checkpoint it kept.
---

# kule undo

Revert the task's newest commit in the workspace and the copy.

## Synopsis

```sh
kule undo [OPTIONS] <TASK>
```

## Description

Every [kule commit](commit.md) keeps a **checkpoint**: what the commit
replaced in the workspace. `kule undo` puts that back, so the workspace is
as it was before the commit. Files the commit created are removed, files it
changed or deleted come back, and the copy is put back the same way.

Run it again to walk further back, one commit at a time, until there is
nothing left to undo.

Checkpoints live in the task's own store (git objects under
`checkpoints/store.git`, with modes and times beside them), inside Kule's
home where no sandbox can reach. Every undo is recorded in the task's signed
[history](history.md).

**Undo is refused when the workspace changed since the commit** at the paths
it would put back: undoing would overwrite someone's newer work. Put those
files back as the commit left them, or deal with them by hand.

In a git repository, git objects the commit added stay in `.git` (git
removes unreferenced objects itself with `git gc`); branch moves and the
index are put back.

Undo works per task: it takes back that task's commits only. Once a task is
deleted with [kule rm](rm.md), its checkpoints are gone.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Take the last commit back:**

```text
$ kule undo 3efe1ca8
undid checkpoint 1 (3 changes)
```

**Nothing left to undo:**

```text
$ kule undo 3efe1ca8
nothing to undo
```

**Walk back several commits:**

```sh
kule undo 3422f830 && kule undo 3422f830
```

**Refused because the folder changed since:**

```text
$ kule undo 3422f830
kule: the workspace changed since Kule last saw it, at: README.md
hint: roll back, or rerun the task on the current workspace
```

## Output

| Line | Meaning |
| --- | --- |
| `undid checkpoint C (N changes)` | The commit with checkpoint `C` is taken back; `N` paths were put back. |
| `nothing to undo` | The task has no commit left to take back. |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: the workspace changed since Kule last saw it, at: <paths>` | These files changed in the real folder after the commit. Kule will not overwrite them. Restore them by hand, or leave the commit in place. |
| `kule: workspace <path> is gone …` | The real folder was moved or deleted. |
| `kule: an agent is at work in task <id> …` | Wait for the run to end. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then undo` | Close the task's terminal (the app's tab, or `exit` in `kule shell`), then undo. |

## In the app

**Undo** on the newest commit in Timeline (Mod+Shift+U, press twice). See
[Timeline](../app/timeline.md).

## See also

- [kule commit](commit.md), [kule history](history.md), [kule rollback](rollback.md)
- [Review, commit and undo](../concepts/review-commit-undo.md)
