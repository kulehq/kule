---
id: agents
title: kule agents
sidebar_label: kule agents
description: List the agents Kule can run, inspect their profiles, and add, try, approve or remove your own.
---

# kule agents

List the agents Kule can run, see what each one's profile opens in the
sandbox, and add, try, approve or remove agents of your own.

## Synopsis

```sh
kule agents [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-agents-list) | List the agents, and why the ones that cannot run cannot (the default). |
| [`show`](#kule-agents-show) | Show what an agent's profile opens in the sandbox. |
| [`registry`](#kule-agents-registry) | List the agents of the ACP registry. |
| [`add`](#kule-agents-add) | Add an agent from the ACP registry as a draft profile. |
| [`try`](#kule-agents-try) | Run a profile once in an empty folder and see what the sandbox refused it. |
| [`approve`](#kule-agents-approve) | Let one of your profiles run, as it stands now. |
| [`revoke`](#kule-agents-revoke) | Take an approval back. |
| [`remove`](#kule-agents-remove) | Take one of your profiles out of `agents.toml`. |

## Description

Kule runs agents that speak the [Agent Client Protocol](https://agentclientprotocol.com)
(ACP). Each agent has a **profile**: the command that starts it, and what it
may open in the sandbox beyond a task's copy (a home of its own in Kule, its
login, the hosts it needs). Nothing it writes reaches your home.

- **Built in**: `claude-code`, `codex` and `gemini`.
- **Yours**: profiles in `agents.toml` in Kule's home (`~/.kule/agents.toml`).
- **From an extension**: profiles an approved [extension](../extensions.md)
  brings.

A profile opens holes in the sandbox, so **one of yours runs only once you
approve it**, and again after any change to it. It may never open the
credentials Kule hides, another agent's state, Kule's home, or a whole
directory like `~/Documents`, and a package it fetches must name one exact
version. Profiles never come from a workspace.

See [Agents](../using-agents.md) for every profile field and how to sign the
built-in agents in.

The usual way to add an agent:

```sh
kule agents registry            # find its id
kule agents add qwen-code       # a draft that opens nothing yet
kule agents try qwen-code --apply   # run it once; add what it was refused
kule agents show qwen-code      # read what it would open
kule agents approve qwen-code   # let it run
```

All subcommands take `--home` (see [global options](index.md)).

## kule agents list

```sh
kule agents list [OPTIONS]
```

```console
$ kule agents
claude-code      built in
gemini           built in
codex            built in
qwen-code        yours, held: not approved as it stands; review it with `kule agents show qwen-code`, then `kule agents approve qwen-code`

Add your own in ~/.kule/agents.toml (format: kule agents show claude-code).
```

`held:` says why one of your agents cannot run now: it is not approved as it
stands (never approved, or changed since), its profile opens something a
profile may not, or another profile has the same name.

## kule agents show

```sh
kule agents show [OPTIONS] <NAME>
```

Prints the profile as you would approve it, then any warnings.

```console
$ kule agents show codex
[[agent]]  codex
runs              npx -y @agentclientprotocol/codex-acp@2.2.2 -c sandbox_mode="danger-full-access"
writes in ~/      nothing
its login         nothing
hidden from it    .codex (only Kule reads there)
copied in         .codex/AGENTS.md, .codex/AGENTS.override.md, .codex/config.toml, .codex/plugins, .codex/prompts, .codex/rules, .codex/skills (at every start, read-only)
sees your env     nothing
also connects to  https://auth.openai.com, https://chatgpt.com
session mode      agent-full-access
home of its own   $CODEX_HOME points at Kule's folder for it: what it writes in Kule stays there
its own home      /Users/you/Library/Application Support/Kule/agents/codex

warning: Kule puts your OPENAI_API_KEY into what codex sends to api.openai.com.
```

| Line | Meaning |
| --- | --- |
| `runs` | The command that starts the agent over ACP. |
| `writes in ~/` | Folders and files under your home the agent writes its state to. It writes them in the task's copy of your home; none of it reaches your own. |
| `its login` | Its sign-in, hidden from every other agent. |
| `hidden from it` | Where it keeps its state when you run it without Kule. Only Kule reads there. |
| `copied in` | Your settings, hooks, skills and instructions, copied into its home in Kule at every start, read-only while it runs. |
| `protected` | From older profiles; no longer needed, since nothing it writes reaches your home. |
| `reveals (macOS)` | Hidden paths it may read on macOS, such as a keychain. |
| `sees your env` | Your environment variables it sees as they are. |
| `sets env` | Variables Kule starts it with. |
| `also connects to` | HTTPS hosts it reaches while it runs, beyond the policy. |
| `session mode` | The agent's own mode Kule starts it in, which leaves sandboxing to Kule. |
| `its own home` / `home of its own` | A folder of its own in Kule (see `own_home` in [Agents](../using-agents.md)). |
| `warning:` | Something to know before you approve, for example a key Kule fills in for it. |

The sandbox is not codex's own: `sandbox_mode="danger-full-access"` turns
off codex's sandbox *inside* Kule's, which is the one that holds.

## kule agents registry

```sh
kule agents registry [OPTIONS]
```

Lists the agents of the public [ACP registry](https://agentclientprotocol.com/registry),
each with its version and how it starts on this system. The list is kept in
Kule's home (`registry.json`); when the registry cannot be reached, the last
copy is shown with `(the registry could not be reached; as last read)`.

```console
$ kule agents registry
auggie                 0.36.0     npx -y @augmentcode/auggie@0.36.0 --acp
claude-acp             0.94.0     npx -y @agentclientprotocol/claude-agent-acp@0.94.0
goose                  1.55.0     download, then ./goose
qwen-code              0.25.3     npx -y @qwen-code/qwen-code@0.25.3 --acp --experimental-skills
cursor                 2026.10.18 not for this system
...
```

## kule agents add

```sh
kule agents add [OPTIONS] <ID>
```

Adds a **draft** profile from the registry to `agents.toml`. A draft opens
nothing yet: no state folder, no login, no hosts. It is held until you
approve it.

```console
$ kule agents add qwen-code
added qwen-code, opening nothing yet; next: kule agents try qwen-code --apply, then kule agents approve qwen-code
```

For an agent the registry ships as a download, `add` says which archive to
get, its SHA-256, and what to put on your `PATH`.

## kule agents try

```sh
kule agents try [OPTIONS] <NAME>
```

| Option | Meaning |
| --- | --- |
| `--apply` | Add what the trial suggests to the profile. The profile then waits for approval again. |

Runs one of **your** profiles once, in an empty folder, opened no wider than
the profile says (up to two minutes), then lists what the sandbox refused it:
the folders it wanted to write and the hosts it wanted to reach. That is
what its profile should open.

Its output has this shape (what is refused and suggested depends on the
agent):

```text
trying qwen-code in an empty folder (up to two minutes)…
it answered: …
refused  file-write-create /Users/you/.qwen/settings.json (node)
refused  connect example-model-api.com:443 (no-rule-lets-it-pass)
suggested state: .qwen
suggested hosts: example-model-api.com
added to qwen-code; review it with kule agents show qwen-code, then approve it
```

What it writes in the suggested state goes to each task's copy of your
home, never your own. An agent that reads where its state lives from a
variable can have a home of its own instead (`own_home`), kept across
tasks.

:::note

`try` really starts the agent, so it may ask for a sign-in and may use your
account with its provider. Linux does not report refused files, so there
only hosts are suggested.

:::

## kule agents approve

```sh
kule agents approve [OPTIONS] <NAME>
```

Prints the profile, then approves it **as it stands**. Any later change to
the profile, by hand or by `try --apply`, holds it again.

```console
$ kule agents approve qwen-code
[[agent]]  qwen-code
runs              npx -y @qwen-code/qwen-code@0.25.3 --acp --experimental-skills
...

approved qwen-code as it stands; a change to it needs approval again
```

## kule agents revoke

```sh
kule agents revoke [OPTIONS] <NAME>
```

```console
$ kule agents revoke qwen-code
qwen-code is no longer approved
```

## kule agents remove

```sh
kule agents remove [OPTIONS] <NAME>
```

Removes one of your profiles from `agents.toml`. Built-in agents cannot be
removed.

```console
$ kule agents remove qwen-code
removed qwen-code
```

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: no agent nope` | Check the name with `kule agents`. |
| `kule: no agent nope in the ACP registry` | Check the id with `kule agents registry`. |
| `kule: there is a profile named qwen-code already` | Edit or remove the existing profile first. |
| `kule: qwen-code has nothing to run on this system` | The registry offers no build for this OS or processor. |
| `kule: no profile qwen-code of yours` | `try` and `remove` work only on your own profiles, not built-in ones. |
| `held: not approved as it stands; …` | Read `kule agents show NAME`, then `kule agents approve NAME`. |

## In the app

*Settings → Agents* lists every agent with what it opens. **Try it** runs a
draft once and offers what it was refused as checkboxes, so you can leave a
telemetry host out; approving works the same way. The first-run screen says
which agents can start, and what to install for the others.

## See also

- [Agents](../using-agents.md): every profile field, and signing in.
- [`kule run`](run.md): give an agent a prompt.
- [Files and settings](../reference/files.md): `agents.toml`.
