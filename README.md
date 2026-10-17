# Kule

Kule is a control room for AI agents that work on your files: code, but
also notes, documents, photos and data. Agents do the work; people watch,
approve and steer. Every command an agent runs is a transaction in a
copy-on-write copy of the folder it works on (the *workspace*), and what
reaches the real folder is decided by the effects the command had, not by
its text.

**Status:** M0–M3 done: engine, git, sandbox, policy, proxy, agents over ACP
(Claude Code, Codex, Gemini), the desktop app and its daemon, live approvals,
checkpoints on Lonca with a signed history. macOS and Linux.

## Any folder, not only repositories

Kule needs no git. A task is a copy of any folder: the agent changes the
copy, you see what changed, and only a commit applies it to the folder,
keeping a checkpoint that undo puts back. When the folder is a git
repository, Kule also reads `.git` as one (see M1, step 1).

- **Notes and documents** (a Markdown or Obsidian vault, reports): let an
  agent restructure them and review the result before anything moves.
- **Photos, downloads, archives**: "sort these into folders" works on the
  copy, so your originals stay where they are until you commit. Moves are
  shown as moves (`» photos/ → 2026/`); in a folder that is not a git
  repository more than 20 deletions wait for you, and undo brings back
  what a commit replaced.
- **Data**: notebooks, CSV files and large datasets; the copy takes no
  space until files change.
- **Something you don't trust yet**: run a script or an installer in a task
  first, and see what it would have changed.

The review says what changed inside each file, by the names a person uses:
functions, methods, fields and types in Rust, C#, TypeScript, JavaScript,
Python and Go; sections in Markdown; keys in JSON and TOML. Every change in
a file is listed under one of them, or under its top level:

```
  ~ notes.md
      + section Plan › Later
  ~ src/task.rs
      ~ method Task::commit
      + method Task::check
```

Tasks on the same folder never see each other's work, so Kule tells where
they meet while both are still going, not when the second commit is
refused: the same method, section or key, or the same file when it cannot
look inside. `kule status` lists it (`task 5e6f7a8b changes them too:
task.rs · method Task::commit`), the app marks the task and lists it under
Review, and, while its window is not in front, the app also tells through
the system's notifications when two tasks start to meet, when an agent
waits for an answer, and when a run is over.

A changed picture (PNG, JPEG, GIF, WebP) shows what it was and what it is
under Review. Only what its header proves to be such a picture, under
8 MiB and 40 megapixels, reaches the window.

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
- What the task stages must be what its files hold: an entry staged with
  other content (or taken out of the index while its file stays) waits
  for a person, since the next `git commit` would record what nobody
  reviewed.
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
`TMPDIR` and `/tmp`; credentials such as `~/.ssh`, `~/.aws`, `~/.npmrc` and
cargo's registry token cannot be read; `--no-network` shuts TCP. Package
managers (cargo, npm, pip, uv, Go, NuGet, Yarn, pnpm) get caches of the
task's own, so nothing a command leaves in a cache reaches a build you run
later; your caches stay read-only. On macOS the cargo and npm caches start
as copy-on-write clones of yours. If the sandbox cannot be
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
rewrites and mass deletes wait (more than 200, or more than 20 outside a git
repository; a file renamed in the copy is a move, not a deletion); git
hooks are denied. Save your own as
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

## M2: agents over ACP, and the desktop app

`kule run $TASK --agent claude-code "prompt"` starts the agent over the
Agent Client Protocol inside the sandbox. Its profile opens only the agent's
own state and login; other agents' logins and transcripts stay hidden. Every
tool call becomes a step (`kule steps $TASK`).

Claude Code, Codex and Gemini come built in. Any other agent that speaks
ACP is added as a profile in `~/.kule/agents.toml`, in the format `kule
agents show claude-code` prints:

```toml
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode"]          # what it may write, under ~
login = [".local/share/opencode/auth.json"] # hidden from every other agent
pass_env = ["OPENROUTER_API_KEY"]           # of your variables, the ones it sees
hosts = ["openrouter.ai"]                   # HTTPS hosts it reaches, while it runs
```

A profile opens holes in the sandbox, so yours runs only once you approve
it (`kule agents approve opencode`, after `kule agents show opencode`), and
again after any change to it. It may never open the credentials Kule hides,
another agent's state, Kule's home or a whole directory like `~/Documents`,
and a package it fetches must name one exact version. Profiles live in
Kule's home, which no sandbox can see, and never come from a workspace.

`crates/kule-desktop` is a Tauri app around the same engine:

- **Tasks**: pick a folder to make a task; each shows its changes and the
  policy's verdict. Tasks waiting for approval collect under *Needs you*.
- **Live**: the agent's answer, tool calls and what each changed, as they
  come.
- **Review**: the effect set and the ruling, with *Commit*, *Approve and
  commit* or *Roll back*.
- **Timeline**: commits (with undo), uncommitted steps, network connections
  and sandbox refusals.

```sh
cargo run -p kule-desktop   # Linux needs libwebkit2gtk-4.1-dev
```

The app is a window onto the Kule daemon (`kule daemon`), which it starts
when none is running. The daemon owns the tasks, the agents' runs and the
questions they wait on, so closing the window stops nothing, and reopening
it shows what happened meanwhile. It listens on `<home>/kuled.sock`, which
only its owner can open (checked by peer credentials too), runs once per
home, and exits after ten idle minutes.

What a commit replaced is kept in the task's own Lonca store (git objects
under `checkpoints/store.git`, with modes and times beside them), and every
commit and undo is an entry in a signed, hash-chained history (`kule history
TASK`). The signing key lives in Kule's home and is hidden from every
sandbox.

Paths the workspace's `.gitignore` excludes (build output, caches, `.env`)
stay in the copy: a commit leaves them behind unless asked
(`kule commit --include-ignored`, or *Also commit these* in the app). The
rules come from the workspace, never from the copy.

In the app, a connection the policy puts to a person waits for an answer:
it shows under *Needs you* and on the task's Live tab with Approve and
Deny. Unanswered, it is refused when the task's wait runs out (15 minutes
unless set from 1 minute to 8 hours; never off) and the agent is told to
go on without it. The CLI has nobody to ask and refuses such connections
at once.

## Not yet

- Linux: Landlock reports refusals only to the audit log, so they are not
  listed; the command still gets EPERM. Network rules need Linux 6.7.
  Commands get a network namespace of their own, with nothing but the
  proxy in reach; that takes unprivileged user namespaces, which Ubuntu
  24.04 refuses by default (`sysctl
  kernel.apparmor_restrict_unprivileged_userns=0`). Without them only the
  proxy's port is checked, and Kule says so. An agent's settings inside its
  state directory are mounted read-only for the agent alone, so changes the
  person makes meanwhile stay. Without user namespaces, and for settings
  that are links or do not exist yet, they are put back after the run
  instead; whatever a put-back replaces is moved to `~/.kule/kept/replaced`,
  never lost.
- Gemini needs an API key from AI Studio (Google no longer serves the
  Code Assist sign-in to clients like this), saved outside the keychain,
  which the sandbox keeps read-only: `GEMINI_FORCE_FILE_STORAGE=true gemini`,
  then `/auth`. Codex works with a ChatGPT sign-in (`codex login`).
- Windows (v1).

## License

Apache-2.0
