---
title: Files and settings
sidebar_label: Files and settings
description: Every file and folder Kule keeps, where it lives, what it holds, and whether you may edit it by hand.
---

# Files and settings

Everything Kule keeps lives in **Kule's home**, `~/.kule` by default
(`%USERPROFILE%\.kule` on Windows). Set `KULE_HOME` (or pass `--home`) to
use another one; see
[Environment variables](environment.md). No sandbox can see Kule's home: a
task's commands and agents cannot read or write anything on this page except
their own copy and temporary files.

A few things live outside it: the packages Kule installs for agents, the
agents' own homes, and a repository's own rules. They are listed at the end.

:::tip Editing by hand

The files marked **edit: yes** are meant for you to write: `policy.cedar`,
`agents.toml` and a repository's `.kule/policy.cedar`. Everything else is
Kule's own state. Change it through the app or the `kule` commands; a file
edited by hand while the daemon runs may be written over, and a broken one
can make Kule refuse to work until it is fixed or removed.

:::

## Kule's home

| Path | What it holds | Edit |
| --- | --- | --- |
| `tasks/<id>/` | One folder per task. See [A task's folder](#a-tasks-folder). | no |
| `trash/` | Deleted tasks, on their way out. Kule empties it in the background right away and whenever the daemon starts. | no |
| `policy.cedar` | **Your own** policy for every task, in place of Kule's. Kule never writes or overwrites it. A file holding the balanced or careful text an earlier version wrote (0.2.0-preview.1 to preview.11) counts as that choice and is judged by today's rules. See [Policy rules](policy-rules.md). | **yes** |
| `policy-choice` | `careful` when you chose Kule's careful policy (`kule policy careful`); absent under balanced. A choice, never the text, so it always means this version's rules. | no: choose with `kule policy` |
| `group-policies/<id>.cedar` | A group's own policy, kept by `kule group policy ID own --file PATH` after it was checked. | no: set it again with the command |
| `groups.json` | Groups: id, name, colour, policy choice, budgets and limits. | no |
| `order.json` | The order of tasks in the app and in `kule list`. | no |
| `limits.json` | The limits for every task (`kule limits`). | no |
| `budget.json` | The daily budget for every task (`kule daily-budget`). | no |
| `budget.lock` | Held while Kule sets aside a run's share or records what a run spent, so its processes do it one at a time. | no |
| `remembered.json` | What you told Kule not to ask about again (`kule rules`). | no |
| `beside.json` | Projects beside a folder that its tasks may read (`kule reveal`). | no |
| `isolation.json` | Whether you accepted weaker isolation (`kule isolation`). | no |
| `secrets.json` | Kept secrets: names, hosts, folders, and values **sealed** (encrypted). Readable by you alone. | no |
| `secrets.key` | The key that seals the secrets, **only** when the system does not keep it: a Kule home other than `~/.kule`, a computer without a keychain or Secret Service, and every Windows computer. For `~/.kule` the key is in the macOS keychain (item `Kule secrets`) or Secret Service on Linux. The file is readable by you alone. | no |
| `kule.key` | The key that signs every task's history (`kule history`). | no |
| `agents.toml` | Your agent profiles. Each runs only once approved, and again after any change. Format: `kule agents show claude-code`; fields: [Agents](../using-agents.md). | **yes** |
| `agents-approved.json` | Which profile, exactly as it was, you approved. | no |
| `registry.json` | The last copy of the ACP registry (`kule agents registry`). | no: a cache |
| `extensions/` | Extensions. See [below](#extensions). | no |
| `shortcuts.json` | Your keyboard shortcuts (Settings → Shortcuts). | no |
| `themes.json` | The theme in use and the themes you made (Settings → Appearance). | no |
| `layout.json` | Where the window's panels are. *Reset the layout* goes back to Kule's own. | no |
| `editors.json` | The editor you last opened a copy in, and the editors whose first-time note you have seen. | no |
| `kept/` | Linux: what Kule moved aside when it put an agent's settings back after a run (`kept/replaced/…`). Never deleted by Kule. | you may delete old entries |
| `kuled.sock` | The daemon's socket. Only its owner can connect. | no |
| `kuled.lock` | Held by the running daemon, so only one runs per home. | no |
| `kuled.token` | The token every request to the daemon carries; made fresh at each start. | no |
| `kuled.log` | Output of a daemon the app started. | read only |

### Extensions

Under `extensions/`:

| Path | What it holds |
| --- | --- |
| `installed.json` | Each installed extension: version, digest, on or off, and the folder of one under development. |
| `approved.json` | For each, the digest, permissions and hosts you approved. |
| `mcp-approved.json` | The MCP servers you let agents use, by digest. |
| `packages/<id>-<digest>.kulext` | The packages, kept as they are, never unpacked. |
| `data/<id>/` | What the extension stores (`storage.json`) and its settings (`config.json`). |

See [Extensions](../extensions.md).

## A task's folder

Each task lives in `tasks/<id>/`, where `<id>` is the 8-character id
`kule list` prints.

| Path | What it holds |
| --- | --- |
| `task.json` | The task's id, the workspace (your folder), when it was made, and the copy's folder name. |
| `caches.lock` | Held while the task's package caches are made, so two Kule processes never make them at once. |
| `settings.json` | The task's name and colour, its group, its budget, its own limits, how long questions wait, and what the copy took when it was made (disk limits count from there). |
| `w/<folder name>/` | **The copy**: where the agent works. `kule path TASK` prints it. Beside it are links to projects you opened with `kule reveal`. |
| `base/` | The folder as it was at the last sync, for three-way merges. |
| `baseline.json` | Where the workspace and the copy last agreed, file by file: Kule compares both with it to find what each changed. |
| `tmp/` | The task's `TMPDIR`. Inside: `caches/` (its own package caches: npm, cargo, pip, uv, Go, NuGet, Yarn, pnpm, …), `kule-ca.pem` and `kule-ca-bundle.pem` (the proxy's certificate authority for this task), `.kule-loopback/` (what holds servers to loopback). |
| `home/` | The **home layer**: what the task's commands see as your home. See [Home layer](../concepts/home-layer.md). |
| `home.json` | What the home layer was made from, to tell what the task changed. |
| `home-kept/` | What `kule home TASK apply` replaced in your real home, one folder per apply, for `kule home TASK undo`. |
| `steps.jsonl`, `steps-mark.json`, `steps-store/` | The uncommitted **steps** (each tool call's changes) and what going back to a step needs. |
| `checkpoints/` | One numbered folder per commit (`000001/checkpoint.json`, …), and `store.git`: the Lonca store with what each commit replaced, and the signed history (`refs/kule/log`). |
| `network.jsonl` | Every connection the task's commands made through the proxy, refused ones included: when, host, port, verdict, rules, `outcome` (`tunneled`, `refused`, `cut`, `failed`, `unresolved`, `private`, `unreachable`), your answer when one was asked, the secrets that went in, and why in `cut` or `failed`. |
| `sandbox.jsonl` | What the sandbox refused (macOS reports it; Linux does not). |
| `usage.jsonl` | What each agent run used, as the agent reported it: tokens, context, cost; and the share it closes (`reservation`). A line with `released: true` is a share you let go of with `kule budget TASK --release`, not a run. |
| `reserved.jsonl` | What each run set aside of the budgets before its agent started. A share counts as spent until a line in `usage.jsonl` closes it. |
| `secrets.json` | The values Kule masked in the copy's `.env` files, sealed like kept secrets. |
| `synced.json` | What the last `kule sync` brought in, for the agent to hear at its next turn. |
| `refused-hints.json` | Which "why a tool failed" notes the agent was already told. |

None of these are for editing by hand. Delete a task with `kule rm TASK`,
never by removing its folder.

### Why `~/.kule` looks large

On macOS (APFS), on Linux with Btrfs or XFS, and on Windows with ReFS or a
Dev Drive, the copy, `base/` and the cached packages are **clones** that
share disk blocks with your files. Finder, Explorer and `du` count them at
full size anyway; look at the free space (`df -h`) to see what is really
used.

## Outside Kule's home

| Path | What it holds |
| --- | --- |
| `~/Library/Application Support/Kule/packages` (macOS), `~/.local/share/kule/packages` (Linux and Windows, under your profile) | Agents that Kule installs with npm (Claude Code's and Codex's ACP adapters), one folder per exact version, installed without running install scripts. No sandbox can write here. |
| `~/Library/Application Support/Kule/agents/<name>` (macOS), `~/.local/share/kule/agents/<name>` (Linux and Windows) | A **home of its own** for an agent whose profile sets `own_home`, as Kule's own agents do: its settings, history and transcripts in Kule, kept across tasks. Kule copies your settings and instructions in at every start (`carry`). |
| `%LOCALAPPDATA%\Kule` (Windows) | Kule's programs, as the installer puts them: the app, `kule.exe`, `kule-proxy.exe` and `kule-pty-helper.exe`. Not Kule's home, and never to be made it. |
| `<workspace>/.kule/policy.cedar` | A repository's own **forbid** rules. It may only tighten the policy. Kule reads it from your real folder, never from a task's copy, and every change to anything under `.kule/` waits for a person. **Edit: yes.** |

## Files you move between computers

| File | What it is |
| --- | --- |
| `*.kule-profile.json` | Your choices in one file (Settings → Profile): themes, shortcuts, the balanced or careful choice, the daily budget, limits and your agent profiles. Never secrets, remembered approvals, or a policy you wrote. On import Kule shows what it holds and you pick the parts; its agents wait for your approval. |
| `*.kule-theme.json` | One theme: a `#rrggbb` colour for each named part of the window, and nothing else. Checked on the way in. |
| `*.kulext` | An extension package. See [Extensions](../extensions.md). |

## See also

- [Environment variables](environment.md)
- [Policy rules](policy-rules.md)
- [Tasks and copies](../concepts/tasks-and-copies.md)
