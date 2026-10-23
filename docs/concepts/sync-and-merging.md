---
title: Sync and merging
sidebar_label: Sync and merging
description: How a task brings in what its folder changed meanwhile, and how Kule merges two tasks' work by function, section or key.
---

# Sync and merging

## The copy does not follow the folder by itself

A task's copy is the folder **as it was at the last sync** (when the task
was made, or the last time you synced). If you edit the folder in your
editor meanwhile, or another task commits to it, the agent does not see it.

`kule status` and the app's Review say when the folder changed:

```text
the workspace changed since the task's copy (kule sync 1a2b3c4d brings it in):
  hello.txt
```

## Sync: bring it in

```sh
kule sync TASK
```

```text
  < hello.txt
```

In the app: Review → **Bring into the copy** (Mod+Shift+G), or the task's
⋮ menu.

What sync does:

- a file only the folder changed comes in as it is;
- a file the task changed too is **merged** (see below) and stays the
  task's change;
- when anything does not merge, **nothing** comes in, and Kule names what
  both changed:

  ```text
  kule: the folder and the task both changed: hello.txt (both changed lines 1–3); nothing was brought in
  hint: commit or roll back the task's changes there first, or leave the copy as it is
  ```

- commits, branches and staged files in the folder's git repository come in
  too; a branch both moved does not;
- settings files such as `.env` come in masked, like the rest of the copy
  (see [Secrets](secrets.md));
- steps start over, as after a commit;
- the agent hears what came in at the start of its next turn.

Your folder is only read. Sync is refused while an agent is at work, and stops
programs still running in the task's sandbox first; Kule names what it
stopped.

## Merging: two tasks, one folder

Tasks on the same folder never see each other's work. Kule tells you
**where they meet** while both are still going: the same method, section or
key, or the same file when it cannot look inside.

```text
task 5e6f7a8b changes them too: task.rs · method Task::commit
```

The app marks the task, lists it under Review, and sends a notification
when two tasks start to meet.

When the **second** of them commits, a file both changed is merged three
ways, from what both started with:

- changes to **different** methods, sections or keys both go in;
- two methods added at the same place are both kept;
- the commit says what it merged.

A **conflict** is named by what both changed:

```text
task.rs (both changed method Task::commit)
```

Two changes to the same method, section or key are a conflict even where
their lines do not touch. So is a merge whose parts would not stay where
either side put them.

The same holds for a file **you** change by hand while the agent works.
Each task keeps what the folder held at its last sync (`base/`) to merge
from: one clone on macOS; elsewhere the files up to 1 MiB, outside `.git`,
`node_modules`, `target` and the like.

## Related

- [`kule sync`](../cli/sync.md), [`kule status`](../cli/status.md), [`kule commit`](../cli/commit.md)
- [Review](../app/review.md)
- [Review, commit and undo](review-commit-undo.md)
