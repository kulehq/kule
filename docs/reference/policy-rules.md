---
title: Policy rules
sidebar_label: Policy rules
description: Every rule in Kule's balanced and careful policies, the names you see in verdicts, and how to write your own.
---

# Policy rules

Every change and connection a task makes is judged by a
[Cedar](https://www.cedarpolicy.com/) **policy** before it reaches your folder.
This page explains every rule in Kule's two built-in policies, the rule
names you see in `kule status`, `kule commit` and the network log, and how to
write rules of your own. For the idea behind it, see
[Policy](../concepts/policy.md).

## How a verdict is reached

Cedar only answers *allow* or *deny*, so Kule asks it two questions about
each thing a task does (each **effect**):

1. **May it pass on its own?** (action `Kule::Action::"auto"`)
2. If not, **may it be put to a person?** (action `Kule::Action::"ask"`)

The first *yes* decides. Two *no*s deny.

| Verdict | Shown as | Means |
| --- | --- | --- |
| Allow | `passes on its own` | `kule commit` applies it. A connection is let through. |
| Ask | `waits for approval` | It needs `kule commit --approve`, or *Approve and commit* in the app. A connection waits for your answer in the app; from the command line, nobody can be asked, so it is refused. |
| Deny | `denied` | It never passes. |

A whole set of changes gets the strictest verdict of its effects. In Cedar a
`forbid` always wins over a `permit`, and an error while evaluating denies.

The rule names in brackets are each rule's `@id`:

```text
policy: waits for approval
  Cargo.toml · waits for approval (manifests-and-lockfiles)
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
```

## Kule's balanced policy

This is the default. Print it with `kule policy` while it holds.

| Rule | Kind | Matches | Effect |
| --- | --- | --- | --- |
| `ask-by-default` | permit `ask` | Everything. | Anything may be put to a person unless a `forbid` says otherwise. |
| `files` | permit `auto` | Every file change (`fs.*`). | Ordinary file changes **pass**. |
| `manifests-and-lockfiles` | forbid `auto` | Files that decide what gets installed or built, by name, in any folder, any case: `package.json`, `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock(b)`, `Cargo.toml`, `Cargo.lock`, `Directory.Packages.props`, `Directory.Build.props`/`.targets`, `nuget.config`, `packages.lock.json`, `global.json`, `go.mod`, `go.sum`, `pyproject.toml`, `setup.py`, `setup.cfg`, `poetry.lock`, `uv.lock`, `Pipfile`(`.lock`), `Gemfile`(`.lock`), Gradle build and settings files, `pom.xml`. | **Waits.** Can be remembered. |
| `project-files` | forbid `auto` | `*.csproj`, `*.fsproj`, `requirements*.txt`. | **Waits.** Can be remembered. |
| `automation` | forbid `auto` | What runs code or steers an agent without anyone running the project. Anything inside `.github`, `.circleci`, `.devcontainer`, `.vscode`, `.idea`, `.zed`, `.kule`, `.husky`, `.claude`, `.cursor`, `.gemini`, `.codex`, `.cargo`, `.mvn`, `.yarn`, `.buildkite`, `.gitea`, `.forgejo`, `.woodpecker`, `.windsurf`, `.continue`, `.roo`, `.clinerules`, or `gradle/wrapper/`; and files like `.gitlab-ci*`, `.envrc`, `.gitattributes`, `.gitmodules`, `.npmrc`, `.yarnrc(.yml)`, `.pnpmfile.cjs`, `build.rs`, `gradlew`, `mise.toml`, `.tool-versions`, `.pre-commit-config.yaml`, `lefthook.yml`, `.mcp.json`, `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `rust-toolchain(.toml)`, `mvnw`, `bunfig.toml`, `dotnet-tools.json`, `azure-pipelines.yml`, `.travis.yml`, `Jenkinsfile`, `bitbucket-pipelines.yml`, `opencode.json`, `.aider.conf.yml`. | **Waits.** Can be remembered (except under `.kule/`). |
| `nested-repository` | forbid `auto` | Files inside a `.git` folder of a repository nested in the workspace. | **Waits.** |
| `nested-git-hooks` | forbid all | A hook created or changed in a nested repository. | **Denied.** |
| `links` | forbid `auto` | A symbolic link created, changed or deleted. | **Waits.** A link leads wherever it points. |
| `mass-delete` | forbid `auto` | More than 200 deletions in one set of changes. A move is not a deletion. | **Waits.** |
| `deletes-outside-git` | forbid `auto` | More than 20 deletions in a folder that is not a git repository. | **Waits.** Only the task's own undo would bring them back. |
| `branch-forward` | permit `auto` | A branch (`refs/heads/*`) moved forward (fast-forward). | **Passes.** |
| `branch-created` | permit `auto` | A new branch. | **Passes.** A new tag, note or replacement waits. |
| `package-registries` | permit `auto` | HTTPS (port 443) to `registry.npmjs.org`, `registry.yarnpkg.com`, `crates.io`, `index.crates.io`, `static.crates.io`, `static.rust-lang.org`, `api.nuget.org`, `globalcdn.nuget.org`, `pypi.org`, `files.pythonhosted.org`, `rubygems.org`, `repo.maven.apache.org`, `repo1.maven.org`. | **Passes.** |
| `git-hooks` | forbid all | A git hook created or changed. Hooks run on your next git command, as you. | **Denied.** |

Not matched by any `auto` permit, and so **waiting** (`no-rule-lets-it-pass`):
git config changes, staged entries that differ from the files, history
rewrites, deleted branches, a switched `HEAD`, other refs (tags, notes,
replacements), and every host not listed above, git hosts included (inside
TLS a push cannot be told apart from a fetch).

## Kule's careful policy

`kule policy careful` writes the balanced policy with one more rule at the
end:

| Rule | Kind | Matches | Effect |
| --- | --- | --- | --- |
| `careful` | forbid `auto` | Every file change and every git change. | **Every change waits for you.** Only the connections above (and each agent's own hosts) pass on their own. |

Under careful, a remembered file does not pass: `careful` asks about every
file, and it cannot be answered for good.

## Names Kule adds

These do not appear in the policy text; Kule adds them as it judges.

| Name | Where | Means |
| --- | --- | --- |
| `no-rule-lets-it-pass` | waits | No `auto` permit matched. |
| `no-rule-allows-asking` | denied | No `ask` permit matched, so it cannot even be put to you. Happens with a policy of your own that leaves out `ask-by-default`. |
| `policy-error: …` | denied | Cedar could not evaluate a rule; it denies. |
| `remembered` | passes | A file you told Kule not to ask about again ([`kule rules`](../cli/rules.md)). |
| `remembered-<id>` | passes | A connection you told Kule not to ask about again. |
| `profile-<agent>` | passes | A host in the agent's profile, or its model's API, open **only while that agent runs** (for example `profile-claude-code` for `api.anthropic.com`). |

## What a rule can look at

Each effect is a `Kule::Effect` entity with these attributes:

| Attribute | Value |
| --- | --- |
| `kind` | `fs.create`, `fs.modify`, `fs.delete`, `git.ref`, `git.config`, `git.hook`, `git.staged`, `git.other`, `net.connect` |
| `path`, `name` | The workspace path and its last part (`""` for `git.ref`). |
| `folded_path`, `folded_name` | The same in lower case. Match these: macOS disks ignore case by default, and so do the tools reading them. |
| `parts` | The folded path's components, to match a folder anywhere (`resource.parts.contains("migrations")`). |
| `entry` | `file`, `dir` or `symlink` for `fs.*`; `""` otherwise. |
| `change` | `created`, `modified` or `deleted`. |
| `ref` | For `git.ref`: the reference, like `refs/heads/main`. |
| `fast_forward` | For `git.ref`: the new value keeps the old one in its history. |
| `commits` | For `git.ref`: how many commits were added. |
| `host`, `port` | For `net.connect`. |

And the `context`:

| Context | Value |
| --- | --- |
| `deletes` | Paths deleted in this set of changes (a move is not one). |
| `files` | Paths changed. |
| `repository` | Whether the workspace is a git repository. |

## Your own policy, for every task

Save a policy as `policy.cedar` in Kule's home (`~/.kule/policy.cedar`). It
**replaces** Kule's for every task (a group with a policy of its own keeps
its own). Since a `forbid` can never be overridden, replacing is the only way
to loosen Kule's rules, so start from Kule's text and change only what you
mean to:

```sh
kule policy > ~/my-policy.cedar      # while balanced holds
# edit it, then:
cp ~/my-policy.cedar ~/.kule/policy.cedar
kule policy | tail -5                # Kule now prints yours
```

Example: let a company package mirror through without asking. Add to the
end of the copy:

```cedar
// Our own: the company's package mirror passes on its own.
@id("company-mirror")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && resource.port == 443 && resource.host == "npm.example.com" };
```

A connection there then shows as:

```text
net: npm.example.com:443 · tunneled (company-mirror)
```

- Kule never overwrites your file: `kule policy balanced` and `careful`
  refuse while it is there. Delete or move it to go back.
- A file that does not parse stops Kule from judging, so commands, runs and
  commits are refused (`kule: policy: unexpected end of input`) until it is
  fixed.
- Keep `ask-by-default`, or nothing can be put to you and every held change
  is denied.

## A repository's own rules

A repository can tighten the policy for everyone who works on it with
`.kule/policy.cedar` at its root. It may contain **only `forbid` rules**:

```cedar
// Changes to database migrations never pass, not even with approval.
@id("no-migrations")
forbid (principal, action, resource)
when { resource.kind like "fs.*" && resource.parts.contains("migrations") };
```

Then:

```console
$ kule status 1046fb30
4 file changes: +4 ~0 -0
  + db/
  + db/migrations/
  + db/migrations/001.sql
  + notes.md
policy: denied
  db/migrations · denied (no-migrations)
  db/migrations/001.sql · denied (no-migrations)
$ kule commit 1046fb30 --approve
...
kule: not committed; the policy denies the changes above
```

`forbid (principal, action, resource)` covers both questions, so the change
is **denied**. Write `action == Kule::Action::"auto"` instead to make it
**wait** for approval.

- Kule reads the file from your **real folder**, never from a task's copy,
  so an agent cannot weaken it. Changes to it in a copy wait for you
  (`automation`), and cannot be remembered.
- A `permit` in it is refused: `.kule/policy.cedar may only contain forbid
  rules: a repository can tighten the policy, never loosen it`.
- The repository's forbids hold under every policy: balanced, careful, yours,
  and every group's.

## A group's own policy

`kule group policy ID own --file PATH` gives one group a policy of its own,
checked before it is kept. The same rules apply as for `policy.cedar`. See
[`kule group`](../cli/group.md).

## See also

- [`kule policy`](../cli/policy.md), [`kule rules`](../cli/rules.md), [`kule commit`](../cli/commit.md)
- [Policy](../concepts/policy.md)
- [Cedar's documentation](https://docs.cedarpolicy.com/)
