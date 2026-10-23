---
title: Timeline, steps and forks
sidebar_label: Timeline
description: A task's history of commits, steps, connections and sandbox refusals, and how to go back to a step or fork a task from it.
---

# Timeline: steps, going back and forking

**Timeline** (Mod+3) is the task's history. It answers "what happened in
this task, and in what order".

## What it lists

| Entry | What it is |
| --- | --- |
| **Commits** | Newest first, each with what it applied. The newest has **Undo**. Every commit and undo is also an entry in the task's signed history (`kule history TASK`). |
| **Steps** | The uncommitted tool calls, each with the files it changed. A commit starts the steps over. |
| **Connections** | The network connections the task's commands made, and whether each was let through, asked about or refused. |
| **Refusals** | What the sandbox refused the commands, under *Refused by the sandbox · N refusals*, the total. The same refusal is one row, with how many times (`×3`); the newest 50 rows show, and *… and N more refusals, older* says how many are left out. macOS reports them; on Linux the command sees the error but Kule cannot list it. |

## Undo a commit

**Undo** (Mod+Shift+U, press twice) reverts the task's newest commit in your
folder and in the copy, putting back what the commit replaced from the
task's checkpoint. Repeat it to walk further back. See
[Review, commit and undo](../concepts/review-commit-undo.md).

## Go back to a step

Every step, and "before the first step", has two actions. The first is
**Go back here**: it puts the copy back as it was after that step.

- Going back is itself a step, so you can go back on it.
- Changes made since the last step are kept as a step first, so nothing is
  lost.
- A step that cannot be gone back to says why: a file over 64 MB, more than
  1 GB in one go, or a file with a second name (a hard link).
- `.git` is not put back. A commit starts the steps over.
- What still runs in the task's sandbox is ended first, and Kule names it
  in a notice and in Live.

## Fork a task

The second action is **Fork from here**: it makes a **new task** on the same
folder that starts from this one's copy as it was after that step. *Fork
task* in the task's ⋮ menu (Mod+Shift+D) forks from the copy as it is now.

The fork takes the task's name with "(fork)", its colour, group, budget and
secret stand-ins, and has no steps or commits of its own. Its changes reach
the folder through its own review and commit.

:::tip When to fork

Fork when you want to try another direction without losing this one, or to
run two agents on two ideas and keep the better result.

:::

## From a terminal

[`kule steps`](../cli/steps.md) lists the steps of the uncommitted work,
[`kule undo`](../cli/undo.md) reverts the newest commit, and
[`kule history`](../cli/history.md) prints the signed history, checked entry
by entry. Going back to a step and forking are in the app only.
