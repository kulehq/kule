---
title: kule limits
sidebar_label: kule limits
description: Show or set what an agent run may take of this computer, for every task, a group or one task.
---

# kule limits

Show or set what an agent run may take of this computer: processor, memory,
disk, GPU and time.

## Synopsis

```sh
kule limits [OPTIONS] [SETTINGS]...
```

## Description

**Limits** hold each agent run of a task. They can be set at three
**scopes**:

| Scope | How to pick it | Holds |
| --- | --- | --- |
| Every task | no option (the default) | Every task, where neither its group nor the task sets its own. |
| Each task in a group | `--group ID` | Each task in that group, field by field, where the task sets none of its own. |
| One task | `--task TASK` | That task only. |

A task's own value stands over its group's, and a group's over every task's.
A group's limit holds each of its tasks on its own: `cpu=2` on a group means
2 cores for each task, not 2 cores shared.

Without settings, the command prints the limits of the scope. With settings,
it changes them and then prints them. Each line says where its value comes
from, so you always see which scope you changed.

A run that is going on keeps the limits it started with. A change holds from
the next run. `kule run` and runs from the app are held to the same limits.

### The fields

| Field | Unit | What happens over it |
| --- | --- | --- |
| `cpu` | cores (`1.5` is one and a half) | The run is **slowed down**, not stopped: Kule pauses and resumes its processes in turn. |
| `memory` | GB, all of the run's processes together | The processes are **killed** at once and the run stops. |
| `disk` | GB the copy, its temporary files and home changes may grow by since the task was made | The run **stops**. No single file can be written past it. |
| `gpu` | `on` / `off` | `off`: on macOS Metal (and OpenCL) gets no device; on Linux the GPU's devices stay read-only. |
| `servers` | `on` / `off` (`off` by default) | macOS: `off`, the run's programs do not listen for connections while its network goes through the proxy; `on`, they do, which is weaker isolation you accept: Node, Python and Ruby servers are pointed at this computer, and while an agent runs in the app Kule keeps the others off your network from outside the sandbox (see [kule isolation](isolation.md)). On Linux every server is held to this computer either way. |
| `run` | minutes, one run | The run **stops**. Time waiting for your answers counts. |
| `task` | hours, all of the task's runs added up | The run **stops**, and no new run starts until you raise it. |
| `idle` | minutes doing nothing | The run **stops**. "Doing nothing" means no message, tool call, output, processor use or new process from the agent. A question waiting for you does not count. |

### Values

| Value | Meaning |
| --- | --- |
| a number above 0 | The limit, in the field's unit (`memory=4`, `cpu=1.5`, `run=30`). |
| `none` | No limit for this field at this scope. |
| `on`, `off` | For `gpu` and `servers` only. |
| `inherit` | With `--group` or `--task` only: drop this scope's own value and take the one from the scope above again. |

## Arguments

| Argument | Meaning |
| --- | --- |
| `SETTINGS...` | Zero or more `FIELD=VALUE` pairs, such as `memory=4 cpu=none gpu=off`. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--group` | group id | | Show or set the limits for each task in this group. See `kule group` for ids. |
| `--task` | task id | | Show or set this task's own limits. |
| `--home` | path | `~/.kule` (or `KULE_HOME`) | Kule's home. See [global options](index.md). |
| `-h`, `--help` | | | Print help. |

`--group` and `--task` cannot be used together.

## Examples

The defaults, with nothing set:

```console
$ kule limits
limits for EVERY TASK (where its group and the task set none of their own):
  processor              no limit
  memory                 no limit
  disk                   no limit
  one run                no limit
  all of a task's runs   no limit
  doing nothing          no limit
  GPU                    may be used
  servers (macOS)        do not listen
```

Hold every task to 1.5 cores and 4 GB, stop runs that sit idle for 10
minutes, and turn the GPU off:

```console
$ kule limits cpu=1.5 memory=4 idle=10 gpu=off
limits for EVERY TASK (where its group and the task set none of their own):
  processor              1.5 cores
  memory                 4 GB
  disk                   no limit
  one run                no limit
  all of a task's runs   no limit
  doing nothing          10 min
  GPU                    off
  servers (macOS)        do not listen
```

Tighter limits for each task in group 1:

```console
$ kule limits --group 1 memory=2 run=30
limits for EACH TASK IN GROUP 1 "clients" (1 task), where a task sets none of its own:
  processor              1.5 cores (every task's)
  memory                 2 GB (the group's)
  disk                   no limit (every task's)
  one run                30 min (the group's)
  all of a task's runs   no limit (every task's)
  doing nothing          10 min (every task's)
  GPU                    off (every task's)
  servers (macOS)        do not listen (every task's)
```

One task in that group needs more memory:

```console
$ kule limits --task 1046fb30 memory=8
limits for TASK 1046fb30 only (in group 1 "clients"):
  processor              1.5 cores (every task's)
  memory                 8 GB (its own)
  disk                   no limit (every task's)
  one run                30 min (group "clients")
  all of a task's runs   no limit (every task's)
  doing nothing          10 min (every task's)
  GPU                    off (every task's)
  servers (macOS)        do not listen (every task's)
```

Give it back to the group's value:

```console
$ kule limits --task 1046fb30 memory=inherit
limits for TASK 1046fb30 only (in group 1 "clients"):
  processor              1.5 cores (every task's)
  memory                 2 GB (group "clients")
  ...
```

## Output

- The first line names the scope in capitals: `EVERY TASK`, `EACH TASK IN
  GROUP …`, or `TASK … only`.
- At a group or task scope, the brackets say where each value comes from:
  `(its own)`, `(the group's)` / `(group "clients")`, or `(every task's)`.
- `no limit` means nothing holds that field; `may be used` means the GPU is
  on; `may listen (weaker isolation)` means servers are allowed.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: memory: write a limit as FIELD=VALUE, such as memory=4` | Add `=` and a value. |
| `kule: foo: a limit is cpu, memory, disk, gpu, servers, run, task or idle` | Use one of these field names. |
| `kule: memory=0: a number of GB above 0, or none` | Use a positive number; `none` removes the limit. |
| `kule: gpu: on, off or inherit` | `gpu` and `servers` take only these words. |
| ``kule: memory: `inherit` is for --group or --task; the limits for every task have nothing above them`` | At the every-task scope, use a value or `none`. |
| `error: the argument '--group <GROUP>' cannot be used with '--task <TASK>'` | Pick one scope. |
| `kule: no group 9` / `kule: no task …` | Check the id with `kule group` or `kule list`. |

When a limit stops a run, the run fails with the reason in Live and in
`kule run`'s output, for example `Stopped: the run's processes held 4.2 GB,
over its memory limit of 4 GB`.

## In the app

*Settings → Limits*, where **Applies to** picks *Every task*, *One group* or
*One task*. *Limits…* in a task's or a group's ⋮ menu opens that scope, and
the measure bar under the prompt in Live opens the task's. Moving a task into
or out of a group asks first when its limits would change.

## See also

- [`kule budget`](budget.md), [`kule daily-budget`](daily-budget.md): what runs may cost.
- [`kule group`](group.md): `kule group put` says how a move changes a task's limits.
- [Budgets and limits](../concepts/budgets-and-limits.md)
