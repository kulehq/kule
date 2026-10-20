---
title: kule name
sidebar_label: kule name
description: Give a task a name shown after its folder's name, or take the name away.
---

# kule name

Give a task a name, shown after its folder's name; without a name, take it
away.

## Synopsis

```sh
kule name [OPTIONS] <TASK> [NAME]
```

## Description

A task is shown by its folder's name. When you have several tasks on one
folder, a name tells them apart: the app shows `app – greeting fix`, and
`kule list` shows `· greeting fix` after the folder.

The name is a label only. It does not change the task's id (commands still
take the id), the copy, or the folder.

Leading and trailing spaces are trimmed. A name that is empty after
trimming takes the name away.

## Arguments

| Name | Required | Meaning | Accepted values |
| --- | --- | --- | --- |
| `TASK` | yes | The task's full id. | See [Naming a task](index.md). |
| `NAME` | no | The new name. Leave it out to remove the name. | Up to 60 characters; no control or text-direction characters. Quote it if it has spaces. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Name a task:**

```text
$ kule name 3efe1ca8 "greeting fix"
$ kule list
3efe1ca8  /Users/me/code/app · greeting fix
```

**Rename it:** run `kule name` again with the new name.

**Take the name away:**

```sh
kule name 3efe1ca8
```

## Output

Nothing on success.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: no task <id>` | Use the full id from `kule list`. |
| `kule: a task's name is at most 60 characters` | Shorten it. |
| `kule: a task's name cannot hold control or text-direction characters` | Remove tabs, newlines and right-to-left marks. Kule refuses them so that a name cannot make itself look like something else. |

## In the app

*Rename…* in the task's ⋮ menu, or F2.

## See also

- [kule color](color.md), [kule list](list.md)
