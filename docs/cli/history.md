---
title: kule history
sidebar_label: kule history
description: Show a task's signed, hash-chained history of commits and undos, checked entry by entry.
---

# kule history

Show the task's signed history (every commit and undo), checked entry by
entry.

## Synopsis

```sh
kule history [OPTIONS] <TASK>
```

## Description

Every [kule commit](commit.md) and [kule undo](undo.md) adds an entry to the
task's history. Each entry is **hash-chained** to the one before it and
**signed** with a key that lives in Kule's home (`kule.key`), hidden from
every sandbox. A commit's entry records its checkpoint, how many changes it
made and a snapshot hash of what it wrote.

`kule history` reads the chain, checks every signature and link, and prints
the entries oldest first. If anything in it was changed after it was signed
(an entry edited, removed or reordered), the check fails and the command
exits with an error instead of printing a history you cannot trust.

Use it to see what a task did to your folder, and to make sure nothing
rewrote that record.

It only reads; nothing changes.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**A task with four commits:**

```text
$ kule history 3422f830
  0. commit 1 · 9 changes · snapshot 1009aa9e9ec6
  1. commit 2 · 1 changes · snapshot ab68d01690e5
  2. commit 3 · 2 changes · snapshot 919dfa44867d
  3. commit 4 · 1 changes · snapshot 0f6b3afc0dfb
history verified: 4 entries, signed
```

**A commit, then its undo:**

```text
$ kule history 3efe1ca8
  0. commit 1 · 3 changes · snapshot 7671876a1160
  1. undo 1
history verified: 2 entries, signed
```

**Check in a script:**

```sh
kule history "$TASK" > /dev/null || echo "the history of $TASK does not verify"
```

## Output

| Line | Meaning |
| --- | --- |
| `N. commit C · M changes · snapshot H` | Entry `N`: a commit that kept checkpoint `C`, wrote `M` paths, with the first 12 characters of its snapshot hash. |
| `N. undo C` | Entry `N`: checkpoint `C` was undone. |
| `history verified: N entries, signed` | Every entry checked out. |

A task with no commits prints only `history verified: 0 entries, signed`.

## Errors and fixes

If the check fails, `kule history` prints the reason after `kule:` and exits
with `2`. Do not trust that task's record. Compare the workspace with what
you expect, and report it if you did not change Kule's files yourself (see
[Security](../security.md)).

## In the app

**Timeline** (Mod+3) lists the commits, newest first, with Undo on the
newest. See [Timeline](../app/timeline.md).

## See also

- [kule commit](commit.md), [kule undo](undo.md)
- [Review, commit and undo](../concepts/review-commit-undo.md)
