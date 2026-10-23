---
title: kule group
sidebar_label: kule group
description: Make and manage groups of tasks, each with a colour, a policy, budgets and limits of its own.
---

# kule group

Make and manage **groups** of tasks. A group has a name, a colour, the policy
its tasks are judged by, and budgets and limits of its own.

## Synopsis

```sh
kule group [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-group-list) | List the groups, their policies and tasks (the default). |
| [`new`](#kule-group-new) | Make a group. |
| [`rename`](#kule-group-rename) | Rename a group. |
| [`color`](#kule-group-color) | Set or remove a group's colour. |
| [`policy`](#kule-group-policy) | Choose or print the policy a group's tasks are judged by. |
| [`budget`](#kule-group-budget) | Show or set what a group's tasks may cost together. |
| [`put`](#kule-group-put) | Put a task in a group, or take it out. |
| [`delete`](#kule-group-delete) | Delete a group; its tasks stay. |

## Description

Groups let you treat a set of tasks alike: all the tasks for one client, or
all the experiments you want held to the careful policy. A task is in at most
one group. Groups are numbered (`1`, `2`, …); every subcommand that takes `ID`
wants that number, as `kule group` prints it.

What a group decides for its tasks:

- **Policy**: the rules that judge every change and connection of its tasks
  (see [Policy](../concepts/policy.md)). By default a group uses the policy
  for every task.
- **Budgets**: a total over all of its tasks' runs, and one over any 24 hours.
  Both hold next to each task's own budget and the daily one for every task.
- **Limits**: set with [`kule limits --group ID`](limits.md). They hold each
  task in the group where the task sets none of its own.

A repository's own `forbid` rules (`.kule/policy.cedar`) and what you told
Kule not to ask about again ([`kule rules`](rules.md)) hold in every group.
See [Groups](../concepts/groups.md) for the full picture.

All subcommands take `--home` (see [global options](index.md)).

## kule group list

```sh
kule group list [OPTIONS]
```

Lists every group with its id, name, policy, budgets, whether it has limits
of its own, its colour, and its tasks. `kule group` with no subcommand does
the same.

```console
$ kule group
1  clients · the policy for every task · budget 20.00 USD, 5.00 USD a day · limits of its own (kule limits --group 1) (teal)
    1046fb30  ~/code/app
2  side projects · the policy for every task (red)
```

With no group yet:

```console
$ kule group
no groups: every task is judged by the policy for every task
```

## kule group new

```sh
kule group new [OPTIONS] <NAME>
```

| Argument / option | Meaning |
| --- | --- |
| `NAME` | The group's name, up to 60 characters. |
| `--color COLOR` | One of `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`, `pink`. Without it, Kule picks a colour no other group uses. |

A new group uses the policy for every task until you choose another. You can
have up to 100 groups.

```console
$ kule group new clients --color teal
group 1 made: clients
$ kule group new "side projects"
group 2 made: side projects
```

## kule group rename

```sh
kule group rename [OPTIONS] <ID> <NAME>
```

```console
$ kule group rename 2 "Side projects"
```

Prints nothing when it works.

## kule group color

```sh
kule group color [OPTIONS] <ID> [COLOR]
```

Sets the group's colour, or removes it when `COLOR` is left out. Its tasks
show that colour unless they have one of their own (see
[`kule color`](color.md)).

```console
$ kule group color 2 purple
$ kule group color 2
```

## kule group policy

```sh
kule group policy [OPTIONS] <ID> [CHOICE]
```

| Argument / option | Meaning |
| --- | --- |
| `ID` | The group. |
| `CHOICE` | `default` (the policy for every task), `balanced`, `careful`, or `own`. Leave it out to print the rules that hold for the group. |
| `--file PATH` | With `own`: a Cedar file with the group's policy. It is checked before it is kept (up to 256 KiB). |

The group's tasks are judged by the new choice from their next commit and run.

```console
$ kule group policy 1 careful
group 1's tasks are judged by it from their next commit and run
$ kule group policy 1 | tail -3
@id("careful")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" || resource.kind like "git.*" };
```

A policy of the group's own:

```console
$ kule group policy 1 own --file clients.cedar
group 1's tasks are judged by it from their next commit and run
```

After the file is kept, `kule group policy 1 own` (without `--file`) switches
back to it. See [Policy rules](../reference/policy-rules.md) for how to write
one; the simplest start is a copy of `kule policy` with your changes.

:::warning

A policy of your own *replaces* Kule's for the group. Rules you leave out no
longer hold for those tasks, except a repository's own forbids. Start from
the text of Kule's balanced policy (`kule policy` prints it while balanced
holds) and change only what you mean to.

:::

## kule group budget

```sh
kule group budget [OPTIONS] <ID> [AMOUNT] [CURRENCY]
```

| Argument / option | Meaning |
| --- | --- |
| `ID` | The group. |
| `AMOUNT` | A positive amount like `20` or `7.50`, or `off`. Leave it out to show both budgets and what was spent. |
| `CURRENCY` | ISO 4217, in capitals. Default `USD`. |
| `--daily` | Set the budget over any 24 hours instead of the one over all runs. |

A group has two optional budgets: one over **all of its tasks' runs**, and
one over **any 24 hours**. A run counts for the group its task was in when it
ran, so moving a task out keeps what it spent in the group's account.

```console
$ kule group budget 1 20
budgets for GROUP 1 "clients": its 1 task together, beside each task's own and the daily one for every task:
  all their runs   20.00 USD, 0.00 USD spent, 20.00 USD left
  any 24 hours     no budget (nothing reported spent)
$ kule group budget 1 5 --daily
budgets for GROUP 1 "clients": its 1 task together, beside each task's own and the daily one for every task:
  all their runs   20.00 USD, 0.00 USD spent, 20.00 USD left
  any 24 hours     5.00 USD, 0.00 USD spent, 5.00 USD left
$ kule group budget 1 off --daily
```

When a group budget is spent, the group's tasks start no run:
`group "clients" has spent its budget: its tasks together; raise it or turn it off to run again`.
Nor do they while a record of what one of its tasks' runs spent cannot be
read: it counts for the group the task is in now, in both budgets, and the
refusal says `Kule cannot tell what group "clients"'s budget has left: …`
(or `… daily budget …`).

What a run sets aside as it starts counts for the group the task was in
then, until the run's usage is recorded: in the budget over all runs, and in
the one over any 24 hours for 24 hours. The lines say so:

```console
$ kule group budget 1
budgets for GROUP 1 "clients": its 1 task together, beside each task's own and the daily one for every task:
  all their runs   20.00 USD, 2.00 USD spent, 8.00 USD left (10.00 USD set aside for runs at work or that did not end)
  any 24 hours     no budget (nothing reported spent)
```

While nothing is left but what is set aside, the group's tasks start no
run either:
``what is left of group "clients"'s budget (10.00 USD) is set aside for runs at work, or that did not end, in task <id>: once a run ends, what it spent counts instead; `kule budget TASK --release` lets go of one that did not end``.

## kule group put

```sh
kule group put [OPTIONS] <TASK> [GROUP]
```

Puts `TASK` in group `GROUP`, or takes it out of its group when `GROUP` is
left out. It then says what changed in the limits and group budgets over the
task's runs, so nothing loosens without you seeing it.

```console
$ kule group put 1046fb30 1
task 1046fb30 is in group 1 "clients"
  its limits: memory 4 GB → 2 GB, one run no limit → 30 min
$ kule group put 1046fb30
task 1046fb30 is in no group
  its limits: memory 2 GB → 4 GB, one run 30 min → no limit
```

The new group's policy judges the task's next commit and run.

## kule group delete

```sh
kule group delete [OPTIONS] <ID>
```

Deletes the group. Its tasks stay, in no group, and are judged by the policy
for every task again.

```console
$ kule group delete 2
group 2 deleted; its tasks are in no group
```

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: no group 9` | Check the id with `kule group`. |
| `kule: a task's colour is one of red, orange, yellow, green, teal, blue, purple, pink, not "magenta"` | Pick one of these colours. |
| `kule: strict: a group's policy is default, balanced, careful or own` | Use one of the four choices. |
| `kule: write the group's policy first` | `own` needs `--file PATH` the first time. |
| `kule: a policy's text goes with a policy of the group's own` | `--file` only goes with `own`. |
| `kule: policy: unexpected end of input` (or another Cedar error) | The file is not valid Cedar; nothing was changed. Fix the file and try again. |
| `kule: at most 100 groups; delete one first` | Delete a group you no longer use. |

## In the app

The **+** next to *Tasks* makes a group. A group's ⋮ menu renames it, changes
its colour, opens *Budget…* and *Limits…* for it, and deletes it. Drag a task
onto a group (or use *Group…* in the task's menu) to put it there, and onto
*Not in a group* to take it out. When a move would change the verdict on any
of the task's pending changes, or its limits and budgets, the app asks first.
A group's policy is chosen in *Settings → Policy*.

## See also

- [`kule limits`](limits.md): a group's limits (`--group ID`).
- [`kule policy`](policy.md): the policy for every task.
- [Groups](../concepts/groups.md), [Policy](../concepts/policy.md), [Budgets and limits](../concepts/budgets-and-limits.md)
