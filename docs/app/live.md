---
title: Live, the agent chat
sidebar_label: Live (agent chat)
description: Give an agent a prompt, watch its answer, tool calls and output as they come, answer its questions and stop it.
---

# Live: the agent chat

The **Agent** panel (Mod+1), also called **Live**, is where you tell an
agent what to do and watch it work. It reads like the terminal beside it:
the conversation fills the panel, and the prompt is a command line at the
bottom that grows as you write.

## Writing a prompt

1. Put the cursor on the prompt line: click it, or press **Mod+L**.
2. Write what the agent should do. **Shift+Enter** makes a new line.
3. Press **Enter** (or **Mod+Enter**), or click **Run**.

Under the prompt line:

| Control | What it does |
| --- | --- |
| **Agent picker** | Chooses the agent that takes the prompt: Claude Code, Codex, Gemini or one you added (see [Agents](../using-agents.md)). |
| **waits** | How long the agent waits for your answer when it asks to connect somewhere the policy puts to you: 1, 5, 15 (the default) or 30 minutes, or 1, 2, 4 or 8 hours. Unanswered, the connection is refused and the agent goes on without it. |
| **Measure bar** | While a run goes on: what it takes of the computer, against the task's limits. See [below](#the-measure-bar). |

Places you picked in the Preview to ask about show as **chips** above the
prompt line; see [Files and Preview](files-and-preview.md).

One agent works in a task at a time. To run agents side by side, make more
tasks, on the same folder or others. You can run the agent again before you
commit: changes pile up in the copy until you decide.

## What Live shows

As they come:

- **Your prompt**, in a block of its own under *You*, and the agent's answer
  under its name. Each message has a **copy button** (it shows when the
  pointer is over the message) that copies it as it was written, Markdown and
  all.
- **The agent's answer**, laid out as Markdown, with code blocks coloured as
  in an editor.
- **Its thinking**, in a quieter style, when the agent shares it.
- **Tool calls**, one line each: `·` while running, `✓` done, `✗` failed, and
  after the arrow the files the call changed. Every call that changes
  something is a **step** you can look at and go back to (see
  [Timeline](timeline.md)).
- **What each call printed**, such as a command's output, under its line:
  - it streams while the command runs;
  - it is open while the call runs or when it failed, and closed once it
    went well (click the line to open or close it);
  - terminal colours and cursor moves are taken out, and the last 64 KB of
    each call's output are kept.
- **Notes**: the agent's plan, permissions it was given, a server Kule
  stopped, pictures an agent could not take, what Kule ended in the task's
  sandbox (see [Processes and servers](../concepts/sandbox.md#processes-and-servers)),
  and what Kule could not do:
  - *Kule could not read the extensions' MCP servers (…); this run goes
    without them*;
  - *Kule could not write the agent's log (…); what the agent writes to it
    is not kept for the rest of this run*. The agent goes on.
- **Question cards** when the agent's program wants to connect somewhere the
  policy does not let through on its own (see [below](#questions)).
- **The end of the run**: *Done* or *Stopped: …* with the reason, each with
  the number of operations the sandbox refused, or *The run failed: …*. A stop you
  asked for shows as a stop, not as a failure. Under the line, *What the
  sandbox refused* opens the list of them: the same refusal once, with how
  many times (`×3`), the newest 20, then *… and N more refusals in the
  Timeline*. On Linux, the line says that Linux does not tell Kule what the
  sandbox refused, and each refusal reached the command as an error. Where Kule
  knows why a refusal keeps a tool from working, it says so under that line.
  For example, a program that wants the screen (LibreOffice, even with
  `--headless`) cannot run in a task, and macOS may report that it quit
  unexpectedly. The agent is told the same at its next prompt, in Kule's
  words, once.

The daemon keeps a task's latest events, so a window opened later shows what
happened meanwhile. When older ones were let go, that window starts with
*N earlier events of this task's runs are no longer kept (Kule keeps the
latest 2000, up to 32 MiB)*.

:::tip Copy on select

Text you select with the mouse in Live goes to the clipboard at once, and a
small *Copied* shows by the pointer. Turn it off in *Settings → Appearance →
Copying*.

:::

## Questions

When the agent's program connects to a host the [policy](../concepts/policy.md)
does not let through on its own, the connection **waits** and a card shows
in Live. The task also moves under *Needs you* in the Tasks panel, and, while
the window is not in front, the system's notifications tell you.

The card names the host and port, the rule that made it wait, and a
countdown to when it is refused. It also says what approving lets through:
a tunnel Kule does not look into (it checks only that the TLS hello asks
for this host, and cuts it if not), or a connection Kule looks inside, with
the secrets it fills there or that a registry stays read only.

Once answered, the card becomes a line in the run: "You approved
api.example:443". If Kule then cuts the connection, the line says so with
why, while the run goes on, and a notification tells you. A question Kule
refused without your answer (nobody answered in time, the run ended, too
many were waiting) is a line of its own that says it was Kule.

| Button | Keys | What it does |
| --- | --- | --- |
| **Approve** | Mod+Shift+Y | Lets this one connection through. For a moment after the card appears (0.6 s), Approve takes no click or key, so a card that shows up under your pointer is not approved by accident. |
| **Approve and remember** | none by default | Lets this host and port through on this folder from now on, without asking. *Settings → Rules* lists what you told Kule to remember, and forgets a rule. |
| **Deny** | Mod+Shift+N | Refuses it. The agent goes on without it. |

How long a question waits is the **waits** setting under the prompt (15
minutes unless you change it, from 1 minute to 8 hours; never off). See
[Network and questions](../concepts/network-and-questions.md).

## The measure bar

While a run goes on, the bar under the prompt shows what it takes of the
computer, updated every two seconds, against the task's
[limits](../concepts/budgets-and-limits.md). For example:

```
CPU 140% / 200% · 1.2 GB / 4 GB · disk +350 MB · 7 processes
```

- It turns **amber** near a limit.
- It adds **slowed** while the processor limit holds the run back.
- **Click it** to change the task's limits.

When a limit stops a run, Live says which one and what it measured, for
example "Stopped: the run's processes held 4.2 GB, over its memory limit of
4 GB". If that task is not the one on screen, a notice says so.

## Stop

While a run goes on, **Stop** (Mod+Shift+K) takes the place of Run. It
cancels the agent's turn, as you would in the agent itself. The button reads
*Stopping…* until the run ends; an agent that does not end its turn within
ten seconds is killed. Whatever it changed so far stays in the copy for you
to review.

## After the run

When Live says the run is done, open **Review** (Mod+2) to see what changed
and commit or roll back. See [Review](review.md).

## From a terminal

[`kule run`](../cli/run.md) gives an agent a prompt from a terminal, held to
the same budgets and limits. [`kule steps`](../cli/steps.md) lists what each
tool call changed. A terminal has nobody to ask, so a connection the policy
puts to a person is refused at once there.
