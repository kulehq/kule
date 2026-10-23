---
title: kule budget
sidebar_label: kule budget
description: Show or set how much one task's agent runs may cost together.
---

# kule budget

Show or set how much one task's agent runs may cost together.

## Synopsis

```sh
kule budget [OPTIONS] <TASK> [AMOUNT] [CURRENCY]
kule budget [OPTIONS] <TASK> --release
```

## Description

A **budget** caps what agent runs cost, as the agents themselves report it.
`kule budget` sets the budget of one [task](../concepts/tasks-and-copies.md):
all of that task's runs together, from the first run on.

- **With an amount**, it sets the task's budget and prints what the task has
  spent and what is left.
- **With `off`**, it takes the task's budget away.
- **Without an amount**, it lists *every* budget that holds the task's next
  run: its own, its group's (see [`kule group budget`](group.md)) and the daily
  budget for every task (see [`kule daily-budget`](daily-budget.md)). It also
  says how much the next run may cost at most: the tightest of them.
- **With `--release`**, it lets go of what runs in the task that did not end
  still hold of the budgets (see below).

How a budget holds:

- No run starts once a budget over it is spent. The refusal names the budget.
- No run starts either while a record of what runs spent, which the budget
  counts, cannot be read: Kule cannot tell what is left. It counts such a
  record rather than take it for nothing spent. Mend or remove it (in the
  task's `usage.jsonl`), or turn the budget off.
- Before its agent starts, a run sets aside its share: the tightest of what
  is left, with what other runs set aside counted as spent. Runs started at
  once under one budget share what is left. When the run's usage is
  recorded, only what it spent counts.
- A run whose usage never comes (Kule was stopped, the record could not be
  written or read) keeps its whole share counted. `--release` lets go of
  it, so it no longer counts as spent, and keeps a record of that in the
  task's `usage.jsonl`. It is refused while an agent is at work in the task.
- A run whose agent reports its cost while it works is stopped when it
  reaches what is left.
- Claude Code reports its cost when each turn ends, so one turn can go over;
  the *next* run is the one that is refused.
- An agent that reports no cost (for example one signed in with a
  subscription) is not held by a budget. Kule cannot hold what it is not told.
- A budget counts only costs reported in its own currency. Agents report in
  USD, so keep budgets in USD unless you know your agent reports another
  currency.

`kule budget` changes only the task's own setting in Kule's home. It never
touches the task's copy or your folder. See
[Budgets and limits](../concepts/budgets-and-limits.md) for how the scopes fit
together.

## Arguments

| Argument | Meaning |
| --- | --- |
| `TASK` | The task's id, as `kule list` prints it (for example `1046fb30`). The full id is needed. |
| `AMOUNT` | A positive amount like `5` or `2.50`, or `off` to remove the budget. Leave it out to show the budgets. |
| `CURRENCY` | An ISO 4217 code in capitals, like `USD` or `EUR`. Default: `USD`. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--release` | | | Let go of what runs in the task that did not end still hold of the budgets, so it no longer counts as spent. Refused while an agent is at work in the task. Takes no `AMOUNT` or `CURRENCY`. |
| `--home` | path | `~/.kule` (or `KULE_HOME`) | Kule's home. See [global options](index.md). |
| `-h`, `--help` | | | Print help. |

## Examples

Give a task a budget of 5 USD:

```console
$ kule budget 1046fb30 5
spent: nothing reported in 0 runs; budget 5.00 USD, 5.00 USD left
```

See every budget over the task's next run:

```console
$ kule budget 1046fb30
budgets over task 1046fb30's next run:
  the task's own                   5.00 USD, 5.00 USD left
the next run may cost at most 5.00 USD
```

A task in a group with budgets of its own, while a daily budget is set too:

```console
$ kule budget 1046fb30
budgets over task 1046fb30's next run:
  group "clients", all its runs    20.00 USD, 20.00 USD left
  group "clients", any 24 hours    5.00 USD, 5.00 USD left
the next run may cost at most 5.00 USD
```

Set a budget in euros, then take it away:

```console
$ kule budget 1046fb30 2.50 EUR
spent: nothing reported in 0 runs; budget 2.50 EUR, 2.50 EUR left
$ kule budget 1046fb30 off
```

`off` prints nothing when the task has no runs yet.

While a run in another task is at work under the daily budget, what it set
aside shows beside the budget, and counts:

```console
$ kule budget 1046fb30
budgets over task 1046fb30's next run:
  every task, any 24 hours         10.00 USD, 0.00 USD left (10.00 USD set aside for runs at work or that did not end)
no run starts: what is left of the daily budget (10.00 USD) is set aside for runs at work, or that did not end, in task 7c2e9a10: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end
```

Let go of the share of a run in that task that did not end (its `kule run`
was killed):

```console
$ kule budget 7c2e9a10 --release
let go of 10.00 USD set aside for a run that did not end; it no longer counts as spent
```

A `spent:` line follows when the task has runs or a budget of its own.

## Output

- `spent: … in N runs` adds up what the agents reported for this task. When
  some runs reported tokens but no cost, the line says how many
  (`(2 without a cost)`). When some of the task's records cannot be read, it
  says how many (`(1 record cannot be read)`). What runs at work, or that
  did not end, set aside is added as
  ` (10.00 USD set aside for runs at work or that did not end)`.
- `budgets over task …'s next run` lists each budget with what is left. A
  budget that is spent shows `0.00` left. What is set aside counts as spent,
  and is shown after it.
- `the next run may cost at most …` is the tightest of what is left.
- `no budget holds task …` means no budget of any scope is set.
- `no run starts: …` means one of the budgets is spent, or what is left of
  it is set aside; the text names which.
- `let go of … set aside for a run that did not end; it no longer counts as spent`
  is what `--release` let go of, one line per share;
  `nothing is set aside for runs that did not end in task …` when there was
  none.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: abc: an amount like 5 or 2.50, or off` | Write a number, or `off`. |
| `kule: a budget is a positive amount in a currency like USD, not 0 USD` | Use an amount above 0, and a three-letter currency in capitals (`USD`, not `usd`). To remove a budget, use `off`. |
| `kule: no task 1046fb3` | Use the full task id from `kule list`. |
| `task 1046fb30 has spent its budget; raise it or turn it off to run again` (from `kule run` or the app) | Raise it (`kule budget TASK 10`) or remove it (`kule budget TASK off`). |
| ``what is left of task 1046fb30's budget (5.00 USD) is set aside for runs at work, or that did not end, in task 1046fb30: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end`` (from `kule run`, the app, or `no run starts:` here) | Wait for the run at work to end. If none is at work (Kule was stopped), let go of its share: `kule budget TASK --release`, in the task the message names. |
| `kule: an agent is at work in task 1046fb30 (process 4242); its run holds its share until it ends` (from `--release`) | Wait for the run to end, or stop it; then what it spent counts instead. |
| `Kule cannot tell what task 1046fb30's budget has left: 1 record of what runs spent (a task's usage.jsonl) cannot be read; mend or remove them, or turn the budget off, to run again` (from `kule run`, the app, or `no run starts:` here) | Mend or remove the line that cannot be read in the task's `usage.jsonl`, or turn the budget off. The message names a group's or the daily budget when that is the one held. |

Every error exits with status 2.

## In the app

*Settings → Budget*, with **Applies to: One task**, or *Budget…* in the task's
⋮ menu. The task header's chip shows what the task spent and what is left,
and in red *N usage records unreadable · budget held* when some cannot be read.
While runs hold a share, it says *X set aside* and counts it in what is left.
`--release` is on the command line only.
While the window is not in front, Kule tells you when 80% of a budget is
spent, and when all of it is.

## See also

- [`kule daily-budget`](daily-budget.md): every task together, in any 24 hours.
- [`kule group`](group.md): a group's budgets (`kule group budget`).
- [`kule limits`](limits.md): processor, memory, disk and time limits.
- [Budgets and limits](../concepts/budgets-and-limits.md)
