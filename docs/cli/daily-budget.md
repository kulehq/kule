---
title: kule daily-budget
sidebar_label: kule daily-budget
description: Show or set what every task may cost together in any 24 hours.
---

# kule daily-budget

Show or set what all your tasks may cost together in any 24 hours.

## Synopsis

```sh
kule daily-budget [OPTIONS] [AMOUNT] [CURRENCY]
```

## Description

The **daily budget** is one budget over every task in Kule's home. It counts
the runs started in the last 24 hours, so it rolls: a run from yesterday
afternoon stops counting tomorrow afternoon.

- No run starts anywhere once the runs of the last 24 hours have spent it.
- A run that starts is held to what is left, the same way a task's own budget
  holds it (see [`kule budget`](budget.md)).
- Costs are the ones the agents report. Runs that report no cost are not
  held.
- A record of what a run spent that cannot be read, in any task, holds
  every run: Kule cannot tell when it was made, so it may be today's.

With an amount, the command sets the daily budget; with `off`, it removes
it; without either, it only shows it. Every form prints what all tasks spent
in the last 24 hours.

## Arguments

| Argument | Meaning |
| --- | --- |
| `AMOUNT` | A positive amount like `20` or `7.50`, or `off`. Leave it out to show the budget. |
| `CURRENCY` | An ISO 4217 code in capitals, like `USD` or `EUR`. Default: `USD`. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` (or `KULE_HOME`) | Kule's home. See [global options](index.md). |
| `-h`, `--help` | | | Print help. |

## Examples

Show it when none is set:

```console
$ kule daily-budget
last 24 hours, every task: no cost reported in 0 runs; no daily budget
```

Allow 20 USD a day across all tasks:

```console
$ kule daily-budget 20
last 24 hours, every task: no cost reported in 0 runs; daily budget 20.00 USD, 20.00 USD left
```

After a few runs:

```console
$ kule daily-budget
last 24 hours, every task: 3.40 USD in 6 runs; daily budget 20.00 USD, 16.60 USD left
```

Remove it:

```console
$ kule daily-budget off
last 24 hours, every task: no cost reported in 0 runs; no daily budget
```

## Output

`last 24 hours, every task: <cost> in <N> runs` is what every task's runs
started in the last 24 hours reported. When costs came in more than one
currency, they are joined with `+`. After the `;` comes the daily budget and
what is left of it, or `no daily budget`.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: abc: an amount like 20 or 7.50, or off` | Write a number or `off`. |
| `kule: a budget is a positive amount in a currency like USD, not 0 USD` | Use an amount above 0 and a capitalised currency code. |
| `the daily budget is spent: every task together, in the last 24 hours; raise it or turn it off to run again` (when a run is refused) | Wait until older runs fall out of the 24 hours, raise the budget, or turn it off. |
| `Kule cannot tell what the daily budget has left: <N> records of what runs spent (in tasks' usage.jsonl) cannot be read; mend or remove them, or turn the budget off, to run again` (when a run is refused) | Mend or remove the lines that cannot be read in the tasks' `usage.jsonl` files, or turn the daily budget off. |

## In the app

*Settings → Budget*, with **Applies to: Every task**. When the window is not in
front, Kule tells you when 80% of the daily budget is spent and when all of it
is. A budget that drops back below 80% as older runs leave the 24 hours is
told again when it nears.

## See also

- [`kule budget`](budget.md): one task's budget.
- [`kule group`](group.md): a group's budgets, including its own daily one (`kule group budget ID AMOUNT --daily`).
- [Budgets and limits](../concepts/budgets-and-limits.md)
