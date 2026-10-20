---
title: Review, commit and undo
sidebar_label: Review, commit and undo
description: How Kule shows what a task changed, how a commit applies it safely with a checkpoint, and how rollback, undo, steps and the signed history work.
---

# Review, commit and undo

## Review: what changed

Kule finds what a task changed by comparing its copy with the workspace.
`kule status` (or **Review** in the app) shows it:

```text
3 file changes: +1 ~1 -1
  - hello.txt
  + new.txt
  ~ notes.md
      + section Notes › Later
policy: passes on its own
```

- `+` created, `~` modified, `-` deleted. Moves are shown as moves
  (`» photos/ → 2026/`), not as a delete and a create.
- Under a changed file, Kule names **what changed inside it**, by the names
  a person uses:
  - functions, methods, fields and types in Rust, C#, TypeScript,
    JavaScript, Python and Go;
  - sections in Markdown;
  - keys in JSON and TOML.

  Every change in a file is listed under one of them, or under its top
  level.
- A changed picture (PNG, JPEG, GIF, WebP) shows before and after in the
  app.
- For a git repository, branch moves, new objects, the index, config and
  hooks get lines of their own. See [Git repositories](git.md).
- The last line is the [policy's](policy.md) verdict.

`kule status TASK --json` gives the same as JSON for scripts.

## Commit: apply it to the folder

```sh
kule commit TASK
```

```text
committed 3 changes · checkpoint 1
```

A commit:

- applies what **passes**; with `--approve` also what **asks**; never what
  is **denied**;
- refuses when the workspace changed underneath, for example when you
  edited a file the agent also changed:

  ```text
  kule: the workspace changed since Kule last saw it, at: hello.txt (both changed lines 1–3)
  hint: roll back, or rerun the task on the current workspace
  ```

  ([Sync](sync-and-merging.md) can bring your edits into the copy first);
- writes each file through a rename, so nobody sees half a file;
- leaves paths that `.gitignore` excludes in the copy, unless you add
  `--include-ignored`;
- puts back the real values of secrets that were masked in `.env` files
  (see [Secrets](secrets.md));
- keeps a **checkpoint**: what the commit replaced.

In the app: **Commit** (Mod+Shift+Enter), or **Approve and commit** when
something waits for you.

## Rollback: throw it away

```sh
kule rollback TASK
```

```text
rolled back 2 changes
```

The copy matches the workspace again. In the app: **Roll back**
(Mod+Shift+Backspace, press twice).

## Undo: take a commit back

```sh
kule undo TASK
```

```text
undid checkpoint 1 (3 changes)
```

Undo reverts the newest commit in the workspace and the copy. Run it again
to walk further back. What a commit replaced is kept in the task's own
Lonca store (git objects under `checkpoints/store.git`, with file modes and
times beside them), so undo brings back deleted files too, even in a folder
that is not a git repository.

In the app: Timeline → **Undo** (Mod+Shift+U, press twice).

## Steps: every tool call

When an agent works, every tool call that changes something becomes a
**step**. `kule steps TASK` lists them; the app shows them in Live and
Timeline. In the app you can:

- **Go back here**: put the copy back as it was after a step. Going back is
  a step itself, so you can go back on it.
- **Fork from here**: start a new task from the copy as it was after that
  step.

A commit starts the steps over. A step that cannot be gone back to (a file
over 64 MB, more than 1 GB in one go, a file with a second name) says why.
`.git` is not put back. See [Timeline](../app/timeline.md).

## The signed history

Every commit and undo is an entry in a **signed, hash-chained history**.
The signing key lives in Kule's home, hidden from every sandbox.

```sh
kule history TASK
```

```text
  0. commit 1 · 3 changes · snapshot 0d0d2c26745d
history verified: 1 entries, signed
```

`kule history` checks every entry and fails if anything in it was changed.

## Related

- [`kule status`](../cli/status.md), [`kule steps`](../cli/steps.md), [`kule commit`](../cli/commit.md), [`kule rollback`](../cli/rollback.md), [`kule undo`](../cli/undo.md), [`kule history`](../cli/history.md)
- [Review](../app/review.md), [Timeline](../app/timeline.md)
- [Sync and merging](sync-and-merging.md)
