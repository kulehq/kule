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
- changes that **wait for approval** (manifests, lockfiles, CI files,
  unfamiliar kinds of file, hidden files, git config, mass deletes, a file
  made executable, … under the built-in
  *balanced* policy) are committed only with `--approve`, and only as you
  reviewed them;
- changes that are **denied** (a new git hook, for example) are never
  committed, even with `--approve`.

If anything is held back, **nothing** is committed: Kule prints the ruling and
exits with code `3`. A commit is all or nothing.

**Approving takes what you reviewed.** `--approve` commits the changes only
while they are the ones you looked at. Pass the digest
[kule status](status.md) prints (`--reviewed DIGEST`): if anything changed
since, the commit is refused and you review again. Without `--reviewed`, on
a terminal, Kule lists what it would approve and asks; with no terminal to
ask on (a script, a pipe), it refuses.

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
in the task, and while a terminal is open on it (the app's, or
[kule shell](shell.md)).

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--approve` | | | Also commit the changes the policy puts to a person. Denied changes still never pass. |
| `--remember` | | | With `--approve`: do not ask again, on this folder, about the files approved now: manifests and project files and hidden files by path, and kinds of file the project had none of by kind. What runs code on its own (CI, editor and agent settings) is asked every time. See [kule rules](rules.md). |
| `--reviewed` | digest | | With `--approve`: the digest `kule status` printed for the changes you reviewed. Only those are committed. Without it, Kule asks on a terminal and refuses elsewhere. |
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
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    Cargo.toml
kule: not committed; rerun with --approve to approve the changes above
$ echo $?
3
$ kule status 3efe1ca8
…
digest: 9f2c…e41a (to approve these: kule commit 3efe1ca8 --approve --reviewed 9f2c…e41a)
$ kule commit 3efe1ca8 --approve --reviewed 9f2c…e41a
committed 3 changes · checkpoint 1
```

On a terminal, `kule commit 3efe1ca8 --approve` alone lists the changes and
asks `Approve and commit these changes? [y/N]`.

**Approve, and do not ask about these files on this folder again:**

```sh
kule commit 3efe1ca8 --approve --remember --reviewed 9f2c…e41a
```

```text
committed 3 changes · checkpoint 1
not asked again on this folder: changes to Cargo.toml (manifests-and-lockfiles)
```

**A denied change:**

```text
$ kule commit 3422f830 --approve --reviewed 51d0…7c3b
policy: denied
  a git hook, which runs on its own (git-hooks) · denied
    git hook pre-commit
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
| `kule: Kule ended <name>, still running in the task's sandbox, to commit` | On stderr: what still ran in the task's sandbox, which Kule ended before it wrote in the copy. For more than one: `kule: Kule ended <N> processes still running in the task's sandbox, to commit: <names>`. See [Processes and servers](../concepts/sandbox.md#processes-and-servers). |
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
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then commit` | Close the task's terminal (the app's tab, or `exit` in `kule shell`), then commit. Kule does not end what runs there, so nothing unsaved is lost. |

## In the app

**Commit**, or **Approve and commit** when something waits, under Review
(Mod+Shift+Enter). *Don't ask again in this folder* on a held file does what
`--remember` does, and *Also commit these* what `--include-ignored` does. See
[Review](../app/review.md).

## See also

- [Review, commit and undo](../concepts/review-commit-undo.md), [Policy](../concepts/policy.md)
- [kule status](status.md), [kule undo](undo.md), [kule rollback](rollback.md), [kule rules](rules.md)
