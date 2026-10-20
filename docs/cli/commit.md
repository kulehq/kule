---
title: kule commit
sidebar_label: kule commit
description: Apply a task's changes to the real folder, as the policy allows, keeping a checkpoint that undo can put back.
---

# kule commit

Apply the task's changes to the workspace, keeping a checkpoint. The policy
decides what may pass.

## Synopsis

```sh
kule commit [OPTIONS] <TASK>
```

## Description

`kule commit` is the only command that writes your real folder. It takes the
changes [kule status](status.md) shows and applies them to the workspace.

**The policy decides first.** Every change has a verdict (see
[Policy](../concepts/policy.md)):

- changes that **pass on their own** are committed;
- changes that **wait for approval** (manifests, lockfiles, CI files, git
  config, mass deletes, … under the built-in *balanced* policy) are committed
  only with `--approve`;
- changes that are **denied** (a new git hook, for example) are never
  committed, even with `--approve`.

If anything is held back, **nothing** is committed: Kule prints the ruling and
exits with code `3`. A commit is all or nothing.

**A checkpoint is kept.** Before writing, Kule saves what the commit replaces
in the task's own checkpoint store, so [kule undo](undo.md) can put the
folder back. Every commit is also an entry in the task's signed
[history](history.md).

**Files are written safely.** Each file goes in through a rename, so nobody
sees half a file. Values Kule masked in `.env` files are put back to the
real ones on the way in.

**The workspace must not have changed underneath.** If someone changed, in
the real folder, a file the task also changed, the two changes are merged
three ways from what both started with: changes to different functions,
sections or keys both go in. If both changed the **same** function, section
or key, the commit is refused as a conflict and nothing is written. See
[Sync and merging](../concepts/sync-and-merging.md).

**Ignored paths stay behind.** Paths the workspace's `.gitignore` excludes
(build output, caches, `.env`) stay in the copy unless you pass
`--include-ignored`. The policy judges them too.

`kule commit` is refused while an agent or a `kule exec` command is at work
in the task.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--approve` | | | Also commit the changes the policy puts to a person. Denied changes still never pass. |
| `--remember` | | | With `--approve`: do not ask again, on this folder, about the files approved now (manifests, CI and project files). See [kule rules](rules.md). |
| `--include-ignored` | | | Also take the paths the workspace's `.gitignore` excludes. |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Everything passes:**

```text
$ kule commit 3422f830
committed 1 changes · checkpoint 2
```

**Something waits for approval:**

```text
$ kule commit 3efe1ca8
policy: waits for approval
  Cargo.toml · waits for approval (manifests-and-lockfiles)
kule: not committed; rerun with --approve to approve the changes above
$ echo $?
3
$ kule commit 3efe1ca8 --approve
committed 3 changes · checkpoint 1
```

**Approve, and do not ask about these files on this folder again:**

```sh
kule commit 3efe1ca8 --approve --remember
```

```text
committed 3 changes · checkpoint 1
not asked again on this folder: changes to Cargo.toml (manifests-and-lockfiles)
```

**A denied change:**

```text
$ kule commit 3422f830 --approve
policy: denied
  git hook pre-commit · denied (git-hooks)
kule: not committed; the policy denies the changes above
```

Roll it back (`kule rollback`), or fix the copy by hand in `kule shell` and
check `kule status` again.

**Take the build output too:**

```text
$ kule commit 3422f830 --include-ignored
committed 2 changes · checkpoint 3
```

**A conflict with the workspace:**

```text
$ kule commit 3efe1ca8
kule: the workspace changed since Kule last saw it, at: README.md (both changed section App)
hint: roll back, or rerun the task on the current workspace
```

## Output

| Line | Meaning |
| --- | --- |
| `committed N changes · checkpoint C` | The changes went in. `N` counts every path the commit wrote, including inside `.git`, so it can be larger than the file count `kule status` shows. `C` is the checkpoint [kule undo](undo.md) uses. |
| `merged with what the folder changed meanwhile: <paths>` | These files were changed in the workspace too, and merged. |
| `not asked again on this folder: …` | With `--remember`: what Kule will let through from now on. |
| `nothing to commit` | The copy holds what the workspace holds. |
| `policy: …` and `kule: not committed; …` | The ruling held the changes back; nothing was written (exit code `3`). |

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: not committed; rerun with --approve to approve the changes above` | Look at what waits (`kule status`), then `kule commit --approve` if you agree. |
| `kule: not committed; the policy denies the changes above` | Denied changes never pass. Roll back, or undo that change in the copy. |
| `kule: the workspace changed since Kule last saw it, at: <paths>` | Someone changed the same parts of these files in the real folder. Run `kule rollback`, or bring the folder's changes in with [kule sync](sync.md) (when they merge) and review again. |
| `kule: workspace <path> is gone …` | The real folder was moved or deleted; a commit has nowhere to go. |
| `kule: <path> changed since it was reviewed; nothing was committed` | A settings file changed between the review and the commit. Check `kule status` again. |
| `kule: task <id> is busy: another kule command holds its lock` | Wait for the other command. |

## In the app

**Commit**, or **Approve and commit** when something waits, under Review
(Mod+Shift+Enter). *Don't ask again in this folder* on a held file does what
`--remember` does, and *Also commit these* what `--include-ignored` does. See
[Review](../app/review.md).

## See also

- [Review, commit and undo](../concepts/review-commit-undo.md), [Policy](../concepts/policy.md)
- [kule status](status.md), [kule undo](undo.md), [kule rollback](rollback.md), [kule rules](rules.md)
