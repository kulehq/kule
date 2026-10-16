# Kule

Kule is a control room for coding agents. Agents do the work; people watch,
approve and steer. Every command an agent runs is a transaction in a
copy-on-write copy of the workspace, and what reaches the real workspace is
decided by the effects the command had, not by its text.

**Status:** M0 and M1 done (engine, git, sandbox, policy, proxy). macOS and Linux.

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

## M1, step 2: the sandbox

`kule exec` runs commands in the operating system's sandbox (Seatbelt on
macOS, Landlock on Linux). They may write only to the task's copy, its own
`TMPDIR` and package caches; credentials such as `~/.ssh`, `~/.aws` and
`~/.npmrc` cannot be read; `--no-network` shuts TCP. If the sandbox cannot be
set up, the command does not run. `--allow-write`, `--hide`, `--reveal` and
`--no-sandbox` adjust it. On macOS, what the sandbox refused is printed after
the command and kept in the task's `sandbox.jsonl`:

```
sandbox: refused file-read-data /Users/me/.config/gh/config.yml (gh)
sandbox: refused file-write-create /Users/me/.zshrc (sh)
```

## M1, step 3: policy

Every change is judged by a Cedar policy before it reaches the workspace:
it passes on its own, waits for approval, or is denied.

```
policy: waits for approval
  Cargo.toml · waits for approval (manifests-and-lockfiles)
```

`kule commit` applies what passes, `kule commit --approve` also what waits,
and nothing that is denied (a new git hook, for example). `kule policy`
prints the built-in rules: source edits and fast-forward branch moves pass;
manifests, lockfiles, CI and editor configuration, git config, history
rewrites and mass deletes wait; git hooks are denied. Save your own as
`~/.kule/policy.cedar` to replace them. A repository can add `forbid` rules
in `.kule/policy.cedar`; they are read from the real workspace, never from
the copy an agent edits.

## M1, step 4: the network goes through a proxy

Commands can connect only to the task's proxy; the proxy variables point at
it. Each connection is judged by the same policy (`net.connect`): package
registries and model APIs pass, anything else is refused until the approval
inbox exists. Every connection lands in the task's `network.jsonl`.

```
net: registry.npmjs.org:443 · tunneled (package-registries)
net: example.com:443 ×2 · refused, waits for approval (no-rule-lets-it-pass)
```

`--no-network` shuts TCP; `--open-network` bypasses proxy and policy.

## M1 in practice

A headless Claude Code run inside the sandbox fixed a failing test in a
small Rust repository and committed the fix; the policy let the result pass
on its own and `kule commit` applied it:

```sh
kule exec $TASK --reveal ~/Library/Keychains \
  --allow-write ~/.claude --allow-write ~/.claude.json \
  -- claude -p "Fix the failing test, run cargo test, commit the fix." \
     --allowedTools "Bash,Read,Edit,Write" --permission-prompts none
kule status $TASK   # policy: passes on its own
kule commit $TASK
```

Which paths an agent needs (its login, state, temporary files) is what agent
profiles in M2 will settle; for now they are flags.

## Not yet

- Linux: Landlock reports refusals only to the audit log, so they are not
  listed; the command still gets EPERM. Network rules need Linux 6.7.
- No approval inbox, no agent profiles, no desktop app (M2, M3).

## License

Apache-2.0
