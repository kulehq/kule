---
title: Settings, section by section
sidebar_label: Settings
description: Every section of Kule's Settings, from agents and policy to themes, shortcuts, extensions and profiles, and what each control does.
---

# Settings, section by section

Open Settings with **Mod+,** or the gear at the bottom left of the window.
The sections are tabs along the top; on a narrow window the tab line
scrolls sideways.

| Section | What it is for |
| --- | --- |
| [Agents](#agents) | The agents Kule can run, and yours to add and approve. |
| [Policy](#policy) | Balanced or careful for every task, a policy per group, and isolation on this computer. |
| [Rules](#rules) | What you told Kule not to ask about again. |
| [Secrets](#secrets) | API keys a task's commands use without seeing them. |
| [Budget](#budget) | What agent runs may cost: every task's, a group's, one task's. |
| [Limits](#limits) | What a run may take of this computer. |
| [Appearance](#appearance) | Themes, motion, fonts and copying. |
| [Shortcuts](#shortcuts) | Every command and its keys. |
| [Extensions](#extensions) | Install, approve, turn on or off, remove. |
| [Profile](#profile) | Your choices in one file, to take to another computer. |

## Agents

Kule's own agents (Claude Code, Codex, Gemini) and yours from
`~/.kule/agents.toml`. See [Agents](../using-agents.md) for what each needs.

- Each agent is a card that opens and closes. A closed card shows its
  chips, the number of notes and its command. An agent that is held (not
  approved, or failing a check) or missing (its program is not installed)
  starts open, so you see why. **Open all** / **Close all** do every card.
- **Add from the ACP registry** lists the agents of the
  [ACP registry](https://agentclientprotocol.com/registry) and takes a draft
  profile from one; **Add by hand** opens an empty profile to fill in.
- **Try it** runs a draft once in an empty folder, opened no wider than the
  draft, and lists what the sandbox refused it as checkboxes: the
  directories it wanted to write to and the hosts it wanted to reach. Tick
  what it should get (a telemetry host can stay out). Linux does not report
  refused files, so there only hosts are suggested.
- An agent you add runs only once you **approve** what it opens (its own
  folders, hosts and keys), and again after any change to it.

## Policy

What passes on its own, what waits for you and what never passes. See
[Policy](../concepts/policy.md).

- **For every task**: **Balanced** (the default) or **Careful**. A policy of
  your own in `~/.kule/policy.cedar` replaces them; Kule never overwrites it.
- **Groups**: each [group](../concepts/groups.md) is judged by *Every task's*
  policy (the default), Kule's **Balanced** or **Careful** one whatever holds
  for the rest, or **Its own…**, a policy you write for the group, checked
  before it is kept. A repository's refusals (in its `.kule/policy.cedar`)
  and what you told Kule not to ask about again still hold. A change counts
  from the task's next commit and run.
- **Isolation on this computer**: where the computer cannot give agents all
  of their isolation (Linux without unprivileged user namespaces), Policy
  lists what is missing, and no agent runs until you tick **Run agents with
  what this computer gives**. The title bar then says *Weaker isolation*
  while it holds. See [Platforms](../platforms.md).

## Rules

What you told Kule not to ask about again: a connection's *Approve and
remember*, or *Don't ask again in this folder* on a held file in Review
(*Don't ask again about files of kind X in this folder* for a kind), which
first shows what it would have let through. Each rule has **Forget**
(click again to confirm), after which Kule asks again. An answer an earlier
version kept for what runs code on its own (`automation`) is marked **No
longer applies**: such files are asked about every time. Forget it to clear
it away.

A denied change is never here, nor anything under `.kule/`, where a
repository keeps its own rules.

## Secrets

API keys a task's commands use without seeing them. See
[Secrets](../concepts/secrets.md).

To keep one, fill in:

| Field | What to put |
| --- | --- |
| **Name** | The variable commands find it in, like `STRIPE_KEY`. |
| **Goes to** | The hosts it may go to, separated by spaces or commas, like `api.stripe.com`. |
| **Value** | The secret. Kept in Kule's home, readable by you alone; never shown again. |
| **For** | One folder, or every folder. |

Then **Keep secret**. A command in a task finds a placeholder in `$STRIPE_KEY`;
the task's proxy puts the real value into requests to the secret's hosts,
over HTTPS, and nowhere else. Values in a folder's `.env` files that look
like secrets are kept out of the copy the same way.

The section says where the values are sealed: with a key your system's
keychain (macOS), Secret Service (Linux) or the Credential Manager
(Windows) keeps, or, where the system keeps none or with a Kule home other
than the default, with a key kept in a file in Kule's home. Each secret has **Forget** (click again to confirm).

## Budget

What agent runs may cost, as their agents report it. See
[Budgets and limits](../concepts/budgets-and-limits.md).

**Applies to** picks one scope at a time: **Every task**, **One group** or
**One task** (then which one). The card below names the scope in capitals
(EVERY TASK, GROUP, THIS TASK ONLY), has an edge in its colour and says whom
what you set there holds. Each change is confirmed with the scope's name.

| Scope | What you set |
| --- | --- |
| Every task | What all tasks may cost together in any 24 hours. |
| One group | What its tasks may cost together over all of their runs, and over any 24 hours; each is optional. |
| One task | What that task's runs may cost together. *What holds its next run* lists each budget over it and marks the one that holds. |

A record of what runs spent that cannot be read holds every budget it
could count for, until it is mended or removed or the budget is turned off;
the task header's chip says *N usage records unreadable · budget held* in red.

What runs set aside as they start counts as spent until each run's usage
comes. The *Spent* and *Last 24 h* lines say so
(*· X set aside for runs at work or that did not end*) and count it in what
is left. To let go of the share of a run that did not end, use
[`kule budget TASK --release`](../cli/budget.md) on the command line.

Leave a field empty for no limit. *Where budgets are set* lists every scope
that has one; click a scope to open it. A task's or a group's ⋮ menu
(*Budget…*) opens its scope directly.

## Limits

What each run may take of this computer. Over the processor limit a run is
slowed down; over memory, disk or time it is stopped, and Live says why.

The same **Applies to** picker as Budget. For a group or a task, each field
says where its value comes from:

| Scope | Each field can be |
| --- | --- |
| Every task | A value, or empty for no limit. |
| One group | **From every task** (the default), **No limit**, or **The group's own** value, for each of its tasks. |
| One task | What it inherits (**From group …** when its group sets that field, else **From every task**), **No limit**, or **Its own** value. |

The fields are processor (cores), memory (GB), disk (GB), **GPU may be used**
(on or off), **Servers (macOS) may listen** (off by default; see
[Terminals and servers](terminals-and-servers.md)), one run (minutes), all
of a task's runs (hours) and doing nothing (minutes). *Groups and tasks with limits of their own* lists every
scope that sets something. A run going on keeps the limits it started with.
See [Budgets and limits](../concepts/budgets-and-limits.md) for how each is
measured.

## Appearance

- **Themes**: Kule's own (System, which follows the computer, Light, Dark,
  Coral, Nord, Solarized Light and two high-contrast ones), yours, and those
  extensions bring. Make a theme from any of them with a live preview; Kule
  warns about colour pairs too faint to read. A theme goes to and comes from
  a `.kule-theme.json` file, which carries colours for the window's named
  parts (`#rrggbb`, checked on the way in) and nothing else.
- **Motion**: the small movements where something changes: menus grow out of
  what opened them, the tab line slides, a new event rises into Live, a
  question opens and pulses twice, task rows slide to their new places, a
  run that ends well draws its tick. **As the computer says** (the default)
  turns it off when the computer's "reduce motion" is on; **On** and **Off**
  decide for themselves.
- **Fonts**: one for the window's text and one for code (the agent chat,
  terminals, diffs and the Preview's code), each from the fonts on your
  computer and with a size of its own: text 11 to 20 px (Kule's is 14), code
  10 to 22 px (13). The code list holds monospaced fonts; you can type any
  font's name. **Kule's fonts** puts both back. Mod+= and Mod+- change both
  sizes at once. A font a profile brings that this computer lacks falls back
  to Kule's own, and the import says so.
- **Copying**: *Copy text when you select it with the mouse*, in the agent
  chat, Review or a terminal; a small *Copied* shows by the pointer. Fields
  you type in copy as usual.

The theme in use and your own themes are kept in `themes.json` in Kule's
home.

## Shortcuts

Every command and its keys. **Search** by name or by keys (like "theme" or
"⌘⇧"), click a command to set your own keys, or **Put back every default**.
Your choices are kept in `shortcuts.json` in Kule's home. See
[Keyboard](keyboard.md).

## Extensions

Extensions add commands, panels, themes and helpers to Kule. Each runs in a
sandboxed frame and may do only what you approve; a change to it waits for
you again. None can answer a question, commit, roll back, read your secrets
or change a policy.

- **Install from a file…** installs a `.kulext`; **Load a folder you are
  writing…** installs a folder you are working on, read live.
- Each extension lists the permissions it asks for, in plain words (for
  example *Read files in a task's copy* or *Send and receive data over the
  internet, to the hosts listed*), with **Approve**, **Turn on** / **Turn
  off** and **Remove** (click again to confirm).
- An extension that is on says *Runs*, or *Does not run now* while no
  extension may run in the window, with why: Kule started without
  extensions this time (safe mode), extensions cannot be served, or they
  wait until the window's WebRTC is off.
- The **MCP servers** an extension offers agents each wait for their own
  approval: **Let agents use it** or **Stop offering it**. An agent starts
  such a server inside its own sandbox.
- Agent profiles an extension brings wait for you under *Settings → Agents*.

See [Extensions](../extensions.md).

## Profile

Your choices in one `.kule-profile.json` file, all of them or the parts you
tick: to set up Kule on another computer the way you have it here, or to
keep a copy.

| Part | What it holds |
| --- | --- |
| Appearance | The theme in use, your own themes, the motion, copying and fonts. |
| Shortcuts | The shortcuts you set yourself. |
| Policy | Kule's balanced or careful policy. |
| Daily budget | What every task may cost together in a day. |
| Limits | What every task's runs may take: processor, memory, disk, GPU, time. |
| Your agents | The agent profiles you added; they wait for approval on the other computer. |

On **import**, Kule shows what the file holds before anything changes, and
you pick the parts to apply. Themes join yours; the rest take the profile's.
Secrets, remembered approvals and a policy you wrote yourself never go into
a profile.

## From a terminal

[`kule agents`](../cli/kule-agents.md), [`kule policy`](../cli/policy.md),
[`kule group policy`](../cli/group.md), [`kule isolation`](../cli/isolation.md),
[`kule rules`](../cli/rules.md), [`kule secrets`](../cli/secrets.md),
[`kule budget`](../cli/budget.md), [`kule daily-budget`](../cli/daily-budget.md),
[`kule limits`](../cli/limits.md) and [`kule ext`](../cli/ext.md) do the
same from a terminal. Appearance, shortcuts and profiles are in the app only.
