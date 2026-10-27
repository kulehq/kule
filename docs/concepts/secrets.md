---
title: Secrets
sidebar_label: Secrets
description: How Kule lets a task's commands use API keys without ever seeing them - placeholders, the proxy, and masked .env files.
---

# Secrets

## The idea

An agent often needs an API key: to call Stripe, to push to a registry, to
reach its own model. If the key is in its environment, the agent (or a
prompt injection) can print it, write it to a file, or send it anywhere.

With Kule, the agent **never sees the value**. It sees a **placeholder**,
and the task's proxy swaps the placeholder for the real value only in
requests to the hosts you named.

## Keep a secret

```sh
kule secrets add STRIPE_KEY --host api.stripe.com
```

- The value is read from the terminal **without echo**, or from standard
  input (`… < key.txt`). Never from the command line, where it would land in
  your shell history.
- `--host` names where it may go (repeat it for several hosts).
- By default a secret is for the folder you run the command in; use
  `--folder PATH` for another, or `--everywhere` for every folder.

```sh
kule secrets              # list them, without values
kule secrets remove STRIPE_KEY
```

In the app: *Settings → Secrets*.

## What a command sees

```sh
kule exec TASK -- sh -c 'echo $STRIPE_KEY'
# prints a placeholder, not your key
```

On the way to `api.stripe.com`, the proxy puts the value in place of the
placeholder in a **credential header**: `Authorization` (`Basic`
included), `x-api-key`, `x-goog-api-key` and the like. Everywhere else it
stays a placeholder:

- in the URL, in any other header, and in the body, so the value never
  reaches what could repeat it back;
- sent to any other host, it is a string that opens nothing.

A request to that host carrying credentials Kule does not hold for it is
refused (403), so nothing goes out under an account that is not yours:

- a credential header, a cookie, or a header or URL parameter named as a
  credential (`token`, `key`, `secret`, `password`, `auth`, `credential` and
  the like, by any spelling: `private_token`, `JOB-TOKEN`, `apiKey`) holds
  only Kule's placeholder;
- with `Basic`, the placeholder is the password, under any user name, or
  the user name with no password;
- a body with fields (a form, JSON, a multipart form, text) is read whole,
  up to 1 MB, and the same holds for its fields, at the top of JSON and in
  objects within objects; one too large or chunked to read is refused.

A refused request is answered by Kule, never sent on, and the connection
is logged as `cut`, with why in Kule's words:
`Kule refused a request in it: a credential Kule does not hold for this host`,
or `… a body Kule checks whole that was chunked or over 1 MiB`. Its `net:`
line says `cut` (see [Network and questions](network-and-questions.md)).

The proxy asks that host for whole responses only (no `Range`). A response
that repeats the value has it blanked out with `*`, as far as the proxy
can read it: as it is, escaped as JSON or a URL writes it, or in base64,
even split between chunks.

The connection log names the secrets that went out:

```text
net: api.stripe.com:443 · tunneled · with STRIPE_KEY
```

To do this, the proxy looks inside TLS for the secret's hosts and no
others. It makes a certificate authority of its own for each run, valid
only for those hosts by its name constraints, with a key that never leaves
its memory, and points `SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`,
`REQUESTS_CA_BUNDLE` and the like at it.

## Agents' API keys

The built-in agents take `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` and
`GEMINI_API_KEY` the same way: the agent's variable holds a placeholder of
the run, and the proxy fills in your key for the agent's own API host. A
profile you write can do the same under `keys` (see [Agents](../using-agents.md)).

## `.env` files are masked too

Values in the copy's `.env` files (`.env`, `.env.local`, `*.env`, `.envrc`;
not `.env.example`) that look like secrets, by their name or their shape,
are replaced with placeholders. A switch (`true`, `off`), a short number
and a search path under a key that names one (`PATH`, `GOPATH`,
`JAVA_HOME`) are settings and stay as they are, unless the key names a
secret or a secret Kule keeps or masked is in the path.

- when the task is made, after each commit, rollback and undo, and before
  every run;
- the **folder keeps the real ones**: a commit puts them back into a
  settings file the agent changed, and merges with the real values;
- the task keeps the values it masked in its own `secrets.json`, sealed;
- a masked value that is also a kept secret gets that secret's placeholder,
  so it reaches the secret's hosts. Any other stays a placeholder;
- a kept secret is masked wherever it is in these files, not only as a
  whole value: inside a longer value, an alias, a command or a comment,
  and as a URL or JSON writes it. Secrets kept for other folders are
  masked too, under placeholders no host fills. A line Kule masked this
  way goes back only where it is unchanged, and no more times than the
  file held it.

A settings file Kule cannot mask, because it is not UTF-8 (a Latin-1
byte, a UTF-16 file from a Windows editor) or is larger than 1 MB, would
show its values as they are. So no agent run, `kule exec`, `kule shell`
or terminal starts in the task while the copy holds one; Kule names the
file and why. Save it as UTF-8 under 1 MB, or move it out of the folder,
and try again.

The same masking applies to the [home layer](home-layer.md)'s clones of
`.zshrc`, `.profile`, `~/.env` and the like. One there Kule cannot mask is
withheld instead: the commands see it neither in the layer nor at its real
path, and Kule says so where they run (the run's notes in the app, the
output of `kule run`, `exec` and `shell`, and the terminal).

## When git holds the values

A task's copy carries the folder's git repository. A value Kule masks that
is in it reaches a command in the task through git (`git show HEAD:.env`,
`git log -p`), whatever the copy's file says. That is so when a `.env` file
was ever committed, even after it was taken out of the repository: the
commit that added it, a reference's log (a commit amended or reset away),
a stash, and an object git keeps outside the history all still hold it.

So before an agent run, `kule exec`, `kule shell` or a terminal starts,
Kule looks for the values it masks, and the secrets kept for the folder,
in the repository: its index, every branch, tag and stash, the whole
history of the `.env`-style files, the references' logs, and every object
git keeps outside the history. If one is there, nothing starts. Kule names
the files and where git holds them, and you choose:

- **Keep them from the task:** take them out of the repository's history,
  and change them, since every clone holds the old ones.
- **Accept it for the folder:** `kule secrets history TASK --accept`, or
  the app's question. Commands start in the folder's tasks again, and each
  time they do Kule says the values are in git: in the run's notes, in the
  output of `kule run`, `exec` and `shell`, and in the terminal. A value
  found later asks again. `kule secrets history TASK --forget`, or
  *Settings → Secrets*, takes it back.

A look that does not finish in a few seconds (a very long history) decides
nothing: Kule says how far it got, and asks the same way. Each look is kept
in the task, and made again when the repository's references, its index or
the values change.

## Where secrets are stored

Secrets live in Kule's home, hidden from every sandbox. Their values are
**sealed** with a key that your system's keychain (macOS), Secret
Service (Linux) or the Credential Manager (Windows) keeps. Where the system
keeps none (a computer without one, a Windows session over SSH), or with a
Kule home other than the default, the key is kept in a file beside;
*Settings → Secrets* says so.

## What it does not do yet

- HTTP/2 (clients fall back to HTTP/1.1), and WebSocket upgrades.
- A placeholder split by a chunk edge, or in a body larger than 1 MiB.
- Tools on macOS that check certificates only through the system's own
  services (pip 24.2 and later without `--use-deprecated=legacy-certs`).
  Kule never adds its authority to the keychain.

## Related

- [`kule secrets`](../cli/secrets.md)
- [Network and questions](network-and-questions.md)
- [Settings](../app/settings.md)
- [Security model](../security.md)
