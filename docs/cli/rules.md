---
title: kule rules
sidebar_label: kule rules
description: List, add and forget what you told Kule not to ask about again.
---

# kule rules

List, add and forget the **remembered rules**: what you told Kule not to ask
about again.

## Synopsis

```sh
kule rules [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-rules-list) | List them (the default). |
| [`allow`](#kule-rules-allow) | Let a connection, the changes to one file, or files of one kind through without asking. |
| [`forget`](#kule-rules-forget) | Ask again about what a rule let through. |

## Description

When the [policy](../concepts/policy.md) puts something to you, you can
answer it once for good. A remembered rule is one of three kinds:

- **A connection**: `HOST:PORT`, for example `api.example.com:443`. It
  becomes a Cedar `permit`, so connections there pass on their own.
- **A file**: a path relative to the folder, for example `package.json`,
  together with the rule that asks about it. It becomes a narrow exception to
  that rule for that one file. A hidden file (`dot-paths`) is answered this
  way, by its path.
- **A kind of file**: a lowercase extension without its dot (`vue`), or a
  name without one (`makefile`), that the `unfamiliar-kinds` rule asks
  about. From then on the kind counts as one the project has: files of that
  kind no longer wait for their kind.

Each holds for **one folder** (the default: the current directory) or for
**every folder** (`--everywhere`).

Some things are never remembered:

- what the policy **denies** (a new git hook, for example);
- what waits for a reason other than a file's name, place or kind: a mass
  delete, a history rewrite, a link, git config, a file made executable,
  changed extended attributes. Only the rules `manifests-and-lockfiles`,
  `project-files`, `unfamiliar-kinds` and `dot-paths` can be answered for
  good;
- what runs code on its own (`automation`: CI, editor and agent settings),
  and another version control system's files (`other-vcs`): these are asked
  every time. A file held by one of these and by another rule too is not
  remembered either;
- anything under `.kule/`, where a repository keeps its own rules;
- a repository's own rule (`repo:<id>`), whatever its `@id`: a remembered
  answer never cancels what the repository asks.

Under the careful policy, every file change also waits for the `careful` rule,
which cannot be remembered, so a remembered file does not pass there.

Remembered rules live in Kule's home (`remembered.json`), which no sandbox
can see. They hold for every group, whatever its policy.

All subcommands take `--home` (see [global options](index.md)).

## kule rules list

```sh
kule rules list [OPTIONS]
```

```console
$ kule rules
  1  connections to example.com:443  · ~/code/app
  3  connections to api.github.com:443  · every folder
```

With nothing kept: `nothing remembered: every question is asked`.

An answer an earlier version kept for the `automation` rule is still listed,
but no longer lets anything through. Forget it with
[`kule rules forget`](#kule-rules-forget):

```console
$ kule rules
  2  changes to .github/workflows/ci.yml (automation)  · ~/code/app · no longer applies: what runs code on its own is always asked
  4  files of kind vue (unfamiliar-kinds)  · ~/code/app
```

## kule rules allow

```sh
kule rules allow [OPTIONS] <WHAT>
kule rules allow [OPTIONS] --kind <KIND>
```

| Argument / option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `WHAT` | `HOST:PORT` or `PATH` | | A connection (a lowercase host name and a port, no wildcard) or a file path relative to the folder (no `..`). |
| `--rule` | rule id | `manifests-and-lockfiles` | For a file: the rule that asks about it. One of `manifests-and-lockfiles`, `project-files`, `unfamiliar-kinds`, `dot-paths`. |
| `--kind` | kind | | Files of this kind, instead of `WHAT`: an extension without its dot (`vue`) or a name without one (`makefile`), in lower case. |
| `--folder` | path | the current directory | The folder the rule holds for. |
| `--everywhere` | | | Hold for every folder. |
| `--yes` | | | Keep it. Without `--yes`, nothing is kept. |

`allow` first shows **what the rule would have let through so far** in your
past tasks: the connections that waited or were refused, and the commits and
uncommitted changes that touched the file. Only `--yes` keeps it. So you can
look before you decide:

```console
$ cd ~/code/app
$ kule rules allow example.com:443
connections to example.com:443 · ~/code/app
would have let through so far: 1 connections, 0 changes, in 1 tasks
not kept: rerun with --yes to keep it
$ kule rules allow example.com:443 --yes
connections to example.com:443 · ~/code/app
would have let through so far: 1 connections, 0 changes, in 1 tasks
kept as 1
```

Let changes to `package.json` in this folder through:

```console
$ kule rules allow package.json --yes
changes to package.json (manifests-and-lockfiles) · ~/code/app
would have let through so far: 0 connections, 0 changes, in 0 tasks
kept as 2
```

Files of one kind the project had none of, from now on:

```console
$ kule rules allow --kind vue --yes
files of kind vue (unfamiliar-kinds) · ~/code/app
would have let through so far: 0 connections, 3 changes, in 1 tasks
kept as 4
```

A hidden file, by its path:

```console
$ kule rules allow .prettierrc.json --rule dot-paths --yes
```

A file that the `automation` rule asks about, such as a CI workflow, cannot
be remembered: what runs code on its own is asked every time.

A host for every folder:

```console
$ kule rules allow api.github.com:443 --everywhere --yes
connections to api.github.com:443 · every folder
would have let through so far: 0 connections, 0 changes, in 0 tasks
kept as 3
```

:::warning

A connection rule lets *every* command in those tasks reach that host, not
only the agent. Git hosts are a common case: inside TLS, a push looks the
same as a fetch.

:::

## kule rules forget

```sh
kule rules forget [OPTIONS] <ID>
```

```console
$ kule rules forget 2
forgot 2: asked again from now on
$ kule rules forget 9
nothing remembered as 9
```

## Errors and fixes

| Message | What to do |
| --- | --- |
| `HOST:PORT: a lowercase host name and a port, without a wildcard` | Write one exact host, like `api.example.com:443`. |
| `PATH: a path relative to the folder, without ..` | Give the path as it is inside the folder. |
| `PATH: the repository's own rules are always put to a person` | Files under `.kule/` cannot be remembered. |
| `RULE: only a rule that asks about a file by its name, place or kind can be answered for good; what runs code on its own is always asked` | `--rule` takes `manifests-and-lockfiles`, `project-files`, `unfamiliar-kinds` or `dot-paths`. |
| `KIND: a kind of file in lower case, as an extension without its dot (vue) or a name without one (makefile)` | Give the kind as Kule names it, like `vue` or `makefile`. A hidden file is remembered by its path. |

## In the app

On a waiting connection, **Approve and remember** keeps a connection rule for
the task's folder. In Review, **Don't ask again in this folder** on a held
file shows first what it would have let through, then keeps a file rule.
Files that wait only for their kind are one group, with **Don't ask again
about files of kind X in this folder**, which keeps the kind. *Settings →
Rules* lists them all, each with **Forget**; an answer kept for `automation`
by an earlier version shows **No longer applies**. From the command line,
`kule commit --approve --remember` keeps what can be remembered of the
changes approved: a kind for files held for their kind, a path for the
others.

## See also

- [`kule policy`](policy.md), [Policy rules](../reference/policy-rules.md)
- [`kule commit`](commit.md): `--approve --remember`.
- [Network and questions](../concepts/network-and-questions.md)
