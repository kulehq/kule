---
title: kule list
sidebar_label: kule list
description: List every task with its id, folder, name, colour and group, in the order the app shows them.
---

# kule list

List every task, in the order the app's Tasks panel shows them.

## Synopsis

```sh
kule list [OPTIONS]
```

## Description

`kule list` prints one line per task in Kule's home. It is the quickest way
to find a task's id, which every other command needs.

The order is the app's: tasks you dragged into place (or moved with
Alt+Shift+↑/↓) keep that order, and tasks you never moved come first,
newest at the top.

It only reads; nothing changes.

## Arguments

None.

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**List tasks:**

```text
$ kule list
3422f830  /Users/me/code/app
40ac70b8  /Users/me/notes
3efe1ca8  /Users/me/code/app · greeting fix (teal)
```

**With names, colours and groups:**

```text
$ kule list
5e6f7a8b  /Users/me/Downloads · march invoices (orange) [clients]
3422f830  /Users/me/code/app
```

**Find the tasks on one folder:**

```sh
kule list | grep ' /Users/me/code/app'
```

**No tasks yet:** `kule list` prints nothing and exits with `0`.

## Output

Each line is:

```text
<id>  <workspace> · <name> (<colour>) [<group>]
```

| Part | Shown when | Set with |
| --- | --- | --- |
| `<id>` | always | made by [kule new](new.md) |
| `<workspace>` | always: the real folder the task copies | |
| `· <name>` | the task has a name | [kule name](name.md) |
| `(<colour>)` | the task has a colour | [kule color](color.md) |
| `[<group>]` | the task is in a group | [kule group put](group.md) |

## In the app

The **Tasks** panel (Mod+0) shows the same list. Tasks waiting for you
collect under *Needs you*.

## See also

- [kule new](new.md), [kule name](name.md), [kule color](color.md)
- [kule group](group.md)
