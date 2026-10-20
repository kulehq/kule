---
title: kule color
sidebar_label: kule color
description: Mark a task with one of eight colours, or take its colour away.
---

# kule color

Mark a task with a colour; without a colour, take it away.

## Synopsis

```sh
kule color [OPTIONS] <TASK> [COLOR]
```

## Description

A colour makes a task easy to spot in the app's task list. A task in a
[group](../concepts/groups.md) shows the group's colour unless it has its own.

The colour is only a mark. It changes nothing about how the task runs or is
judged.

## Arguments

| Name | Required | Meaning | Accepted values |
| --- | --- | --- | --- |
| `TASK` | yes | The task's full id. | See [Naming a task](index.md). |
| `COLOR` | no | The colour. Leave it out to remove the colour. | `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `pink` |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Mark a task teal:**

```text
$ kule color 3efe1ca8 teal
$ kule list
3efe1ca8  /Users/me/code/app · greeting fix (teal)
```

**Take the colour away:**

```sh
kule color 3efe1ca8
```

**An unknown colour is refused:**

```text
$ kule color 3efe1ca8 mauve
kule: a task's colour is one of red, orange, yellow, green, teal, blue, purple, pink, not "mauve"
```

## Output

Nothing on success.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: a task's colour is one of red, orange, … not "<word>"` | Use one of the eight names. |
| `kule: no task <id>` | Use the full id from `kule list`. |

## In the app

The colour swatches in the task's ⋮ menu (Mod+. opens it for the current
task, or right-click the task).

## See also

- [kule name](name.md), [kule group](group.md)
