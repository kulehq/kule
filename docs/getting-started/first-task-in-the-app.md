---
title: Your first task in the app
sidebar_label: First task (app)
description: A step-by-step walk through the desktop app, from the first-run screen to a prompt, a question, Review, Commit and Undo.
---

# Your first task in the app

This page walks through one task in the desktop app, from the first start
to a commit and an undo. It takes about ten minutes, most of it the agent's
work.

You need the app ([Install](install.md)) and at least one agent signed in.

**Mod** means ⌘ on macOS and Ctrl on Linux.

## 1. The first-run screen

With no task yet, the window opens on **A control room for your agents**
and three steps:

1. **Your agents.** Kule checks which agents can start on this computer.
   For one that cannot, it says what to install: Claude Code and Codex need
   Node.js (`npx`), Gemini its own CLI. Sign each one in once, in a
   terminal, as you would without Kule.
2. **How careful.** Pick a policy for every task:
   - **Balanced**: source, docs and data pass on their own, and so do
     kinds of file your project already tracks in git. Other kinds (scripts
     among them), hidden files and folders, what decides what gets
     installed, built or run (manifests, lockfiles, CI, editor settings),
     git history rewrites and mass deletes wait for you.
   - **Careful**: every change to a file or to git waits for you. Only
     package registries and the agents' own APIs are reached without
     asking.

   You can change it later in *Settings → Policy*.
3. **A folder.** Click **Choose a folder…** and pick the folder the first
   task works on. A small test folder is a good start.

:::tip Agents not found?

Opened from the Finder or a launcher, the app reads `PATH` from your login
shell, so agents find `npx` and `node` as in a terminal. If an agent still
shows as missing, see [Troubleshooting](../troubleshooting.md).

:::

## 2. The task

Kule makes a **task**: a copy-on-write copy of the folder. The window now
shows:

- **Tasks** on the left: your task, named after its folder;
- the **task header** at the top: its name, its folder and its state;
- **Review**, **Timeline** and **Preview** in the main area;
- the **Agent** panel at the bottom, with the prompt line.

[The window](../app/window.md) describes every region and panel.

## 3. Write a prompt

Click the prompt line at the bottom of the Agent panel (or press **Mod+L**)
and write what the agent should do, for example:

```text
Read notes.md and add a "Later" section with three ideas.
```

Below the prompt:

- the **agent** picker chooses who takes the prompt;
- **waits** sets how long a question waits for your answer (1 minute to
  8 hours, 15 minutes by default).

Press **Enter** (or click **Run**) to start. Shift+Enter makes a new line.

## 4. Watch it work: Live

The Agent panel shows the run as it happens:

- your prompt under **You**, the agent's answer under its name;
- each **tool call** on one line: `·` while running, `✓` done, `✗` failed,
  and after the arrow the files it changed;
- what each command printed, streaming under its line;
- under the prompt, a bar with what the run takes of the computer
  (`CPU 140% / 200% · 1.2 GB / 4 GB · …`).

**Stop** (Mod+Shift+K) stops the run at any time. Whatever it changed so
far stays in the copy for you to review.

See [Live](../app/live.md).

## 5. Answer a question

If the agent's program connects to a host the policy does not let through
on its own, the connection **waits**. A card shows in Live, and the task
moves under **Needs you** in the task list. The card names the host, the
port, the rule that made it wait, and a countdown.

- **Approve** (Mod+Shift+Y): let this connection through.
- **Approve and remember**: let this host through on this folder from now
  on.
- **Deny** (Mod+Shift+N): refuse it. The agent goes on without it.

Unanswered, the connection is refused when the countdown ends. See
[Network and questions](../concepts/network-and-questions.md).

## 6. Review

When the run is done, Live says **Done**. Open **Review** (Mod+2):

- **the ruling** at the top: everything passes, something waits for you,
  or something is denied, and which rule said so;
- the **files** that were created, modified, deleted or moved, and inside
  each file what changed by name: a function, a Markdown section, a JSON
  key. A changed picture shows before and after;
- **ignored paths** (what `.gitignore` excludes) that stay in the copy
  unless you tick *Also commit these*;
- **writes in your home**, if the agent changed `~/.zshrc` or similar.
  Those are reviewed apart from the commit. See
  [Home layer](../concepts/home-layer.md).

Click a file in **Files** to open it in **Preview** (Mod+4), as it is now
in the copy.

## 7. Commit, or roll back

- **Commit** (Mod+Shift+Enter) applies the changes to your folder and keeps
  a checkpoint. When something waits for you, the button reads **Approve
  and commit**. Denied changes are held back even when you approve.
- **Roll back** (Mod+Shift+Backspace, press twice) throws the changes away.

You do not have to decide after one run. Run the agent again: changes pile
up in the copy until you commit or roll back.

## 8. Undo

Open **Timeline** (Mod+3). The newest commit has **Undo** (Mod+Shift+U,
press twice). It takes the commit back out of your folder.

Timeline also lists every **step** (each tool call that changed
something). **Go back here** puts the copy back as it was after a step, and
**Fork from here** starts a new task from that point. See
[Timeline](../app/timeline.md).

## You are done

```text
make a task → write a prompt → watch → answer questions → review → commit or roll back → undo if needed
```

Closing the window stops nothing: the daemon keeps the run going, and
reopening the window shows what happened meanwhile.

Next:

- [How Kule works](../concepts/how-kule-works.md)
- [Settings, section by section](../app/settings.md)
- [Keyboard shortcuts](../app/keyboard.md)
