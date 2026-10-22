---
title: Budgets and limits
sidebar_label: Budgets and limits
description: Cap what agent runs may cost (per task, per group, per day) and what they may take of the computer (processor, memory, disk, GPU, time), and stop a run at any moment.
---

# Budgets and limits

Two kinds of caps keep agent runs in check:

- **Budgets**: how much money runs may cost, as the agents report it.
- **Limits**: how much of the computer a run may take: processor, memory,
  disk, GPU and time.

Both can be set at **three scopes**.

## Three scopes

| Scope | Budget | Limits |
| --- | --- | --- |
| **Every task** | What all tasks may cost together in any 24 hours. | Hold where a group and a task set none. |
| **A group** | What the group's tasks may cost together, over all their runs and/or over any 24 hours. | Hold each of the group's tasks, field by field, where the task sets none. |
| **One task** | What this task's runs may cost together. | The task's own, field by field. |

For limits, a task's own value stands over its group's, and a group's over
every task's. For budgets, **every budget over a run holds, and the
tightest decides**.

In the app, *Settings → Budget* and *Settings → Limits* show one scope at a
time, picked under **Applies to** (Every task, One group, One task). The
card names the scope in capitals (EVERY TASK, GROUP, THIS TASK ONLY), so a
change never lands where you did not mean it. A task's or group's ⋮ menu
opens the right scope.

## Budgets

```sh
kule budget TASK 5           # this task's runs may cost 5 USD together
kule budget TASK 4.50 EUR    # another currency (ISO 4217)
kule budget TASK off         # no budget for this task
kule budget TASK             # list every budget over its next run

kule group budget ID 20          # a group: over all of its tasks' runs
kule group budget ID 10 --daily  # a group: over any 24 hours

kule daily-budget 20         # every task together, over any 24 hours
```

How a budget holds:

- **No run starts** once a budget over it is spent.
- A run is held to the tightest of what is left. A run whose agent reports
  its cost as it works is **cancelled** when it gets there.
- A refusal names the budget that is spent.
- A run counts for the group its task was in **when it ran**, so moving a
  task out keeps what it spent in the group's account.

:::warning A budget holds only what agents report

- **Claude Code** reports the cost as each turn ends, so a turn can go over
  and the next run is the one refused.
- **Codex** signed in with ChatGPT reports tokens but no cost.
- **Gemini** reports neither.

An agent signed in with a subscription may report no cost at all; a budget
does not hold what is not reported.

:::

What each run used (tokens, the context window, and the cost when the
agent says) is kept in the task's `usage.jsonl`. `kule status` and the
app's task header add it up.

While the window is elsewhere, the app tells you when 80% of a budget is
spent, and when all of it is.

## Limits

```sh
kule limits                                # every task's limits, and where each comes from
kule limits memory=4 cpu=2                 # set them for every task
kule limits --group ID memory=8 gpu=off    # for each task in a group
kule limits --task TASK run=30 idle=5      # for one task
kule limits --task TASK memory=inherit     # take the group's / every task's again
kule limits cpu=none                       # no processor limit
```

| Field | Unit | Over it |
| --- | --- | --- |
| `cpu` | cores (`1.5` = one and a half) | The run is **slowed down**, not stopped: Kule pauses and resumes its processes in turn (`SIGSTOP`/`SIGCONT`, a tenth of a second at a time). |
| `memory` | GB, all the run's processes together | The processes are **killed at once**; the run stops. |
| `disk` | GB the copy, its temporary files and home changes may grow by since the task was made | The run **stops**. No single file can be written past it. |
| `gpu` | `on` / `off` | Off: Metal (and OpenCL) gets no device on macOS; `/dev/dri` and NVIDIA's devices stay read-only on Linux. |
| `servers` | `on` / `off` (off by default) | macOS: off, the run's programs do not listen for connections while its network goes through the proxy; on, they do, which is weaker isolation you accept: Node, Python and Ruby servers are pointed at this computer, and while an agent runs in the app Kule keeps the others off your network from outside the sandbox (see [`kule isolation`](../cli/isolation.md)). On Linux every server is held to this computer whatever it says. |
| `run` | minutes, one run | The run **stops**. |
| `task` | hours, all of a task's runs together | The run **stops**, and none starts until you raise it. |
| `idle` | minutes doing nothing | The run **stops**. |

Notes on how they count:

- **Memory** is the processes' footprint on macOS, their proportional share
  on Linux.
- **Disk** counts only what the task added, measured as often as a walk
  allows (at most every second). A large folder does not use up its own
  limit, nor do the package caches a task starts with.
- **Doing nothing** means no message, tool call or output from the agent,
  no processor use and no new process. A long, quiet build is not doing
  nothing. A question waiting on you does not count.
- **One run** counts all of the run's time, including waiting for your
  answers.
- No system lets a share of a GPU be set, so `gpu` is only on or off.
- The macOS sandbox sees the port a server listens on but not its
  address, so `servers` is on or off too.
- A run keeps the limits it started with; a change holds from the next run.
- `kule run` from a terminal is held to the same limits.

When a limit stops a run, Live says which one and what it measured:

```text
Stopped: the run's processes held 4.2 GB, over its memory limit of 4 GB
```

While a run goes on, the bar under the prompt shows what it takes against
its limits (`CPU 140% / 200% · 1.2 GB / 4 GB · disk +350 MB · 7 processes`)
and adds *slowed* while the processor limit holds it back.

## Stop

**Stop** in the app (Mod+Shift+K) stops the run at work whenever you want:
its turn is cancelled, and the agent is killed if it does not end it within
ten seconds. What it changed so far stays in the copy for you to review.

## Related

- [`kule budget`](../cli/budget.md), [`kule daily-budget`](../cli/daily-budget.md), [`kule limits`](../cli/limits.md), [`kule group`](../cli/group.md)
- [Groups](groups.md)
- [Live](../app/live.md), [Settings](../app/settings.md)
