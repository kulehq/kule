---
title: kule ext
sidebar_label: kule ext
description: Make, pack, install, approve and manage Kule extensions and the MCP servers they offer.
---

# kule ext

Make, pack, install, approve and manage **extensions**, and the MCP servers
they offer agents.

## Synopsis

```sh
kule ext [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-ext-list) | List extensions, and why the ones that cannot run cannot (the default). |
| [`new`](#kule-ext-new) | Start an extension: a folder that installs as it is. |
| [`info`](#kule-ext-info) | Show what a `.kulext` or a folder asks for, without installing it. |
| [`pack`](#kule-ext-pack) | Pack a folder into a `.kulext` file. |
| [`install`](#kule-ext-install) | Install or update a `.kulext`. |
| [`dev`](#kule-ext-dev) | Install a folder you are working on, read live. |
| [`approve`](#kule-ext-approve) | Let an extension run, as it stands now. |
| [`enable`](#kule-ext-enable) / [`disable`](#kule-ext-disable) | Turn an approved extension on or off. |
| [`remove`](#kule-ext-remove) | Remove an extension and what it kept. |
| [`mcp`](#kule-ext-mcp) | The MCP servers extensions offer agents. |

## Description

An **extension** adds commands, panels, themes, settings, notes on a review,
agent profiles or MCP servers to Kule. It is JavaScript in a folder, shared
as a `.kulext` file. Kule trusts it no more than an agent:

- it asks for **permissions** from a closed list, and runs only once you
  approve **exactly the version you saw** (by its digest);
- any change to it waits for approval again;
- its code runs in sandboxed frames that reach nothing but Kule.

No permission reaches secrets, answers to questions, commit, rollback, undo
or policies. Agents and MCP servers it brings each wait for their own
approval. [Extensions](../extensions.md) covers the manifest, the
permissions, the SDK and the package format.

When the app is open, `kule ext` makes its changes through the daemon, so the
app sees them at once; otherwise it changes Kule's home directly.

All subcommands take `--home` (see [global options](index.md)).

## kule ext list

```sh
kule ext list [OPTIONS]
```

```console
$ kule ext
me.hello                         0.1.0      Hello — runs
```

The state is `runs`, `turned off`, or `held: …` with the reason, such as
`not approved yet` or `it changed since you approved it`. `(dev)` marks one
installed from a folder. With none: ``no extensions; `kule ext install FILE.kulext` adds one``.

## kule ext new

```sh
kule ext new [OPTIONS] <ID> [DIR]
```

| Argument | Meaning |
| --- | --- |
| `ID` | `publisher.name`, each part 1 to 20 of `a-z`, `0-9` and `-`. |
| `DIR` | Where to write it. Default: the name part of the id (`hello` for `me.hello`). |

Writes a working extension: `kule-extension.json` (the manifest), `main.js`,
a panel (`panel.html`, `panel.js`, `panel.css`), `kule-ext.d.ts` (the SDK's
types) and a `README.md`.

```console
$ kule ext new me.hello
wrote me.hello into hello
try it: kule ext dev hello, then kule ext approve me.hello
```

## kule ext info

```sh
kule ext info [OPTIONS] <PATH>
```

Reads a `.kulext` file or an extension's folder as Kule would, and prints
what it asks for. Nothing is installed.

```console
$ kule ext info me.hello-0.1.0.kulext
me.hello 0.1.0 — Hello
publisher:   me (not verified by anyone)
about:       What it does, in a sentence.
digest:      96e2ff19826845575cd1ea734e7b657615c53eb06107c3f12dd8230086f11af5
may:         tasks.read — See your tasks: their names, folders and state
command:     Hello: say hello (ext.me.hello.hello)
panel:       Hello
```

| Line | Meaning |
| --- | --- |
| `publisher` | The name in the manifest. Nobody checks it: anyone can write any name. |
| `digest` | The package's SHA-256. Approval is tied to it. A folder shows a `dev-` digest. |
| `may` | Each permission it asks for, in words. `(new)` marks one it did not have when you approved it. |
| `reaches` | A host it may connect to. |
| `warning: it can read your work and send data to the hosts above` | It has both a network permission and one that reads your work. |
| `command`, `panel`, `theme` | What it adds to the window. |

## kule ext pack

```sh
kule ext pack [OPTIONS] <DIR>
```

| Option | Meaning |
| --- | --- |
| `-o`, `--out OUT` | Where to write it. Default: `<id>-<version>.kulext` in the current directory. |

```console
$ kule ext pack hello
packed me.hello 0.1.0 into me.hello-0.1.0.kulext
digest 96e2ff19826845575cd1ea734e7b657615c53eb06107c3f12dd8230086f11af5
```

The same folder always packs to the same bytes, so the digest is stable.
Publish it beside the file, so people can check it with `kule ext info`
before they install.

## kule ext install

```sh
kule ext install [OPTIONS] <FILE>
```

Installs a `.kulext`, or updates the installed one with the same id. It runs
only once approved.

```console
$ kule ext install me.hello-0.1.0.kulext
me.hello 0.1.0 — Hello
publisher:   me (not verified by anyone)
...
held:        not approved yet

installed; `kule ext approve me.hello` lets it run
```

An update holds the extension again until you approve the new version:

```console
$ kule ext install me.hello-0.2.0.kulext
me.hello 0.2.0 — Hello
...
may:         review.read — Read a task's changes and their diffs  (new)
held:        it changed since you approved it
```

## kule ext dev

```sh
kule ext dev [OPTIONS] <DIR>
```

Installs a folder you are working on, **read live**: code changes run as they
are, while a changed manifest waits for approval again. Because unapproved
code runs from it, the folder must be somewhere no agent can write: not under
`/tmp`, `/var/tmp`, `/dev/shm`, Kule's home (the tasks' copies), or an
agent's own folders.

```console
$ kule ext dev hello
...
installed from hello; `kule ext approve me.hello` lets it run
```

## kule ext approve

```sh
kule ext approve [OPTIONS] <ID>
```

| Option | Meaning |
| --- | --- |
| `--yes` | Approve what is shown without asking. Needed when there is no terminal (a script). |

Shows what the extension asks for, then asks `Let me.hello run as shown?
[y/N]`. What is approved is what was shown: if it changed in between, the
approval is refused.

```console
$ kule ext approve me.hello --yes
me.hello 0.1.0 — Hello
...
approved me.hello as it stands; a change to it needs approval again
```

## kule ext enable

```sh
kule ext enable [OPTIONS] <ID>
```

```console
$ kule ext enable me.hello
me.hello runs
```

If it is still held (not approved, say), it prints `me.hello is on, but
held: …`.

## kule ext disable

```sh
kule ext disable [OPTIONS] <ID>
```

Stops it without removing it.

```console
$ kule ext disable me.hello
me.hello is turned off
```

## kule ext remove

```sh
kule ext remove [OPTIONS] <ID>
```

Removes it, its package, its settings and what it stored.

```console
$ kule ext remove me.hello
removed me.hello and what it kept
```

## kule ext mcp

```sh
kule ext mcp [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| `list` | List the MCP servers that running extensions offer (the default). |
| `approve SERVER [--yes]` | Let agents use `EXTENSION/NAME`, as it stands now. Shows the command, then asks. |
| `revoke SERVER` | Stop offering `EXTENSION/NAME` to agents. |

An extension may offer agents an **MCP server**. Each one waits for its own
approval. An agent starts an approved server **inside its own sandbox**, so
the server reads what the agent can, the agent's sign-in included.

```console
$ kule ext mcp
An agent starts each inside its sandbox: it reads what the agent can, its sign-in included.

acme.db/db — waits for you
  starts: npx -y @acme/db-mcp@1.2.3
$ kule ext mcp approve acme.db/db --yes
acme.db/db starts: npx -y @acme/db-mcp@1.2.3
An agent starts it inside its sandbox: it reads what the agent can, its sign-in included.
agents get acme.db/db from their next run: npx -y @acme/db-mcp@1.2.3
$ kule ext mcp revoke acme.db/db
agents no longer get acme.db/db
```

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: "Bad_Id" must be publisher.name` | Use lower-case `publisher.name`. |
| `kule: no extension me.hello` | Check the id with `kule ext`. |
| `kule: no terminal to ask on; add --yes to approve what is shown above` | In a script, read the output first, then pass `--yes`. |
| `… is under /private/tmp, where agents can write; an extension under development must sit where only you can change it` | Move the folder somewhere of yours, such as `~/src/hello`, or pack and install it instead. |
| `kule: name the server as EXTENSION/NAME` | Write the server as `acme.db/db`. |
| `kule: no extension that runs offers acme.db/db` | The extension is not installed, not approved or turned off. |

## In the app

*Settings → Extensions* installs, approves, turns on or off and removes
extensions, and approves the MCP servers they offer. The app can also start
with every extension off: `--no-extensions`, or `KULE_NO_EXTENSIONS=1`.

## See also

- [Extensions](../extensions.md): manifest, permissions, SDK, package format.
- [`kule agents`](kule-agents.md): agent profiles an extension brings wait there.
- [Files and settings](../reference/files.md): `extensions/`.
