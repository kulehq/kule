---
title: How Kule works
sidebar_label: How Kule works
description: The whole picture on one page - tasks and copies, the sandbox, the proxy, the policy, review, commit and undo.
---

# How Kule works

This page is the map. Each part has its own page with the details.

## The loop

```text
  your folder (workspace)
        │  kule new
        ▼
  task copy  ◀── agent or command, inside the SANDBOX
        │          · writes only to the copy and its tmp
        │          · network only through the task's PROXY ──▶ pass / ask you / refuse
        ▼
  effects: what changed in the copy
        │
        ▼
  POLICY verdict for each effect: pass · ask · deny
        │
        ▼
  REVIEW (you) ──▶ commit: apply to the folder, keep a checkpoint ──▶ undo
               └─▶ rollback: throw the changes away
```

1. **You make a task** on a folder. Kule makes a copy-on-write **copy** of
   it. → [Tasks and copies](tasks-and-copies.md)
2. **An agent (or any command) works in the copy**, inside the operating
   system's **sandbox**. It can write only to the copy and its own
   temporary files; your credentials and other folders are hidden. →
   [The sandbox](sandbox.md)
3. **Its network goes through the task's proxy.** Each connection is judged
   by the policy: package registries pass, the agent's model API passes
   while it runs, anything else waits for your answer (a **question**). →
   [Network and questions](network-and-questions.md)
4. **Kule finds the effects** by comparing the copy with the workspace:
   files created, modified, deleted or moved, git branches moved, hooks
   added. It does not trust what the command *says* it did; it looks at
   what changed.
5. **The policy judges each effect**: **pass** (goes in on its own),
   **ask** (waits for your approval) or **deny** (never goes in). →
   [Policy](policy.md)
6. **You review**, then **commit** (apply to the folder, keeping a
   **checkpoint**) or **roll back** (throw away). **Undo** takes a commit
   back. → [Review, commit and undo](review-commit-undo.md)

## Why compare instead of watch?

Kule decides by the **effects** a command had, not by its text. `rm -rf
build`, a Python script and an agent's file edit are all judged the same
way: by what they changed in the copy. That is why:

- a harmless-looking command that rewrites `package.json` still waits for
  you;
- a scary-looking command that only touches the copy changes nothing real;
- the same logic works on every platform, without tracing system calls.

## What is protected, and from whom

Kule assumes the agent may be wrong or manipulated (a prompt injection in a
file it reads, for example). It does not rely on the agent behaving. The
boundaries hold instead:

| Boundary | What holds it |
| --- | --- |
| Agent ↔ your folder | The copy, the policy, and your review |
| Agent ↔ your computer | The sandbox (Seatbelt on macOS; Landlock, seccomp and namespaces on Linux) |
| Agent ↔ the network | The task's proxy and the policy |
| Agent ↔ your secrets | Placeholders; the proxy fills in real values only for the secret's hosts |
| Agent ↔ your home directory | The [home layer](home-layer.md): its writes wait for review |

See [Security model](../security.md) for the full list.

## Who runs what

- **`kule`** is the command line. It runs the engine directly.
- **The desktop app** is a window onto the **Kule daemon**. The daemon owns
  the tasks, the agents' runs and the questions they wait on. Closing the
  window stops nothing; the daemon exits after ten idle minutes. See
  [`kule daemon`](../cli/daemon.md).
- **Agents** (Claude Code, Codex, Gemini, or any that speaks the Agent
  Client Protocol) are separate programs you install. Kule starts them in
  the sandbox and talks to them over ACP. See [Agents](../using-agents.md).

## Where things live

| Where | What |
| --- | --- |
| Your folder (the workspace) | Only read until you commit, sync or undo. |
| `~/.kule` (Kule's home, or `KULE_HOME`) | Tasks, their copies, settings, the policy, secrets, the signed history. No sandbox can see it. |
| `~/.kule/tasks/<id>/` | One task: its copy, temporary files, home layer, steps and checkpoints. |

See [Files and settings](../reference/files.md).

## Read on

- [Tasks and copies](tasks-and-copies.md)
- [The sandbox](sandbox.md)
- [Policy](policy.md)
- [Network and questions](network-and-questions.md)
- [Review, commit and undo](review-commit-undo.md)
- [Sync and merging](sync-and-merging.md)
- [Git repositories](git.md)
- [Home layer](home-layer.md)
- [Secrets](secrets.md)
- [Budgets and limits](budgets-and-limits.md)
- [Groups](groups.md)
