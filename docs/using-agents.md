---
title: Agents
sidebar_label: Agents
description: The agents Kule runs out of the box, how they run inside the sandbox, and how to add any other ACP agent with a profile in agents.toml.
---

# Agents

An **agent** is the AI program that does the work in a task: Claude Code,
Codex, Gemini, or any other program that speaks the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP). Kule starts
the agent, gives it your prompt, and shows what it does. The agent runs
inside the task's [sandbox](concepts/sandbox.md), in the task's copy, with
its network going through the task's proxy.

## Built-in agents

| Agent | Name in Kule | What it needs | How it signs in |
| --- | --- | --- | --- |
| **Claude Code** | `claude-code` | Node.js (`npx`) | As without Kule: `claude` in a terminal, or `ANTHROPIC_API_KEY`. |
| **Codex** | `codex` | Node.js (`npx`) | `codex login` with a ChatGPT account, or `OPENAI_API_KEY`. |
| **Gemini** | `gemini` | The Gemini CLI (`gemini` on your `PATH`) | An API key from Google AI Studio (see below). |

`claude-code` is the default (`kule run TASK "prompt"` uses it). Pick
another with `kule run TASK --agent codex "prompt"`, or with the agent
picker under the prompt in the app.

Sign each agent in once, the way you do without Kule. Kule's runs use that
sign-in.
The app's first screen, and `kule agents list`, say which agents can start
and what is missing for the others.

### Claude Code

- Kule starts Claude Code through its ACP adapter, a pinned npm package
  (`@agentclientprotocol/claude-agent-acp`).
- On macOS its sign-in lives in the keychain, which stays hidden from the
  sandbox. Kule reads the token there itself and hands the agent a
  placeholder; the task's proxy puts the real token into requests to
  `api.anthropic.com` alone.
- `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN` and `CLAUDE_CODE_OAUTH_TOKEN`
  in your environment are handled the same way: the agent sees a
  placeholder, never the key.

### Codex

- Kule starts Codex through its ACP adapter
  (`@agentclientprotocol/codex-acp`).
- Codex's own command sandbox cannot be set up inside Kule's, so Kule starts
  it in full-access mode and Kule's sandbox stands in for it.
- A ChatGPT sign-in (`codex login`) works: it refreshes its token at
  `auth.openai.com` and reaches the model through `chatgpt.com`, which are
  open while Codex runs. `OPENAI_API_KEY` goes to `api.openai.com` alone, as
  a placeholder.
- Signed in with ChatGPT, Codex reports tokens but no cost, so a
  [budget](concepts/budgets-and-limits.md) cannot hold it.

### Gemini

- Gemini needs an **API key from Google AI Studio**. Google no longer serves
  its Code Assist sign-in to clients like this one.
- Save the key outside the keychain, which the sandbox keeps read-only, once:

  ```sh
  GEMINI_FORCE_FILE_STORAGE=true gemini
  # then, inside Gemini:
  /auth
  ```

- Or set `GEMINI_API_KEY` (or `GOOGLE_API_KEY`): the agent sees a
  placeholder, and the proxy puts the key into requests to
  `generativelanguage.googleapis.com` alone.
- Gemini reports neither tokens nor cost.

## How an agent runs

When you give a task a prompt (`kule run`, or **Run** in the app):

1. Kule starts the agent with its **profile**: the command that starts it,
   and the narrow extra room it needs in the sandbox (its own state
   directory and login, its hosts).
2. The agent works in the task's copy, inside the sandbox. Other agents'
   logins and transcripts stay hidden from it.
3. Every tool call it makes becomes a **step** (`kule steps TASK`, or the
   [Timeline](app/timeline.md)). Connections the policy puts to you wait as
   [questions](concepts/network-and-questions.md).
4. Nothing reaches your folder until you [commit](concepts/review-commit-undo.md).

### What an agent may not change for later

An agent's own directory (`~/.claude`, `~/.codex`, `~/.gemini`) is outside
the sandbox, and what the agent writes there in Kule reaches it the next
time you start it without Kule. So what makes an agent run something or
follow instructions when it starts is **read-only** in the sandbox:
settings, hooks, plugins, extensions, skills, commands, rules, trusted
folders and its `CLAUDE.md`, `AGENTS.md` or `GEMINI.md`. Kule's own profiles
cover these; for your own profiles, a trial suggests them and
`kule agents show` warns about any left writable.

Or give an agent a **home of its own** in Kule with `own_home` (see below):
nothing it writes in Kule then reaches it outside, and you sign it in there
once.

### Packages fetched with npx

An agent that `npx` fetches (Claude Code's and Codex's adapters) is installed
**once per version**, outside the sandbox and without running the package's
install scripts, from a lockfile Kule carries, every dependency at its
integrity hash. It goes under:

- macOS: `~/Library/Application Support/Kule/packages`
- Linux: `~/.local/share/kule/packages`

No sandbox can write there. The agent starts from there with `node`, so a
new task does not fetch it again into its own npm cache.

## Adding your own agent

Any agent that speaks ACP can be added as a **profile** in
`~/.kule/agents.toml` (in `KULE_HOME` if you set it). Kule's home is hidden
from every sandbox, so no agent can write a profile for itself, and profiles
are never read from a workspace.

### The quick way: from the ACP registry

```sh
kule agents registry            # list the agents of the ACP registry
kule agents add qwen-code       # take a draft profile from it, opening nothing yet
kule agents try qwen-code       # run it once in an empty folder; list what the sandbox refused
kule agents try qwen-code --apply   # add what it was refused to the profile
kule agents show qwen-code      # read what the profile opens, and any warnings
kule agents approve qwen-code   # let it run, as it stands now
```

`kule agents try` runs the profile once in an empty folder, opened no wider
than its draft, then lists what the sandbox refused it: the directories it
wanted to write to and the hosts it wanted to reach. `--apply` adds those to
the profile, which then waits for your approval like any change. Linux does
not report refused files, so there only hosts are suggested.

In the app, *Settings → Agents* does the same: **Add from the ACP registry**,
then **Try it**, with checkboxes so a telemetry host can stay out.

### By hand

Write the profile yourself, in the format `kule agents show claude-code`
prints:

```toml
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode"]          # what it may write, under ~
login = [".local/share/opencode/auth.json"] # hidden from every other agent
keys = { OPENROUTER_API_KEY = ["openrouter.ai"] } # your API key, never seen by it
hosts = ["openrouter.ai"]                   # HTTPS hosts it reaches, while it runs
```

Then `kule agents show opencode` and `kule agents approve opencode`.

### Approval

A profile opens holes in the sandbox, so one of yours runs **only once you
approve it** as it stands. Changed afterwards, by anyone, it waits for
approval again. `kule agents revoke NAME` takes an approval back;
`kule agents remove NAME` takes the profile out of `agents.toml`.

## `agents.toml` field reference

Each agent is one `[[agent]]` table. Unknown fields are refused. Paths are
relative to your home directory.

| Field | Type | What it is |
| --- | --- | --- |
| `name` | string, required | How the agent is asked for (`kule run --agent NAME`). Lowercase letters, digits and dashes. It may not be one of Kule's own agents' names. |
| `command` | list of strings, required | Starts the agent speaking ACP on stdio. A package started with `npx`, `bunx`, `pnpx`, `uvx` or `pipx` must name **one exact version** (`npx -y name@1.2.3`). |
| `state` | list of paths | Where the agent writes its own state, under `~`. It may not be a directory that holds far more than one agent's (`.config`, `.local/share`, `.cache`, `Library/Application Support`, `Documents`, …), nor something that runs as you start a shell, a session or a tool (`.zshrc`, `.gitconfig`, `.config/fish`, `.local/bin`, `Library/LaunchAgents`, …). |
| `protected` | list of paths | Paths in its state that configure what runs outside the sandbox later (settings, hooks, MCP servers, plugins, instructions): readable, never writable. |
| `login` | list of paths | Its login, inside its state. Hidden from every other agent and from commands Kule runs itself. |
| `keys` | table: variable → list of hosts | Your environment variables that hold its API keys, each with the hosts it is for. The agent's variable holds a placeholder; the task's proxy puts the key into requests to those hosts alone, over HTTPS. |
| `keychain` | table: variable → `{ service, json, hosts }` | macOS: a key the agent keeps in the keychain, by the variable it reads it from. Kule reads it there (`service` is the keychain item; `json` the dotted path inside the item's JSON, if any) and hands the agent a placeholder, as for `keys`. A key of the same name in your environment wins. |
| `hosts` | list of hosts | HTTPS hosts it needs for its login and model, beyond what the policy lets through. Open only while this agent runs. |
| `env` | table: variable → value | Variables the agent is started with. |
| `pass_env` | list of variable names | Your environment variables the agent may see as they are. Others stay out. Prefer `keys` for anything secret. |
| `own_home` | variable name | A home of its own in Kule, for an agent that reads where its state lives from a variable (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, or `HOME` itself). Kule points the variable at a folder of the agent's own, so what it writes in Kule never reaches it outside; you sign it in there once. Not `PATH`, `TMPDIR`, `TMP`, `TEMP`, `PWD` or `SHELL`. |
| `reveal_on_macos` | list of paths | Default-hidden paths it needs to read on macOS. Only `Library/Keychains` may be revealed. |
| `mode` | string | The ACP session mode to start in, when the agent offers one: for an agent whose own sandbox cannot start inside Kule's. |
| `devices` | list of paths | Linux: devices under `/dev` the agent and its commands need beyond those every command gets, such as `/dev/snd`, `/dev/video0` or a serial port. Kule refuses those that reach what is yours: disks, memory, keyboard and mouse, consoles and terminals. |

Own home folders live outside Kule's home, which the sandbox hides whole:
`~/Library/Application Support/Kule/agents` on macOS and
`~/.local/share/kule/agents` on Linux.

### What a profile may never open

Kule checks every profile before it can run. A profile may never open:

- the credentials Kule hides (`~/.ssh`, `~/.aws`, …), except the macOS
  keychain through `reveal_on_macos`;
- another agent's state or login;
- Kule's own home;
- a whole directory like `~/Documents`, `~/.config` or `~/Library`;
- what runs as you start a shell, a session or a tool;
- a package without one exact version.

### Devices on Linux

Every command in a task gets these devices by name: `/dev/null`, `zero`,
`full`, `tty`, its own terminals, `random` and `urandom` to read, and the
GPU's unless the GPU [limit](concepts/budgets-and-limits.md) turns it off.
Never `/dev` whole, where a camera, a microphone or a USB device of your
session would be. An agent that needs more names it under `devices`.

## From a terminal

See [`kule agents`](cli/kule-agents.md) for every subcommand, and
[`kule run`](cli/run.md) to give an agent a prompt. In the app, see
[Settings](app/settings.md) → Agents.
