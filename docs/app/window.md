---
title: The window
sidebar_label: The window
description: The parts of Kule's window, where each panel lives, and how to move, hide and reset them.
---

# The window

Kule's desktop app is a window onto the Kule daemon, the background process
that owns your tasks and the agents' runs (see
[When the window is closed](#when-the-window-is-closed)). This page names
every part of the window so the other pages can refer to them.

**Mod** on these pages means **⌘** on macOS and **Ctrl** on Linux and
Windows.

## First start

With no task yet, the window opens on a first-run screen titled *A control
room for your agents*, with three steps:

1. **Your agents.** Kule checks which agents can start on this computer
   (Claude Code, Codex, Gemini and any you added) and, for those that cannot,
   says what to install. Claude Code and Codex need Node.js for `npx`;
   Gemini needs its own CLI. Sign each agent in once, in a terminal, the way
   you would without Kule: Kule's runs use that sign-in. See
   [Agents](../using-agents.md).
2. **How careful.** Pick the policy for every task:
   - **Balanced**: changes to ordinary files pass on their own. What decides
     what gets installed, built or run (manifests, lockfiles, CI files,
     editor settings), git history rewrites and mass deletes wait for you.
   - **Careful**: every change to a file or to git waits for you. Only
     package registries, and each agent's own API while it runs, are reached
     without asking.

   You can change it later under *Settings → Policy*. See
   [Policy](../concepts/policy.md).
3. **A folder.** *Choose a folder…* makes your first
   [task](../concepts/tasks-and-copies.md) on it.

Opened from the Finder or a launcher, the app reads `PATH` from your login
shell, so agents find `npx`, `node` and the rest as they do in a terminal.

## Regions and panels

The window has four **regions**: left, main, right and bottom. Each region
holds **panels** as tabs.

| Panel | What it shows | Default region |
| --- | --- | --- |
| **Tasks** | Your tasks and groups. *Needs you* at the top collects the tasks waiting for an answer or an approval. | Left |
| **Files** | The task's copy as a tree, changed files marked. See [Files and Preview](files-and-preview.md). | Left |
| **Review** | What the task changed and what the policy says about it, with Commit and Roll back. See [Review](review.md). | Main |
| **Timeline** | Commits, steps, network connections and sandbox refusals. See [Timeline](timeline.md). | Main |
| **Preview** | One file of the copy, shown large. See [Files and Preview](files-and-preview.md). | Main |
| **Agent** (Live) | The conversation with the agent and the prompt line. See [Live](live.md). | Bottom |
| **Terminal** | Your own shells in the copy, one tab each. See [Terminals and servers](terminals-and-servers.md). | Bottom |

The right region is empty and hidden by default. By default the agent chat
and the terminals share the bottom, side by side:
`Agent | zsh 1 | zsh 2 | +`.

### Moving and resizing

- **Move a panel**: drag its tab to another region, or use the tab's menu,
  or the keyboard.
- **Resize a region**: drag its edge. A side can grow until the middle keeps
  at least 360 × 160 pixels, so the main area never disappears between them.
- **Show or hide a region**: use the button at the end of its tab bar, the
  four switches at the right of the title bar, or the keys below. The bottom
  panel can fill the window. The main area and the bottom panel are never
  hidden together.
- **Reset the layout**: run *Reset the layout* from the command palette
  (Mod+K) to go back to Kule's own.

| Show or hide | Keys |
| --- | --- |
| Left side | Mod+B |
| Bottom panel | Mod+J |
| Right side | Mod+Alt+B |
| Agent chat | Mod+Shift+J |
| Terminal | Ctrl+\` |
| Main area | no default; give it one in *Settings → Shortcuts* |

The layout is kept across restarts in `layout.json` in Kule's home (see
[Files and settings](../reference/files.md)).

## Title bar

At the top of the window:

- the window's title;
- **Weaker isolation**, an amber chip, while you have accepted weaker
  isolation on a Linux computer that cannot give agents all of theirs (see
  [`kule isolation`](../cli/isolation.md));
- four **layout switches** for the left side, the main area, the bottom
  panel and the right side. On macOS these sit in the window's own title
  bar, as in VS Code.

The narrow bar on the far left holds Kule's mark and, at the bottom, the
**Settings** gear (Mod+,).

## Task header

Above the main area, the open task's header shows:

- the task's name and its folder;
- **server chips** such as `localhost:5173 ↗` for servers working in its
  copy (see [Terminals and servers](terminals-and-servers.md));
- its **state**, such as "agent at work" or "3 changes · needs approval";
- what its runs have **spent** so far, as the agents reported it;
- a **builds from** chip when the folder builds from a project beside it
  that its tasks cannot read yet (see [`kule reveal`](../cli/reveal.md)).

## The Tasks panel

- **New task…** (Mod+N) picks a folder and makes a task on it.
- **Needs you** lists tasks that wait for you: a question about a
  connection, or changes that need approval.
- **Tasks** lists every task and group. The **+** next to the title makes a
  [group](../concepts/groups.md).
- Each task has a **⋮ menu** (Mod+., or a right click): *Rename…*, a
  colour, *Group…*, *Budget…*, *Limits…*, *Open terminal*, *Open the copy in
  an editor*, *Bring in the folder's changes*, *Fork task*, *Move up*/*Move
  down* and *Delete task*. Items that would disturb a running agent say
  *(agent at work)* and wait.
- A group's ⋮ menu has *Rename…*, a colour, *Budget…*, *Limits…*, *Move
  up*/*Move down* and *Delete group* (its tasks stay, in no group).
- Drag a task to reorder it, or onto a group to put it there; drag it onto
  *Not in a group* to take it out. Alt+Shift+↑/↓ moves the current task
  (Alt+↑/↓ with the list focused). `kule list` keeps the same order.

## Command palette

**Mod+K** opens the command palette. Every action of the window is there by
name, with its shortcut, including the ones that have no default key. Type
part of a name to find it. **Mod+/** lists the shortcuts. See
[Keyboard](keyboard.md).

## Menu bar

File, Edit, View, Task and Help (with Kule and Window on macOS), at the top
of the screen on macOS and at the top of the window on Linux and Windows.
Every item runs the same command as its shortcut and shows the keys you set.
Shortcuts that text editing owns too (⌘⌫, ⌥⇧↑) stay in the window, so they
keep working in a text field.

## When the window is closed

The window is a view onto the **Kule daemon**, which the app starts when
none is running. The daemon owns the tasks, the agents' runs and the
questions they wait on:

- Closing the window stops nothing. A run goes on, and its questions wait
  for their time.
- Reopening the window shows what happened meanwhile.
- The daemon exits after ten minutes with nothing going on.

Kule keeps everything in its home, `~/.kule` (or `KULE_HOME`): tasks and
their copies, settings, the policy and secrets. No sandbox can see it. See
[`kule daemon`](../cli/daemon.md) and [Files and settings](../reference/files.md).

## From a terminal

The `kule` command works on the same tasks as the window. See the
[CLI reference](../cli/index.md), and [`kule list`](../cli/list.md),
[`kule new`](../cli/new.md), [`kule name`](../cli/name.md) and
[`kule color`](../cli/color.md) for what the Tasks panel does.
