---
title: kule reveal
sidebar_label: kule reveal
description: Let a folder's tasks read a project beside it, such as a shared library it builds from.
---

# kule reveal

Let a folder's tasks **read** a project that lies beside it, such as a shared
library it builds from. They never get to write it.

## Synopsis

```sh
kule reveal [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-reveal-list) | List the projects opened, and what the folder builds from that its tasks cannot read (the default). |
| [`add`](#kule-reveal-add) | Let the folder's tasks read a project beside it. |
| [`remove`](#kule-reveal-remove) | Hide it again. |

## Description

The [sandbox](../concepts/sandbox.md) hides your own folders from a task's
commands: Documents, Desktop, Downloads and the like, and also **the folder
your workspace lies in**, with the other projects there. So when
`~/code/app` is the workspace, `~/code/shared` is hidden too.

That breaks a build that reads a project next to it:

- `path = "../shared"` in `Cargo.toml`,
- `"file:../ui"` in `package.json`,
- `..\Shared\Shared.csproj` in a .NET project.

`kule reveal add` opens such a project to the folder's tasks, **read-only**.
Kule also **suggests** what to open: it reads the folder's own manifests
(`Cargo.toml`, `package.json`, `go.mod`, `go.work`, `pyproject.toml`, Gradle
settings, `.csproj`/`.fsproj`/`.vbproj`/`.sln`/`.slnx`, `Directory.*.props`)
and names the projects they point at. It only suggests; only you open one.

Rules that keep this safe:

- The list lives in Kule's home, out of every sandbox's reach. Suggestions
  come from your real folder, never from a task's copy.
- An opened project is read, never written. What Kule hides inside it (for
  example credential folders) stays hidden.
- It is checked again before every run. A project that has since become a
  link, or is no longer in one of your hidden folders, is not opened.
- Next to the task's copy, Kule places links to the projects you opened, so
  `../shared` from the copy leads where it leads from the folder.

To open a path for a single command only, use
[`kule exec --reveal PATH`](exec.md).

All subcommands take `--home` (see [global options](index.md)).

## kule reveal list

```sh
kule reveal list [OPTIONS]
```

| Option | Default | Meaning |
| --- | --- | --- |
| `--folder FOLDER` | the current directory | The folder whose suggestions to show. |

```console
$ cd ~/code/app
$ kule reveal
no project beside a folder is open to its tasks
builds from ~/code/shared, which its tasks cannot read: kule reveal add '~/code/shared'
```

After opening it:

```console
$ kule reveal
  1  ~/code/shared  · for ~/code/app
```

A project that is kept but cannot be opened now shows why, for example
`(not opened: …: a link; name the folder it leads to)`.

`kule new` prints the same suggestions when it makes a task.

## kule reveal add

```sh
kule reveal add [OPTIONS] <PATH>
```

| Argument / option | Default | Meaning |
| --- | --- | --- |
| `PATH` | | The project folder, relative to the current directory or absolute. |
| `--folder FOLDER` | the current directory | The workspace whose tasks may read it. |

```console
$ cd ~/code/app
$ kule reveal add ../shared
kept as 1: tasks in ~/code/app read ~/code/shared, never write it
```

From then on, a task on `~/code/app` can read `../shared`; a write there is
refused:

```console
$ kule exec 1046fb30 -- touch ../shared/x
touch: ../shared/x: Operation not permitted
no changes
sandbox: refused file-write-create ~/code/shared/x (touch)
```

## kule reveal remove

```sh
kule reveal remove [OPTIONS] <ID>
```

```console
$ kule reveal remove 1
closed 1: hidden again from the next command on
$ kule reveal remove 1
nothing open as 1
```

## Errors and fixes

`kule reveal add` refuses a path that is not a project beside the workspace.
The message starts with the path and says why:

| Reason | What it means |
| --- | --- |
| `not found` / `not a folder` | Name an existing directory. |
| `a link; name the folder it leads to` | Give the real folder, not a symbolic link to it. |
| `the workspace itself, or a folder holding it` | Only a project *beside* the workspace can be opened. |
| `not in a folder Kule hides as yours (the workspace's own folder, Documents, …); the tasks read it already, or Kule keeps it hidden for good` | The path is either already readable by the tasks (nothing to do), or it is somewhere Kule never opens, such as a credentials folder. |

## In the app

*Settings → Rules* lists the projects opened, and the task header's **builds
from** chip suggests the ones the folder's manifests name.

## See also

- [`kule exec`](exec.md): `--reveal PATH` for one command.
- [Sandbox](../concepts/sandbox.md)
