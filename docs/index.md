---
slug: /
title: What is Kule?
sidebar_label: Introduction
description: Kule lets AI agents work on a copy of your folder, in a sandbox, and nothing reaches the real folder until you review and commit it.
---

# What is Kule?

Kule is a **control room for AI agents** that work on your files. The files
can be code, but also notes, documents, photos or data. The agents do the
work; you watch, approve and steer.

The idea is simple: **an agent never touches your real folder.** It works on
a copy, inside a sandbox, and you decide what comes back.

```text
 your folder ──copy──▶ task copy ──agent works here, in a sandbox──▶ changes
      ▲                                                                 │
      └──────────── commit (keeps a checkpoint, undo puts it back) ◀────┘
                                   review
```

:::note Kule is a preview

Kule runs on **macOS and Linux**. Windows is planned. The latest
pre-release is
[`0.2.0-preview.5`](https://github.com/kulehq/kule/releases/tag/v0.2.0-preview.5):
[download it or build Kule from source](getting-started/install.md).

:::

## Why use it?

Coding agents like Claude Code, Codex and Gemini run commands on your
computer. Normally that means they can change any file you can, read your
credentials, and reach any host on the network. You either watch every
command, or you trust the agent.

Kule gives you a third option:

- **The agent works on a copy.** On macOS (APFS) and on Linux with Btrfs or
  XFS, the copy takes almost no disk space.
- **The agent runs in a sandbox.** It can write only to its copy and its own
  temporary files. Your `~/.ssh`, `~/.aws`, Documents, Desktop and other
  projects are hidden from it.
- **Its network goes through a proxy.** Package registries and the agent's
  own model API pass. Anything else waits for your answer, and every
  connection is logged.
- **A policy judges what changed.** Ordinary file edits pass on their own.
  Changes that decide what gets installed, built or run (`package.json`,
  lockfiles, CI files, git hooks…) wait for your approval.
- **Nothing reaches your folder until you commit.** A commit keeps a
  checkpoint, so `undo` puts the folder back.
- **Secrets stay secret.** The agent sees placeholders; Kule's proxy puts
  the real API key into requests to the right host only.

## Two ways to use Kule

| | What it is | Start here |
| --- | --- | --- |
| **The desktop app** | A window with your tasks, the agent's chat, Review and Timeline. Best for daily work. | [Your first task in the app](getting-started/first-task-in-the-app.md) |
| **The `kule` command** | The same engine from a terminal: make tasks, run commands or agents in the sandbox, review and commit. | [Quick start](getting-started/quick-start.md) |

Both work on the same tasks, so you can mix them.

## The words Kule uses

You will meet these terms everywhere in the docs:

| Term | Meaning |
| --- | --- |
| **Workspace** | Your real folder, the one you give Kule. |
| **Task** | One piece of work on a workspace. It has an 8-character id like `1a2b3c4d`. |
| **Copy** | The task's copy-on-write copy of the workspace. The agent works only here. |
| **Effect** | One thing a command changed: a file created, modified or deleted, a git branch moved, a hook added. |
| **Policy** | The rules that judge each effect. A rule's verdict is **pass** (goes in on its own), **ask** (waits for your approval) or **deny** (never goes in). |
| **Commit** | Applying the task's changes to the workspace. |
| **Checkpoint** | What a commit replaced, kept so `undo` can put it back. |
| **Step** | One tool call of an agent that changed something. You can go back to any step. |
| **Question** | A network connection the policy puts to you, waiting for Approve or Deny. |
| **Home layer** | Where the task's writes to your home directory (`~/.zshrc`, `~/.config`) land, for you to review. |
| **Placeholder** | What the agent sees instead of a secret's real value. |

[How Kule works](concepts/how-kule-works.md) explains all of them in one
page.

## Where to go next

- [Install Kule](getting-started/install.md)
- [Quick start: five minutes in a terminal](getting-started/quick-start.md)
- [Your first task in the app](getting-started/first-task-in-the-app.md)
- [Watch the introduction video](intro-video.mdx)
- [Every `kule` command](cli/index.md)
- [Security model](security.md)
