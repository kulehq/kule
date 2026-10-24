---
title: kule run
sidebar_label: kule run
description: Give an AI agent a prompt; it works in the task's copy, inside the sandbox, and every tool call becomes a step.
---

# kule run

Give an agent a prompt. It works in the task's copy, inside the sandbox,
with its network through the task's proxy. Nothing reaches the workspace
until you commit.

## Synopsis

```sh
kule run [OPTIONS] <TASK> <PROMPT>
```

## Description

`kule run` starts an agent such as Claude Code, Codex or Gemini, hands it
your prompt, and prints what it does until its turn ends. Kule talks to the
agent over the **Agent Client Protocol (ACP)**, the same way an editor would.

What the agent can do:

- **Files:** it reads and writes the task's copy. It cannot write your real
  folder, and it cannot read your credentials (`~/.ssh`, `~/.aws`, …), your
  own folders (Documents, Desktop, …) or other agents' logins. See
  [The sandbox](../concepts/sandbox.md).
- **Network:** every connection goes through the task's proxy. Package
  registries and the agent's own model API pass; anything else is judged by
  the [policy](../concepts/policy.md). The CLI has nobody to ask, so a
  connection the policy would put to a person is **refused at once**. (In
  the app it waits for your answer.) See
  [Network and questions](../concepts/network-and-questions.md).
- **Your home:** what it writes in `~/.zshrc`, `~/.config` and the like is
  caught in the task's home layer for you to review with
  [kule home](home.md), never written to your home on its own.
- **Secrets:** it sees placeholders, never your API keys. See
  [Secrets](../concepts/secrets.md).

Every tool call the agent makes that changes files becomes a **step**
(see [kule steps](steps.md)). When the run ends, Kule prints what changed in
the copy and what the policy says about it, so you can go straight to
[kule commit](commit.md) or [kule rollback](rollback.md). You can also run
the agent again first: changes pile up in the copy until you decide.

**Budgets and limits hold.** A run does not start once one of the task's
budgets is spent (its own, its group's, or the daily one for every task), or
once the task has used up its time. While it runs, the
[limits](../concepts/budgets-and-limits.md) on processor, memory, disk, time
and idling hold it, as they do in the app. They count every process the
run started, also one that left the agent's process tree.

**The agent keeps to what Kule can hold.** A message from the agent longer
than 16 MiB, or more than 1024 tool calls left open at once, stops the run.
A tool call's title is cut to 200 characters. Where Kule runs the agent's
commands for it (in the app), it runs 32 at once at most.

**Nothing it started outlives the run.** When the run ends, however it ends,
whatever the agent left running in its sandbox ends too, whatever became of
its parent. Kule names what it ended in a note:
`· Kule ended <names>, which the agent left running in its sandbox, as the run ended`.

**A run that fails or is stopped is told in full.** Kule prints what it
changed, its connections, its home writes, what the sandbox refused, the
paths Kule put back, the policy's ruling, what was spent and how long it
took, as after a run that went well, and only then the error. On Linux a
run that failed also prints
`kule: Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error`.
If Kule could not record what the run spent, put back what it changed, or
read or record what the sandbox refused, it says so in a `kule:` line each,
and `kule run` fails.

**The agent hears what changed.** If [kule sync](sync.md) brought the folder's
changes into the copy since the agent's last run, or the sandbox refused it
something Kule can explain, a short note in Kule's words goes before your
prompt, once.

One agent works in a task at a time. To run agents side by side, make more
tasks (on the same folder or others).

:::note

The agent must be installed and signed in the way it is without Kule:
Claude Code and Codex need Node.js (`npx`), Gemini its CLI. `kule agents`
lists which agents can start here and why the others cannot. See
[Agents](../using-agents.md).

:::

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |
| `PROMPT` | yes | What the agent should do. Quote it: one argument. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--agent` | name | `claude-code` | Which agent takes the prompt: `claude-code`, `codex`, `gemini`, or the name of an agent profile you added and approved (see [kule agents](kule-agents.md)). |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Ask Claude Code to fix a test:**

```sh
kule run 3422f830 "The test in tests/greet.rs fails. Fix it and run cargo test."
```

**Use Codex instead:**

```sh
kule run 3422f830 --agent codex "Add a --verbose flag to the CLI."
```

**Work on documents, not code:**

```sh
kule run 40ac70b8 "Sort the notes in this folder into one folder per year. Keep the file names."
```

**Run, check the ruling, and commit only if everything passes on its own:**

```sh
kule run "$TASK" "Update the README's install section" && kule commit "$TASK"
```

`kule commit` exits with `3` and commits nothing if anything waits for your
approval, so this is safe.

## Output

While the agent works, a busy line says what it is doing. A run's output
looks like this (the agent's words and tools vary):

```text
I'll look at the failing test first.

  ✓ Read tests/greet.rs → step 1: no file changes
  ✓ Edit src/main.rs → step 2: src/main.rs
  ✓ cargo test → step 3: target/debug/app and 41 more
plan: Read the test / Fix greet / Run the tests

The greeting now matches what the test expects, and cargo test passes.

1 file changes: +0 ~1 -0
  ~ src/main.rs
      ~ function greet
net: index.crates.io:443 · tunneled (package-registries)
policy: passes on its own
spent: 0.12 USD, 18234 tokens in 1 run
took: 41s
```

| Line | Meaning |
| --- | --- |
| plain text | The agent's answer, as it writes it. |
| `✓ <tool> → step N: <files>` | A tool call that went well, the step it became and the files it changed (the first five; `kule steps` has the rest). `✗` means it failed. |
| `plan:` | The agent's plan, when it shares one. |
| `· <note>` | A note from Kule or the agent. |
| `· Kule allowed "<call>" (the agent's option: "<label>")` | The agent asked for permission and what Kule did, from the kind of option it chose; the agent's own label is only quoted after. `Kule did not allow …` when the option refuses (on a terminal, only those show, marked `✗`). |
| `N file changes: +a ~m -d` | What the copy now holds that the workspace does not: created, modified, deleted. Inside each file, the functions, sections or keys that changed. See [kule status](status.md). |
| `git:` | What happened to the repository: branch moves, new objects, config or hook changes. See [Git](../concepts/git.md). |
| `net:` | Each connection the run made and what became of it: `tunneled` when it went through; `cut` when Kule ended it or refused a request in it; `failed` when it ended in an error of its own (the host went away, a reset); `refused, …` when the policy did not let it through. `network.jsonl` says why for `cut` and `failed`. |
| `in your home, caught for review` | Writes in your home folder, waiting in the home layer. See [kule home](home.md). |
| `sandbox: refused …` | What the sandbox refused (macOS lists them; on Linux the command just gets an error). |
| `sandbox: <path> changed during the run; Kule put it back …` | A path the run may not change, which Kule put back as it was. |
| `policy:` | The ruling on the changes: *passes on its own*, *waits for approval* or *denied*, with the rule behind each. |
| `spent:` | What the task's runs have cost so far, as the agents reported it, and what is left of its budget. ` (N records cannot be read)` when some of the task's usage records cannot be read. ` (<amount> set aside for runs at work or that did not end)` when runs hold a share that their usage has not closed yet; it counts in what is left. |
| `took:` | How long the run took. |

When the output is not a terminal, every event is a plain line of its own,
which is easier to log.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: no agent <name>; known: claude-code, gemini, codex` | Check the name. Add your own agents with [kule agents](kule-agents.md). |
| `kule: agent <name> cannot run: <why>` | The agent is missing, not signed in, or (for your own profiles) not approved yet. Run `kule agents` to see why. |
| `kule: task <id> has spent its budget; raise it or turn it off to run again` | Raise it with `kule budget TASK <amount>`, or `kule budget TASK off`. The message names a group's or the daily budget when that is the one spent. |
| `kule: Kule cannot tell what <budget> has left: <N> record(s) of what runs spent … cannot be read; mend or remove them, or turn the budget off, to run again` | A usage record that cannot be read counts, so the budget holds every run until it is mended or removed (a task's `usage.jsonl`), or the budget is turned off. See [Budgets and limits](../concepts/budgets-and-limits.md). |
| ``kule: what is left of <budget> (<amount>) is set aside for runs at work, or that did not end, in task <id>: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end`` | Runs that started before it (in the tasks named) hold what is left. Wait for them to end. A run that did not end (Kule was stopped) keeps its share counted until you let go of it with [kule budget](budget.md) `TASK --release`. |
| `kule: Kule could not set aside what the run may spend (…), so it does not start` | Kule could not write the task's `reserved.jsonl`. Nothing started. Check that Kule's home can be written. |
| `kule: the task has used its time: its runs took N minutes together; …` | Raise or remove the task's time limit with [kule limits](limits.md). |
| `kule: an agent is at work in task <id> …` | One agent per task. Wait, or make another task. |
| `kule: a terminal is open on this task's copy (in Kule's app, or kule shell); close it first, then start work` | A terminal on the task (the app's, or `kule shell`) is open. Close it first. |
| `kule: Kule opened task <id>'s copy in <app> outside the sandbox; …` | Close that terminal app or editor, then run [kule closed](closed.md) `TASK`. |
| `kule: <error> (see <home>/tasks/<id>/agent.log)` | The agent failed. It comes after what the run did. The agent's own messages are in `agent.log`: its last 8 MB. When the log was turned over during the run, the line ends `, and agent.log.1 beside it for what came before`: the 8 MB before them. |
| `kule: stopped: <why>` | A limit or the agent's own messages stopped the run, as the rest of the line says; it names no log. |
| `kule: stopped: the agent sent a message larger than 16 MiB` | The agent said more in one message than Kule takes. Run it again with a narrower prompt. |
| `kule: stopped: the agent left more than 1024 tool calls open` | The agent started tool calls without ending them. |
| `kule: Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error` | On Linux, after a run that failed: no `sandbox: refused` line does not mean nothing was refused: a refused operation reached the agent's command as a permission error. |
| `kule: what the run spent could not be recorded (…), so the <amount> set aside for it stays counted` | Kule could not write the task's `usage.jsonl`. The run's whole share counts in place of what it spent, until you let go of it with [kule budget](budget.md) `TASK --release`. Check that Kule's home can be written. Without a budget the line ends at `(…)`. |
| `kule: what the run spent could not be read (…), so the <amount> set aside for it stays counted` | Kule could not read what the run's agent reported. The run's whole share counts, as above. |
| `kule: Kule could not put back the protected paths the run changed: …` | Look at the changes with [kule status](status.md) before you commit, or [roll them back](rollback.md). |
| `kule: what the sandbox refused cannot be read: …` / `… could not be recorded: …` | The refusals are not listed, or not kept for the agent's next run and the app's Timeline. |
| `kule: what the run changed cannot be shown: …` / `the connections the run made …` / `what the run wrote in your home …` / `the policy …` / `what the run spent …` | Kule could not read that part back (a folder in the copy that cannot be read, say). The rest is still told, the refusals among it, and the run's own error, if any, comes after. |
| `kule: Kule could not read the extensions' MCP servers (…); this run goes without them` | What you approved in [kule ext mcp](ext.md) cannot be read, so the agent gets no MCP server from extensions for this run. Check that Kule's home can be read, then approve them again. |
| `kule: the run ended, but Kule could not do all of the above` | The run itself went well; one of the lines above says what Kule could not do. |
| `· Kule could not write the agent's log (…); what the agent writes to it is not kept for the rest of this run` | A note, not an error: the agent goes on, but its messages from then on are not in `agent.log`. Check that Kule's home can be written. |
| `kule: <agent> reported no cost, so the task's budget does not count this run` | A warning, not an error: agents signed in with a subscription may report tokens only, and a budget cannot hold what is not reported. |

On some Linux systems no agent runs until you accept weaker isolation; see
[kule isolation](isolation.md).

## In the app

Write the prompt in the **Agent** panel (Mod+L puts the cursor there), pick
the agent under it and press Enter or **Run**. The app also shows each tool
call's output as it streams, lets you answer the agent's questions, and has
**Stop** (Mod+Shift+K). See [Live](../app/live.md).

## See also

- [kule exec](exec.md): run one command in the sandbox instead of an agent.
- [kule steps](steps.md), [kule status](status.md), [kule commit](commit.md)
- [Agents](../using-agents.md), [Budgets and limits](../concepts/budgets-and-limits.md)
