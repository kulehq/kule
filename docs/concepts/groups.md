---
title: Groups
sidebar_label: Groups
description: Organise tasks into groups with a name and colour, and give each group its own policy, shared budget and limits.
---

# Groups

A **group** collects tasks. Each group has:

- a **name** and a **colour** (its tasks show that colour unless they have
  their own);
- the **policy** its tasks are judged by;
- a **budget** its tasks share;
- **limits** for each of its tasks.

Use groups to keep clients, projects or kinds of work apart: for example a
"clients" group under the careful policy with a daily budget, and a
"sandbox" group under the balanced one.

## Make and fill a group

```sh
kule group new clients --color blue   # make a group
kule group                            # list groups, their policies and tasks
kule group put TASK GROUP_ID          # put a task in it
kule group put TASK                   # take it out of its group
kule group rename ID "Client work"
kule group color ID teal              # or no colour to take it away
kule group delete ID                  # its tasks stay, in no group
```

A group's id is a small number, printed when you make it and by `kule
group`:

```text
group 1 made: clients
1  clients · the policy for every task (blue)
```

Colours are `red`, `orange`, `yellow`, `green`, `teal`, `blue`, `purple`
and `pink`.

In the app: the **+** next to *Tasks* makes a group. Drag a task onto it,
or use *Group…* in the task's ⋮ menu; drag it onto *Not in a group* to take
it out. The group's ⋮ menu changes its colour, policy, budget and limits.

## A group's policy

```sh
kule group policy ID            # print the rules that hold for the group
kule group policy ID default    # the policy for every task (the default)
kule group policy ID careful    # Kule's careful one, whatever the rest use
kule group policy ID balanced   # Kule's balanced one
kule group policy ID own --file group.cedar   # one you write, in Cedar
```

A policy you write is checked before it is kept. A repository's own
refusals (`.kule/policy.cedar`) and what you told Kule not to ask about
again still hold for the group's tasks. See [Policy](policy.md).

## A group's budget and limits

```sh
kule group budget ID 20          # over all of its tasks' runs
kule group budget ID 10 --daily  # over any 24 hours
kule group budget ID             # show both and what was spent
kule group budget ID off         # take one away

kule limits --group ID memory=8 cpu=2
```

A group's limits hold **each** of its tasks, not all of them together: a
group's processor limit is per task. A run counts for the group its task
was in when it ran. See [Budgets and limits](budgets-and-limits.md).

## Moving a task asks first

Moving a task into or out of a group can change how it is treated. When the
move would change:

- the verdict on any of the task's pending changes (a change waiting for
  you that would pass on its own, say), or
- the limits or group budgets that hold its runs
  (`Its limits: memory 8 GB → 2 GB`),

Kule lists what changes and asks first. `kule group put` prints the changes.

## Related

- [`kule group`](../cli/group.md), [`kule limits`](../cli/limits.md)
- [Tasks and copies](tasks-and-copies.md)
- [Settings](../app/settings.md)
