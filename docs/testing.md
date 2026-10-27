---
title: Help test Kule
sidebar_label: Help test Kule
description: Scenarios to try Kule on macOS, Linux or Windows, what to look for in each, and how to report what you find without sharing secrets.
---

# Help test Kule

Kule is a preview, and what it needs most now is people using it on their
own computers and their own work. Reports of every kind help: a bug, a step
that was unclear, a refusal that got in your way, and a run where
everything worked.

This page has six scenarios. Pick any of them; each takes 5 to 20
minutes. At the end of each one, ask yourself one question: **what
happened that you did not expect?**

## Before you start

- **Platforms:** macOS 13 or later (Apple silicon or Intel), Linux x86_64,
  and Windows 11 24H2 with update 26100.9278 or later (x64 or ARM64).
  Windows is the newest: reports from there help most, and
  [Platforms](platforms.md) says what to expect of its sandbox.
- **Use the latest preview.** Fixes are not backported, so a report on an
  older one may already be fixed. `kule --version` tells you which one you
  have.
- **Pick your folder with care.** Kule does not change your folder until
  you commit, but it is a preview. Start with a test folder, or with a
  project whose work is all committed and pushed.
- **Agents use your own accounts.** A run spends your own agent's quota or
  API credit, as it would without Kule.

## 1. Install and check

Follow [Install](getting-started/install.md): the `.dmg`, `.deb` or `.rpm`
from the releases page, or a build from source. Then run:

```sh
kule --version
kule agents list
kule isolation
```

**Look for:** a step the page does not explain, a command that differs
from the page, a checksum or attestation check that fails, macOS refusing
to open the app, `kule isolation` listing something missing on Linux.

## 2. The loop, without an agent

Follow [Quick start in a terminal](getting-started/quick-start.md),
steps 1 to 8 and 10. Nothing in it needs an agent or the network.

**Look for:** output that is not what the page shows, a change missing
from `kule status`, `kule undo` not putting the folder back exactly.

## 3. The app, with an agent

Sign in to one agent (Claude Code, Codex or Gemini) and follow
[Your first task in the app](getting-started/first-task-in-the-app.md).

**Look for:** an agent that shows as missing although it works in a
terminal, a question card you did not understand, Review naming a change
wrongly (the function, section or key), Commit or Undo not doing what the
button says.

## 4. Your own work

Make a task on a project of yours and ask an agent for something you would
ask it anyway: a small fix, a test, a refactor, a dependency update.

**Look for:**

- the policy asking you too often, or letting something pass that you
  would have wanted to see;
- a tool, command or host your work needs that Kule refuses, and whether
  the refusal said why;
- how long the task's copy took to make, and how much space it took;
- anything the agent did that Kule did not show you.

## 5. Your own setup

Kule is tested on a few machines. Yours is probably different, and that is
the point. Tell us how it went on:

- a Linux distribution other than Ubuntu, or Ubuntu 24.04 with its user
  namespace restriction;
- a file system other than APFS, Btrfs or XFS;
- a large repository or a monorepo;
- an Intel Mac;
- another agent that speaks ACP (see [Agents](using-agents.md)).

## 6. The boundaries

Kule's promise is that an agent cannot reach past the task: outside its
copy, around the policy, to your secrets or to a host the policy refuses.
[Security](security.md) lists those boundaries, and
[Platforms](platforms.md) what each platform does not do yet.

If you find a way past one, **do not open an issue**: report it privately,
as [SECURITY.md](https://github.com/kulehq/kule/blob/main/SECURITY.md)
says. The fix is made in private and released first, and the advisory
credits you unless you prefer not.

Kule's own attempts to get past its boundaries run from a clone of the
repository, on your computer:

```sh
cargo test escape_
```

Each one first does its thing outside Kule, where it must get through,
and then in a task, where it must not. On Windows the attempts need a
Windows 11 24H2 desktop session; set `KULE_TEST_REQUIRE_SANDBOX=1` so one
that cannot run there fails rather than skips. One that fails is a private
report too, not an issue. They stand beside the external tools the release
gate runs, never in their place.

## Reporting what you found

Open an issue at
[github.com/kulehq/kule/issues/new/choose](https://github.com/kulehq/kule/issues/new/choose)
and pick one:

| Form | For |
| --- | --- |
| **Bug report** | Something broke or did not do what the docs say. |
| **Test report** | How a scenario went: what was unclear, slow or in the way. "Everything worked" is welcome too. |
| **Idea or change** | Something Kule should do, or do differently. |

Questions and longer conversations are welcome in
[Discussions](https://github.com/kulehq/kule/discussions).

The forms ask for what helps most: the version, the platform, the agent,
the steps, and `kule isolation`'s output.

### Keep your secrets out of it

Issues are public. Before you paste anything:

- remove API keys, tokens, passwords and cookies;
- remove what you would not publish: private paths, host names, file
  contents, prompts about your work;
- from the daemon's log (`~/.kule/kuled.log`), paste only the lines around
  the problem, not the whole file.

A shorter report without them is still a good report.

## What happens next

Every report is read. A fix ships in the next preview, and its issue says
which one. The [CHANGELOG](https://github.com/kulehq/kule/blob/main/CHANGELOG.md)
lists what each preview changed.

