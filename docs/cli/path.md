---
title: kule path
sidebar_label: kule path
description: Print where a task's copy, or a file in it, is on disk, for cd, scripts and editors.
---

# kule path

Print where a task's copy is on disk, or a file in it.

## Synopsis

```sh
kule path [OPTIONS] <TASK> [FILE]
```

## Description

The copy lives inside Kule's home, at `<home>/tasks/<id>/w/<folder name>`.
`kule path` prints that path, or the path of one file or folder in the copy,
so you can `cd` there, open it in an editor, or use it in a script.

`FILE` is a path inside the copy, relative to its top. Kule never follows a
link to get there, and refuses `..`, so the printed path is always inside
the copy.

Where [`kule shell --outside`](shell.md#outside-the-sandbox) refuses,
`kule path` warns on stderr when the task has changed what would run in
that folder by itself (git config or hooks, a toolchain pin, an editor's
settings), or while an agent is at work: your
prompt or editor would run them outside the sandbox. It still prints the path (a path alone runs nothing),
but take the warning seriously before you run git or open an editor there.

Only stdout holds the path, so command substitution works even with the
warning.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |
| `FILE` | no | A file or folder inside the copy, like `src/main.rs`. Without it, the copy itself. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**The copy:**

```text
$ kule path 3422f830
/Users/me/.kule/tasks/3422f830/w/app
```

**Go there:**

```sh
cd "$(kule path 3422f830)"
```

**Open the copy in your editor:**

```sh
code "$(kule path 3422f830)"
```

**One file:**

```text
$ kule path 3422f830 src/main.rs
/Users/me/.kule/tasks/3422f830/w/app/src/main.rs
```

**Compare a file with the workspace's:**

```sh
diff ~/code/app/src/main.rs "$(kule path 3422f830 src/main.rs)"
```

**The warning after a git config change:**

```text
$ kule path 3422f830
kule: careful in there: the copy's git config changed, and git there would run it outside the sandbox; review it, then commit or roll it back before opening a terminal
/Users/me/.kule/tasks/3422f830/w/app
```

## Output

One absolute path on stdout. A warning, if any, on stderr starting with
`kule: careful in there:`.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: <file>: No such file or directory (os error 2)` | The file is not in the copy. Paths are relative to the copy's top. |
| `kule: <file>: a path inside the copy, without ..` | Do not use `..` or an absolute path. |

:::tip

An editor runs things by itself from a folder: tasks, settings, recommended
extensions, git hooks. If an agent worked in the copy, open it in VS Code's
Restricted Mode (answer *no* when it asks whether you trust the authors).
The app's *Open the copy in your editor* asks first each time the copy
holds changes not committed.

:::

## In the app

*Copy path* (⌥⌘C on macOS, Ctrl+Alt+C on Linux) on a file in **Files**, and
*Open the copy in your editor* in the task's ⋮ menu. See
[Files and Preview](../app/files-and-preview.md).

## See also

- [kule shell](shell.md), [kule open](open.md)
