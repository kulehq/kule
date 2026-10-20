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

How a budget holds:

- No run starts once a budget over it is spent. The refusal names the budget.
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

## Output

- `spent: … in N runs` adds up what the agents reported for this task. When
  some runs reported tokens but no cost, the line says how many
  (`(2 without a cost)`).
- `budgets over task …'s next run` lists each budget with what is left. A
  budget that is spent shows `0.00` left.
- `the next run may cost at most …` is the tightest of what is left.
- `no budget holds task …` means no budget of any scope is set.
- `no run starts: …` means one of the budgets is spent; the text names which.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: abc: an amount like 5 or 2.50, or off` | Write a number, or `off`. |
| `kule: a budget is a positive amount in a currency like USD, not 0 USD` | Use an amount above 0, and a three-letter currency in capitals (`USD`, not `usd`). To remove a budget, use `off`. |
| `kule: no task 1046fb3` | Use the full task id from `kule list`. |
| `task 1046fb30 has spent its budget; raise it or turn it off to run again` (from `kule run` or the app) | Raise it (`kule budget TASK 10`) or remove it (`kule budget TASK off`). |

Every error exits with status 2.

## In the app

*Settings → Budget*, with **Applies to: One task**, or *Budget…* in the task's
⋮ menu. The task header's chip shows what the task spent and what is left.
While the window is not in front, Kule tells you when 80% of a budget is
spent, and when all of it is.

## See also

- [`kule daily-budget`](daily-budget.md): every task together, in any 24 hours.
- [`kule group`](group.md): a group's budgets (`kule group budget`).
- [`kule limits`](limits.md): processor, memory, disk and time limits.
- [Budgets and limits](../concepts/budgets-and-limits.md)
