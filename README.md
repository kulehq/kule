# Kule

Kule is a control room for coding agents. Agents do the work; people watch,
approve and steer. Every command an agent runs is a transaction in a
copy-on-write copy of the workspace, and what reaches the real workspace is
decided by the effects the command had, not by its text.

**Status:** M0 (engine spike) done, M1 in progress. macOS and Linux.

## M0: transactions on a copy-on-write copy

```sh
kule new ~/code/app                  # task 1a2b3c4d · copy at ~/.kule/tasks/1a2b3c4d/copy
kule exec 1a2b3c4d -- rm -rf src     # runs in the copy, then lists what changed
kule status 1a2b3c4d                 # uncommitted changes (--json for machines)
kule rollback 1a2b3c4d               # throw them away: the copy matches the workspace again
kule commit 1a2b3c4d                 # or apply them to the workspace, keeping a checkpoint
kule undo 1a2b3c4d                   # revert the newest commit; repeat to walk further back
```

- The copy is made with reflinks (APFS clones on macOS, `FICLONE` on Linux),
  so it takes no extra space until files change. Filesystems without
  reflinks fall back to a plain copy.
- Effects are found by comparing the copy with the workspace, not by
  watching system calls, so the same logic will run on every platform.
- Commit refuses when the workspace changed underneath (a human edited a file
  the agent also changed, or added files to a directory the agent deleted).
- Commit writes files through a rename, so nobody sees half a file.

## M1, step 1: `.git` as a repository, not as files

`git status` rewrites `.git/index`, every ref update appends to the logs,
and git maintenance adds and removes lock files in the background. Kule
judges `.git` by what it means:

- Objects are only ever added; undo leaves them for `git gc`.
- References are checked by value: a human commit on the branch the agent
  moved is a conflict, while `git pack-refs` or switching branches and back
  is not.
- The index is compared by its entries, not its stat data, so `git status`
  in the workspace blocks neither commit nor undo.
- Hooks and `config` changes are reported on their own lines, since they
  make the next `git` command run code.

```
1 file changes: +0 ~1 -0
  ~ crates/lonca-core/src/lib.rs
git: main 8b5ec1d → ad08c52 · fast-forward · 1 commit
git: 6 new object files · index changed
git: config changed (it can make the next git command run code)
git: hook pre-commit changed (it runs on the next matching git command)
```

## Not yet

- **No sandbox.** A command run with `kule exec` can still write outside the
  copy. Sandboxing (Seatbelt, bubblewrap/Landlock) is M1.
- No policy yet: hook and config changes are reported but still committed
  (Cedar policy is M1). No network proxy, no approval inbox, no desktop app.

## License

Apache-2.0
