---
title: Policy
sidebar_label: Policy
description: How Kule's Cedar policy judges every change and connection as pass, ask or deny, the balanced and careful policies, your own policy, and remembered approvals.
---

# Policy

Every change a task makes, and every network connection, is judged by a
**policy** before it can reach your folder or the network. The policy is
written in [Cedar](https://www.cedarpolicy.com), a small language for
permission rules.

## The three verdicts

| Verdict | What happens | Shown as |
| --- | --- | --- |
| **pass** | The change goes in on commit; the connection goes through. | `policy: passes on its own` |
| **ask** | The change waits for your approval; the connection waits for your answer. | `policy: waits for approval` |
| **deny** | Never goes in, even when you approve. | `policy: denied` |

What does not pass is put together by why: a reason in plain words, the
**rule** that stopped it, and the paths under it:

```text
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    package.json
```

A rule that cannot decide, or a policy with an error, refuses.

## The built-in policies

You pick one on the first-run screen, in *Settings → Policy*, or with
[`kule policy`](../cli/policy.md):

### Balanced (the default)

**Pass on their own:**

- changes to source, docs and data (source code, notes, documents, data
  files, pictures and fonts), and to kinds of file your project already
  tracks in git;
- a git branch moved forward (fast-forward) or a new branch;
- connections to package registries over HTTPS (npm, Yarn, crates.io,
  NuGet, PyPI, RubyGems, Maven Central), and each agent's own model API
  while that agent runs.

**Wait for you (ask):**

- other kinds of file, scripts (`.sh`, `.ps1`, `.bat`, …) among them, until
  you answer the kind once for good or the project tracks it;
- whatever the task writes in a hidden file or folder (a path part that
  starts with a dot), of any kind;
- manifests and lockfiles: `package.json`, `package-lock.json`,
  `yarn.lock`, `Cargo.toml`, `Cargo.lock`, `go.mod`, `pyproject.toml`,
  `requirements*.txt`, `*.csproj`, `pom.xml`, `build.gradle` and the rest;
- what runs code on its own or steers an agent: CI (`.github`,
  `.gitlab-ci*`, …), editor settings (`.vscode`, `.idea`), dev containers,
  hook managers (`.husky`, `lefthook.yml`), toolchain and package manager
  settings (`.npmrc`, `rust-toolchain.toml`, `build.rs`), an agent's
  settings, instructions and MCP servers (`.claude`, `CLAUDE.md`,
  `AGENTS.md`, `.mcp.json`, …) and Kule's own `.kule/`;
- a repository inside the folder, and symbolic links;
- git config changes, history rewrites, branch deletions, switching
  `HEAD`, tags and other refs;
- **mass deletes**: more than 200 deletions, or more than 20 in a folder
  that is not a git repository (a renamed file counts as a move, not a
  deletion);
- any other connection, git hosts included.

**Denied:** git hooks, a nested repository's hooks too: they would run on
your next git command, as you.

Names are matched in any folder and however they are cased.

### Careful

Every change to a file or to git waits for you. Only package registries,
and each agent's own API while it runs, are reached without asking.

```sh
kule policy careful     # every change waits
kule policy balanced    # back to the default
kule policy             # print the rules that hold now
```

The full rule list, with each rule's id, is in
[Built-in policy rules](../reference/policy-rules.md).

## Your own policy

Save your own Cedar policy as `~/.kule/policy.cedar` to replace the
built-in one. Kule never overwrites it. Start from what `kule policy`
prints.

## A repository's own rules

A repository can **add refusals** (Cedar `forbid` rules) in
`.kule/policy.cedar`. They are read from the real workspace, never from the
copy an agent edits, and they can only make the policy stricter.

## A group's policy

A [group](groups.md) can judge its tasks by its own policy: the one for
every task (the default), balanced or careful whatever the rest use, or one
you write for the group. See [`kule group policy`](../cli/group.md).

## Approving, and remembering

When something waits for you:

- `kule commit TASK --approve`, or **Approve and commit** in the app, lets
  it in. Denied changes stay out.
- `--remember` (or *Don't ask again in this folder*) stops asking, on this
  folder, about the files approved now: manifests and project files and
  hidden files by path, kinds of file the project had none of by kind.
- **Approve and remember** on a connection question lets that host and port
  through on this folder.

Before an allowance is kept, Kule can show what it would have let through
in your past tasks:

```sh
kule rules allow api.example.com:443        # shows what it would have let through
kule rules allow api.example.com:443 --yes  # keeps it
kule rules                                  # lists what you told Kule
kule rules forget ID                        # asks again
```

A remembered connection becomes a Cedar `permit`; a remembered file or
kind, a narrow exception to the rule that asked about it. **Never
remembered:** what is denied, what asks for reasons other than a file's
name, place or kind (a mass delete, a history rewrite), what runs code on
its own (CI, editor and agent settings, asked every time), and anything
under `.kule/`.

## Related

- [`kule policy`](../cli/policy.md), [`kule rules`](../cli/rules.md), [`kule commit`](../cli/commit.md)
- [Built-in policy rules](../reference/policy-rules.md)
- [Network and questions](network-and-questions.md)
- [Settings](../app/settings.md)
