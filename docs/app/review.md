---
title: Review and commit
sidebar_label: Review
description: See what a task changed and what the policy says about it, then commit it to your folder or roll it back.
---

# Review and commit

**Review** (Mod+2) shows the task's uncommitted changes and the
[policy](../concepts/policy.md)'s ruling on them. Nothing reaches your real
folder until you commit here (or with [`kule commit`](../cli/commit.md)).

## The ruling

At the top, the ruling says one of three things, and which rule said so:

- **All pass**: every change may reach the folder on its own.
- **Some wait for your approval**: for example a changed `package.json`,
  lockfile or CI file, a script, or a hidden file under the balanced policy.
- **Some are denied**: for example a new git hook. Denied changes never
  reach the folder, even when you approve.

Under it, what waits or is denied is put together by why: a reason in plain
words ("runs code when a tool or an agent opens the folder"), the rules, and
the files it holds. Files that wait only for their kind are one group per
kind ("a kind of file this project has none of: vue"), answered at once.

## What changed

- **Created, modified and deleted files.** Moves are told apart from
  deletions (`» photos/ → 2026/`).
- **Inside each file**, Review names what changed by the names a person uses:
  functions, methods, fields and types in Rust, C#, TypeScript, JavaScript,
  Python and Go; sections in Markdown; keys in JSON and TOML. Every change in
  a file is listed under one of them, or under its top level:

  ```
    ~ notes.md
        + section Plan › Later
    ~ src/task.rs
        ~ method Task::commit
        + method Task::check
  ```

- **Pictures** (PNG, JPEG, GIF, WebP) show what they were and what they are.
  Only what its header proves to be such a picture, under 8 MiB and 40
  megapixels, reaches the window.
- **Git** changes, when the folder is a repository: branch moves, new
  objects, the index, and lines of their own for hooks and `config`, since
  those make the next `git` command run code. See [Git](../concepts/git.md).

## Ignored paths

Files the folder's `.gitignore` excludes (build output, caches, `.env`) stay
in the copy. Review lists them; tick **Also commit these** to take them too.
The policy judges them as well. The rules come from your folder, never from
the copy the agent edits.

## Writes in your home

An agent that changed `~/.zshrc`, ran `git config --global` or wrote under
`~/.config` did not reach your home: its writes were caught in the task's
home layer (see [Home layer](../concepts/home-layer.md)). They wait here,
apart from the commit:

| Button | What it does |
| --- | --- |
| **Show** | A line-by-line diff of what the change would do to your file, with what looks like a secret shown as `‹hidden …›` on both sides. |
| **Apply** | Takes this one change to your real home, after checking your file is still as it was. What it replaces is kept in the task. |
| **Discard** | Throws the change away. |
| **Undo** | Takes the last apply back, if you have not changed those paths since. |

A file that runs at every start (`.zshrc`, `.profile`, `.gitconfig`,
autostart entries, systemd user units) is marked *runs at every start* and
opens its diff right away. A home change is never part of a commit.

## Where tasks meet

Tasks on the same folder never see each other's work. So Review lists the
other tasks that changed the same files, down to the method, section or key
(`task 5e6f7a8b changes them too: task.rs · method Task::commit`), while both
are still going. When the second of them commits, a file both changed is
merged three ways; only the same part changed in both is a conflict. See
[Sync and merging](../concepts/sync-and-merging.md).

## Changed in the folder

The copy does not follow your folder by itself: the agent works on the
folder as it was when the copy was made or last brought up to date. Review
lists what the folder changed since, such as your own edits or another
task's commit, and which of those files the task changed too.

**Bring into the copy** (Mod+Shift+G, also in the task's ⋮ menu) brings
them in:

- a file only the folder changed comes in as it is;
- a file the task changed too is merged by method, section or key, and stays
  the task's change;
- when any of them does not merge, nothing comes in, and Kule names what both
  changed;
- settings files such as `.env` come in masked, like the rest of the copy;
- steps start over, as after a commit, and the agent hears what came in at
  the start of its next turn.

It is refused while an agent is at work. Programs still running in the task's
sandbox are stopped first, as before a commit. Your folder is only read.

## The buttons

| Button | When | Keys |
| --- | --- | --- |
| **Commit** | Everything passes. Applies the changes to your folder and keeps a checkpoint. | Mod+Shift+Enter |
| **Approve and commit** | Something waits for you. Applies what passes and what you approve; denied changes are held back. | Mod+Shift+Enter |
| **Don't ask again in this folder** | On a held file (a manifest, a project file or a hidden file): first shows what it would have let through, then remembers it for this folder. What runs code on its own (CI, editor and agent settings) has none: it is asked every time. | none |
| **Don't ask again about files of kind X in this folder** | On a group of files held only for their kind: first shows what it would have let through, then remembers the kind for this folder. | none |
| **Roll back** | Throws the changes away; the copy matches the folder again. Press twice. | Mod+Shift+Backspace |

After a commit, **Undo** on the [Timeline](timeline.md) (Mod+Shift+U, press
twice) takes the newest commit back out of your folder.

Before a commit, a rollback or a sync writes in the copy, what still runs in
the task's sandbox is ended. Kule names what it ended, in a notice and in a
note in Live, for example *Kule ended node, still running in the task's
sandbox, to commit*. See
[Processes and servers](../concepts/sandbox.md#processes-and-servers).

:::note

A commit refuses when your folder changed underneath it: you edited a file
the agent also changed, or added files to a directory the agent deleted.
Bring the folder's changes into the copy first. A commit writes each file
through a rename, so nobody sees half a file.

:::

## From a terminal

[`kule status`](../cli/status.md) shows the same changes and ruling
(`--json` for scripts). [`kule commit`](../cli/commit.md) commits
(`--approve`, `--remember`, `--include-ignored`),
[`kule rollback`](../cli/rollback.md) throws the changes away,
[`kule sync`](../cli/sync.md) brings the folder's changes in, and
[`kule home`](../cli/home.md) handles writes in your home.
