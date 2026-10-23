---
title: Git repositories
sidebar_label: Git repositories
description: How Kule judges .git by what it means - objects, references, the index, config and hooks - rather than as plain files.
---

# Git repositories

Kule needs no git, but when a workspace is a git repository, Kule reads
`.git` **as a repository, not as files**. Otherwise every `git status`
would look like a change: it rewrites `.git/index`, every ref update
appends to the logs, and git maintenance adds and removes lock files in the
background.

## What an agent's git work looks like

An agent that edits `main.rs` and commits in the copy:

```text
1 file changes: +0 ~1 -0
  ~ main.rs
      ~ function main
      + function check
git: main 86eba66 → 82180a4 · fast-forward · 1 commit
git: 3 new object files · index changed
```

With `kule commit`, the branch moves in your real repository too, after
the policy and your review.

## How each part is judged

| Part | How Kule judges it |
| --- | --- |
| **Objects** | Only ever added. Undo leaves them for `git gc`. |
| **References** (branches, tags) | Checked by value. A human commit on the branch the agent moved is a conflict; `git pack-refs`, or switching branches and back, is not. |
| **The index** | Compared by its entries, not its stat data, so `git status` in the workspace blocks neither commit nor undo. |
| **Staged files** | What the task stages must be what its files hold. An entry staged with other content (or taken out of the index while its file stays) waits for you, since the next `git commit` would record what nobody reviewed. |
| **Config and hooks** | Reported on lines of their own, because they make the next `git` command run code. |

```text
git: config changed (it can make the next git command run code)
git: hook pre-commit changed (it runs on the next matching git command)
```

## What the policy says about git

With the [balanced policy](policy.md):

- moving a branch **forward** (fast-forward) and creating a **new branch**
  pass on their own;
- history rewrites, deleting a branch, switching `HEAD`, tags, notes and
  replacements **wait for you**;
- git config changes **wait for you**;
- **git hooks are denied**, a nested repository's too: they would run on
  your next git command, as you.

Git hosts are not package registries: a connection to `github.com` waits
for you, because inside TLS a push cannot be told apart from a fetch.

## What runs by itself protects your terminal too

The task's terminal and [`kule shell`](../cli/shell.md) run in the task's
sandbox, like the agent. Opening the copy **outside** the sandbox is
different: *Open in terminal app*, *Open the copy in your editor* and
`kule shell --outside` run as you. Kule refuses them while an agent is at
work in the task, and while the task has changed something that would run
there by itself, outside the sandbox:

- the copy's git config or hooks, which your prompt's `git status` would
  run. Anything in `.git` that git does not keep as data (objects, refs,
  the index, logs) counts as git config;
- a repository inside the copy, or another version control system's
  (Mercurial, Sapling, Jujutsu, Darcs, Pijul and others), whose config and
  hooks run on its next command;
- a file the [`automation`](../reference/policy-rules.md) rule names, such
  as a toolchain pin (`rust-toolchain.toml`, `.nvmrc`, `.python-version`,
  `.tool-versions`), `.envrc`, or an editor's settings and tasks
  (`.vscode/`), which a prompt or an editor reads when it opens the folder;
- the package manager a `package.json` names (`packageManager`, or
  `devEngines.packageManager`): with Corepack on, the next `pnpm` or `yarn`
  there, an editor's included, fetches and runs that version. Other changes
  to `package.json` leave the terminal open; they wait at commit as a
  manifest. A `package.json` Kule cannot read as JSON closes it too.

`kule path` warns on stderr. Review and commit or roll them back first.

While the copy holds other changes not committed, Kule lists them and asks
before it opens the copy outside the sandbox. See
[Terminals and servers](../app/terminals-and-servers.md#outside-the-sandbox).

## Worktrees

A worktree's repository next to the workspace stays readable to the task,
so git works in the copy as it does in the folder.

## Related

- [Review, commit and undo](review-commit-undo.md)
- [Sync and merging](sync-and-merging.md): commits and branches in the folder come in on sync
- [`kule status`](../cli/status.md), [`kule shell`](../cli/shell.md), [`kule path`](../cli/path.md)
