---
title: kule status
sidebar_label: kule status
description: Show a task's uncommitted changes, down to functions and sections, and what the policy says about them.
---

# kule status

Show the task's uncommitted changes and what the policy says about them.

## Synopsis

```sh
kule status [OPTIONS] <TASK>
```

## Description

`kule status` compares the task's copy with the workspace and tells you,
before you commit:

- **What changed:** files created, modified, deleted and moved, and inside
  each file, what changed by the names a person uses: functions, methods,
  fields and types in Rust, C#, TypeScript, JavaScript, Python and Go;
  sections in Markdown; keys in JSON and TOML.
- **What happened to git**, when the folder is a repository: branch moves,
  new objects, the index, config and hook changes. See
  [Git](../concepts/git.md).
- **What the policy says:** whether the changes pass on their own, wait for
  your approval, or are denied, and which rule decided each. See
  [Policy](../concepts/policy.md).
- **Writes in your home** that wait for review (see [kule home](home.md)).
- **Where other tasks meet this one:** tasks on the same folder that change
  the same files, down to the function or section.
- **What the workspace changed meanwhile:** files changed in the real folder
  since the copy was last brought up to date (by you, or by another task's
  commit), which [kule sync](sync.md) can bring in.
- **What the task's agent runs have spent**, and what is left of its budget.

Effects are found by comparing the copy with the workspace, not by watching
commands, so `status` shows everything: the agent's work, your own edits in
`kule shell`, and anything a `kule exec` command did.

It only reads; nothing changes.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--json` | | | Print one JSON object instead of text. See [JSON output](#json-output). |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Changes that pass on their own:**

```text
$ kule status 3efe1ca8
2 file changes: +1 ~1 -0
  + NOTES.md
  ~ src/main.rs
      ~ function main
      + function wave
policy: passes on its own
```

**A change that waits for you** (a manifest, under the built-in *balanced*
policy):

```text
$ kule status 3efe1ca8
3 file changes: +1 ~2 -0
  ~ Cargo.toml
      + key dependencies
  + NOTES.md
  ~ src/main.rs
      ~ function main
      + function wave
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    Cargo.toml
```

**Moves, told apart from deletions:**

```text
$ kule status 3422f830
6 file changes: +1 ~0 -2 »3
  + docs/
  » report.pdf → docs/report.pdf
  - fake.pdf
  » src/ → lib/
  - run.sh
policy: passes on its own
```

**Git, and a change that is denied:**

```text
$ kule status 3422f830
git: hook pre-commit changed (it runs on the next matching git command)
policy: denied
  a git hook, which runs on its own (git-hooks) · denied
    git hook pre-commit
```

**The folder changed meanwhile:**

```text
$ kule status 3efe1ca8
1 file changes: +0 ~1 -0
  ~ src/main.rs (changed in the workspace too; not outlined)
policy: passes on its own
the workspace changed since the task's copy (kule sync 3efe1ca8 brings it in):
  README.md
  src/main.rs (the task changed it too)
```

**Two tasks on one folder meet:**

```text
$ kule status 3422f830
1 file changes: +0 ~1 -0
  ~ README.md
      ~ section App
policy: passes on its own
task 3efe1ca8 changes them too:
  README.md · section App
```

**Ignored paths stay behind:**

```text
$ kule status 3422f830
2 ignored by the workspace, staying in the copy (commit --include-ignored takes them):
  · target/
  · target/app
1 file changes: +0 ~1 -0
  ~ README.md
      ~ section App
policy: passes on its own
```

## Output

| Line | Meaning |
| --- | --- |
| `no changes` | The copy holds what the workspace holds. |
| `N file changes: +a ~m -d »v` | Counts: created, modified, deleted, moved. |
| `+ path`, `~ path`, `- path` | A file or folder (ending in `/`) created, modified or deleted. |
| `» from → to` | A move or rename. A file moved with its folder is not listed again. |
| indented `+`/`~`/`-` lines | What changed inside the file above: `function main`, `method Task::commit`, `section Plan › Later`, `key dependencies`. Every change in the file is listed under one of them, or under its top level. |
| `(… not outlined)` | Kule could not look inside this file, and says why (for example, the workspace changed it too). |
| `N ignored by the workspace, …` | Paths the workspace's `.gitignore` excludes. A plain commit leaves them in the copy. |
| `git: main a141d88 → 4e385b1 · fast-forward · 1 commit` | The branch moved. `history rewritten` instead of `fast-forward` means commits were replaced. |
| `git: 3 new object files · index changed` | New git objects (only ever added) and an updated index. |
| `git: config changed (…)`, `git: hook … changed (…)` | Changes that make the next git command run code. |
| `git: <path> is staged unlike the file (…)` | The index holds content for this file that differs from the file itself, so the next `git commit` would record something nobody reviewed. It waits for you. |
| `in your home, caught for review` | Writes in your home folder; see [kule home](home.md). |
| `policy: <verdict>` | The overall ruling: `passes on its own`, `waits for approval`, or `denied`. |
| `  <reason> (<rules>) · <verdict>` | What does not simply pass, put together by why: the reason in plain words, the rules behind it, and the verdict. Files that wait only for their kind are one group per kind (`a kind of file this project has none of: weird`). See [Policy rules](../reference/policy-rules.md). |
| `    <subject>` | A change in the group above: a path, or a git change such as `git hook pre-commit`. |
| `digest: <hex> (to approve these: …)` | What these changes hash to. `kule commit TASK --approve --reviewed <hex>` commits them only while they are still these. |
| `home digest: <hex>` | The same for the writes in your home; see [kule home](home.md). |
| `task <id> changes them too:` | Another task on the same folder changes the same files; the lines under it name the file and the function, section or key. |
| `the workspace changed since the task's copy …` | What `kule sync` would bring in. `(the task changed it too)` marks files both changed. |
| `workspace <path> is gone (moved or deleted); commit and undo are refused` | The real folder is no longer there. |
| `spent: …; budget …, … left` | What the task's agent runs cost so far. Shown once there has been a run or a budget is set. ` (N records cannot be read)` when some records of what runs spent cannot be read; a budget over them holds the next run (see [kule budget](budget.md)). ` (<amount> set aside for runs at work or that did not end)` when runs hold a share; it counts in what is left. |

At most 40 files are listed; the rest are counted (`… N more (kule status --json)`).

## JSON output

`kule status TASK --json` prints one object. Its fields:

| Field | What it holds |
| --- | --- |
| `effects` | `effects`: every changed path, with `path`, `change` (`created`, `modified`, `deleted`) and `kind` (`file`, `dir`, …); plus moves, git changes and ignored paths when there are any, and `repository` (whether the folder is a git repository). |
| `ruling` | `verdict` (`allow`, `ask` or `deny`) and `judgements`: one per subject, with `subject`, `verdict` and the `rules` that decided it. |
| `outline` | For each changed file, what changed inside it, or `"outline": "skipped"` with a `reason`. |
| `spent` | `runs`, `tokens`, `cost` (a list of amounts and currencies) and `unpriced_runs`, and `unreadable` (records that cannot be read) when there are any. While runs hold a share: `held` (what they set aside, one amount per currency, counted as spent until each run's usage comes) and `held_by` (the tasks those runs are in). |
| `budget` | The task's own budget, or `null`. |
| `home` | Writes in your home waiting for review. |
| `behind` | `changed`: paths the workspace changed since the last sync; `both`: those the task changed too; `repository`: whether commits, branches or the index changed. |
| `digest` | What the changes a plain commit would take hash to, for `kule commit --approve --reviewed`; `null` with none. |

```json
{
  "effects": {
    "effects": [
      { "path": "src/main.rs", "change": "modified", "kind": "file" }
    ],
    "repository": true
  },
  "ruling": {
    "verdict": "allow",
    "judgements": [
      { "subject": "src/main.rs", "verdict": "allow", "rules": ["files"] }
    ]
  },
  "outline": [
    { "path": "src/main.rs", "outline": "skipped", "reason": "before_unknown" }
  ],
  "spent": { "runs": 0, "tokens": 0, "cost": [], "unpriced_runs": 0 },
  "budget": null,
  "home": [],
  "behind": { "changed": ["src/main.rs"], "both": ["src/main.rs"], "repository": false },
  "digest": "9f2c…e41a"
}
```

A script that commits only what passes on its own:

```sh
if [ "$(kule status "$TASK" --json | jq -r .ruling.verdict)" = allow ]; then
  kule commit "$TASK"
fi
```

(`kule commit` refuses on its own anyway, with exit code `3`.)

## In the app

The **Review** panel (Mod+2) shows the same, with Commit and Roll back. See
[Review](../app/review.md).

## See also

- [Review, commit and undo](../concepts/review-commit-undo.md)
- [kule commit](commit.md), [kule rollback](rollback.md), [kule sync](sync.md), [kule steps](steps.md)
