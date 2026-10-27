---
title: kule secrets
sidebar_label: kule secrets
description: Keep API keys that a task's commands use without ever seeing them.
---

# kule secrets

Keep **secrets**: API keys and tokens a task's commands can use without ever
seeing them.

## Synopsis

```sh
kule secrets [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-secrets-list) | List the kept secrets, without their values (the default). |
| [`add`](#kule-secrets-add) | Keep a secret for one or more hosts. |
| [`remove`](#kule-secrets-remove) | Forget a secret. |

## Description

A kept secret has a **name** (an environment variable, like `STRIPE_KEY`)
and the **hosts** it may go to (like `api.stripe.com`). In a task:

1. The commands find a **placeholder** in the variable, for example
   `kule_secret_4f7567ba…`, never the value.
2. The task's [proxy](../concepts/network-and-questions.md) looks inside
   HTTPS requests to those hosts only, and puts the real value in place of
   the placeholder in a credential header (`Authorization`, `x-api-key`,
   `x-goog-api-key` and the like).
3. Anywhere else (the URL, other headers, the body, any other host) the
   placeholder stays a placeholder, which opens nothing.

The connection log names the secrets that went out:
`net: api.stripe.com:443 · tunneled · with STRIPE_KEY`.

Where the value lives: in Kule's home (`secrets.json`), hidden from every
sandbox, sealed with a key that your system keeps (the macOS keychain,
Secret Service on Linux, the Credential Manager on Windows). Where the
system keeps none (a computer without a keyring, a Windows session over
SSH), or with a Kule home other than the default, the key is kept in a file
beside it (`secrets.key`, readable by you alone), and *Settings → Secrets*
says so.

Values in the copy's `.env` files that look like secrets are masked the same
way, without you adding them. See [Secrets](../concepts/secrets.md) for the
full picture and what is not covered yet.

A secret holds for **one folder** (the default: the current directory) or
**every folder** (`--everywhere`).

All subcommands take `--home` (see [global options](index.md)).

## kule secrets list

```sh
kule secrets list [OPTIONS]
```

```console
$ kule secrets
STRIPE_KEY  → api.stripe.com  · ~/code/app
GH_TOKEN  → api.github.com, uploads.github.com  · every folder
```

With none: `no secrets kept`.

## kule secrets add

```sh
kule secrets add [OPTIONS] --host <HOSTS> <NAME>
```

| Argument / option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `NAME` | variable name | | The environment variable it goes in, like `STRIPE_KEY`: capitals, digits and `_`. |
| `--host` | host name | | A host it may go to. Repeat for more. At least one. |
| `--folder` | path | the current directory | The folder it is for. |
| `--everywhere` | | | Every folder. |

The value is **never** an argument, so it stays out of your shell history and
the process list. At a terminal, Kule asks for it without echo; otherwise it
reads the first line of standard input.

```console
$ cd ~/code/app
$ kule secrets add STRIPE_KEY --host api.stripe.com
value of STRIPE_KEY (not shown):
kept STRIPE_KEY: commands see a placeholder; the value goes only to api.stripe.com
```

From a password manager, through a pipe:

```console
$ op read "op://dev/github/token" | kule secrets add GH_TOKEN \
    --host api.github.com --host uploads.github.com --everywhere
kept GH_TOKEN: commands see a placeholder; the value goes only to api.github.com, uploads.github.com
```

What a command in the task sees:

```console
$ kule exec 1046fb30 -- sh -c 'echo $STRIPE_KEY'
kule_secret_4f7567ba95b5beb31fccc18345b34260
```

## kule secrets remove

```sh
kule secrets remove [OPTIONS] <NAME>
```

| Option | Meaning |
| --- | --- |
| `--folder FOLDER` | The folder it was kept for (default: the current directory). |
| `--everywhere` | The one kept for every folder. |

```console
$ kule secrets remove STRIPE_KEY
forgot STRIPE_KEY
$ kule secrets remove GH_TOKEN --everywhere
forgot GH_TOKEN
```

Removing from the wrong place says `no secret STRIPE_KEY kept there`.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: lower: a secret's name is an environment variable's, like STRIPE_KEY` | Use an upper-case variable name. |
| `kule: *.stripe.com: a lowercase host name, without a wildcard or a port` | Name each exact host with its own `--host`. |
| `kule: no value given` | Type the value, or pipe it in; an empty line is not a value. |
| `error: the following required arguments were not provided` … `--host <HOSTS>` | Add at least one `--host`. |

## In the app

*Settings → Secrets* adds, lists and removes secrets, and says where the key
that seals them is kept.

## See also

- [Secrets](../concepts/secrets.md)
- [Network and questions](../concepts/network-and-questions.md)
- [Files and settings](../reference/files.md)
