---
title: Quick start in a terminal
sidebar_label: Quick start (CLI)
description: A five-minute tutorial that makes a task, changes its copy in the sandbox, reviews, commits, undoes and rolls back, then hands the task to an agent.
---

# Quick start in a terminal

In five minutes you will make a task on a small folder, change its copy in
the sandbox, review what changed, commit it to the real folder, take it
back, and finally let an agent do the work.

You need `kule` on your `PATH` ([Install](install.md)). Every command below
can be pasted as it is.

## 1. Make a demo folder

```sh
mkdir -p ~/demo && cd ~/demo
printf '# Notes\n\n## Plan\n\n- write docs\n' > notes.md
printf 'hello\n' > hello.txt
```

This is your **workspace**: the real folder. Kule will not change it until
you commit.

## 2. Make a task

```sh
kule new ~/demo
```

```text
task c224e328 · copy at ~/.kule/tasks/c224e328/w/demo
```

The **task** has an id (`c224e328` here; yours differs) and a **copy** of
the folder. On macOS and on Btrfs or XFS the copy is a clone and takes no
extra space until files change.

Keep the id in a variable so the rest of this page pastes as it is. `kule
list` prints one task per line, id first:

```sh
kule list
```

```text
c224e328  ~/demo
```

```sh
T=c224e328      # use your own id
```

See [`kule new`](../cli/new.md) and [`kule list`](../cli/list.md).

## 3. Run a command in the sandbox

[`kule exec`](../cli/exec.md) runs any command inside the task's copy, in
the sandbox, then lists what it changed:

```sh
kule exec $T -- sh -c '
  printf "\n## Later\n\n- ship it\n" >> notes.md
  echo hi > new.txt
  rm hello.txt'
```

```text
3 file changes: +1 ~1 -1
  - hello.txt
  + new.txt
  ~ notes.md
      + section Notes › Later
```

`+` is a created file, `~` a modified one, `-` a deleted one. Under a
changed file, Kule names *what* changed inside it: here a new Markdown
section. For code it names functions and methods; for JSON and TOML, keys.

Your real folder is untouched:

```sh
ls ~/demo        # hello.txt  notes.md
```

## 4. Review

```sh
kule status $T
```

```text
3 file changes: +1 ~1 -1
  - hello.txt
  + new.txt
  ~ notes.md
      + section Notes › Later
policy: passes on its own
```

The last line is the **policy's verdict**. Source, docs and data pass on
their own; other kinds of file and hidden files wait for you. See [`kule status`](../cli/status.md) (it also has `--json`).

## 5. Commit

```sh
kule commit $T
```

```text
committed 3 changes · checkpoint 1
```

Now the changes are in `~/demo`. The commit kept **checkpoint 1**: what it
replaced, so it can be undone.

```sh
ls ~/demo        # new.txt  notes.md
kule history $T
```

```text
  0. commit 1 · 3 changes · snapshot 0d0d2c26745d
history verified: 1 entries, signed
```

Every commit and undo is an entry in a signed, hash-chained history. See
[`kule commit`](../cli/commit.md) and [`kule history`](../cli/history.md).

## 6. Undo

```sh
kule undo $T
```

```text
undid checkpoint 1 (3 changes)
```

`~/demo` is back as it was: `hello.txt` and `notes.md`, without the new
section. Run `undo` again to walk further back. See
[`kule undo`](../cli/undo.md).

## 7. A change that waits for you

Some files decide what gets installed, built or run. The policy does not
let them pass on their own:

```sh
kule exec $T -- sh -c 'echo "{\"name\":\"demo\"}" > package.json; echo scratch > draft.txt'
kule status $T
```

```text
2 file changes: +2 ~0 -0
  + draft.txt
  + package.json
      + key name
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    package.json
```

`manifests-and-lockfiles` is the name of the rule that asked. A plain
commit now refuses:

```sh
kule commit $T
```

```text
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    package.json
kule: not committed; rerun with --approve to approve the changes above
```

You have two choices:

- `kule commit $T --approve` approves and commits. Add `--remember` to stop
  asking about these files in this folder.
- `kule rollback $T` throws the changes away.

## 8. Roll back

```sh
kule rollback $T
```

```text
rolled back 2 changes
```

The copy matches the workspace again. See
[`kule rollback`](../cli/rollback.md).

## 9. Let an agent do it

Everything so far works the same when an agent does the work. Install and
sign in to an agent first (see *Install the agents* in [Install](install.md)),
then:

```sh
kule run $T --agent claude-code "Add a 'Later' section to notes.md with one idea for the project."
```

- The agent runs over the **Agent Client Protocol** (ACP), inside the same
  sandbox, with its network through the task's proxy.
- Its answer and its tool calls print as they come.
- Every tool call that changes something becomes a **step**:
  `kule steps $T` lists them.
- Connections the policy would put to a person are refused at once from
  the command line (nobody is there to ask). The [app](first-task-in-the-app.md)
  asks you instead.
- The run is held to the task's [budgets and limits](../concepts/budgets-and-limits.md).

When it is done, review and decide as before:

```sh
kule status $T
kule commit $T          # or: kule rollback $T
```

`--agent` takes `claude-code` (the default), `codex` or `gemini`, or the
name of an agent you added. See [`kule run`](../cli/run.md).

## 10. Clean up

```sh
kule rm $T
```

```text
removed task c224e328
```

`kule rm` deletes the task and its copy. It never touches the workspace.

## What you learned

```text
kule new FOLDER          make a task (a copy)
kule exec TASK -- CMD    run a command in the sandbox
kule run TASK "PROMPT"   let an agent work
kule status TASK         what changed, and the policy's verdict
kule commit TASK         apply it to the folder (keeps a checkpoint)
kule undo TASK           take the last commit back
kule rollback TASK       throw uncommitted changes away
```

Next: the same loop [in the desktop app](first-task-in-the-app.md), or
[how Kule works](../concepts/how-kule-works.md) underneath.
