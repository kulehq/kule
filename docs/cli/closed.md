---
title: kule closed
sidebar_label: kule closed
description: Tell Kule you closed the terminal apps and editors the app opened a task's copy in, so agents may work in the task again.
---

# kule closed

Tell Kule you closed the terminal apps and editors the app opened the
task's copy in, outside the sandbox: agents may work in the task again.

## Synopsis

```sh
kule closed [OPTIONS] <TASK>
```

## Description

*Open in terminal app* and *Open the copy in your editor* open the task's
copy outside the sandbox, as you. Kule remembers each app it opened the
copy in, but it cannot see the app close. So until you say it is closed, no
agent starts work in the task: the app asks you before the next run, and
`kule run` and `kule exec` refuse with a message that names the app.

Close the terminal app or editor, then run `kule closed TASK`. It forgets
every app the copy was opened in, and runs start again. Running it when
nothing is open does nothing.

In the app, the same question comes before the next run: close the app,
then choose *I closed it, start the run*.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Run again after closing the editor:**

```text
$ kule exec 3422f830 -- cargo test
kule: Kule opened task 3422f830's copy in VS Code outside the sandbox; while that is open it can run what an agent writes there. Close it, then say so: in the app, or with `kule closed 3422f830`
$ kule closed 3422f830
$ kule exec 3422f830 -- cargo test
```

## Output

Nothing on success.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: no task <id>` | Use the full id from `kule list`. |

## In the app

Before a run in a task whose copy Kule opened outside the sandbox, the app
asks *Close … first*. Close the app, then choose *I closed it, start the
run*. See [Terminals and servers](../app/terminals-and-servers.md#outside-the-sandbox).

## See also

- [kule shell](shell.md), [kule path](path.md)
- [Files and preview](../app/files-and-preview.md)
