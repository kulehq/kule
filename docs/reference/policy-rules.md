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

What waits or is denied is shown put together by why. Each group starts
with the reason in plain words, then the rule names in brackets (each
rule's `@id`), then the verdict; the paths it holds are under it. Files that
wait only for their kind are one group per kind:

```text
policy: waits for approval
  decides what gets installed (manifests-and-lockfiles) · waits for approval
    Cargo.toml
  a kind of file this project has none of: sh (unfamiliar-kinds) · waits for approval
    scripts/release.sh
    scripts/setup.sh
net: example.com:443 · refused, would wait for approval; the command line asks no one (no-rule-lets-it-pass)
```

A rule with no reason of its own, such as one in a policy you wrote, is
shown as `a rule of the policy`.

## Kule's balanced policy

This is the default. Print it with `kule policy` while it holds.

| Rule | Kind | Matches | Effect |
| --- | --- | --- | --- |
| `ask-by-default` | permit `ask` | Everything. | Anything may be put to a person unless a `forbid` says otherwise. |
| `files` | permit `auto` | Every file change (`fs.*`). | File changes **pass** unless a rule below holds them. With the next two rules, only files of a known kind outside hidden paths pass. |
| `unfamiliar-kinds` | forbid `auto` | A file (never a deletion or a folder) outside hidden paths, which `dot-paths` holds, whose kind is neither on the list below nor one the project already has. Its kind is its extension in lower case, or its whole name when it has none. Known kinds: source (`rs go py rb java kt kts scala swift m mm c h cc cpp cxx hpp hh cs fs fsx vb ts tsx js jsx mjs cjs vue svelte php pl pm lua r jl dart ex exs erl hrl hs elm clj cljs cljc ml mli nim zig sol sql graphql gql proto css scss sass less html htm xml svg`), docs (`md mdx markdown rst txt adoc org tex`), data (`json jsonc json5 yaml yml toml csv tsv`), pictures (`png jpg jpeg gif webp avif ico bmp pdf`), fonts (`woff woff2 ttf otf`), and the names `README`, `LICENSE`, `LICENCE`, `COPYING`, `NOTICE`, `AUTHORS`, `CONTRIBUTORS`, `CHANGELOG`. A kind the project already has is one your folder's git index holds outside hidden paths; Kule reads it from your folder, never from the task's copy. Outside git only the list counts. Scripts (`sh`, `ps1`, `bat`, …) are left out on purpose. | **Waits.** Can be remembered, by kind. |
| `dot-paths` | forbid `auto` | A file (never a deletion or a folder) under a path where any part starts with a dot (`.env`, `.config/tool.json`, `src/.eslintrc`), whatever its kind, tracked or not. Tools look for their settings there. | **Waits.** Can be remembered, by path. |
| `manifests-and-lockfiles` | forbid `auto` | Files that decide what gets installed or built, by name, in any folder, any case: `package.json`, `package-lock.json`, `npm-shrinkwrap.json`, `yarn.lock`, `pnpm-lock.yaml`, `bun.lock(b)`, `Cargo.toml`, `Cargo.lock`, `Directory.Packages.props`, `Directory.Build.props`/`.targets`, `nuget.config`, `packages.lock.json`, `global.json`, `go.mod`, `go.sum`, `pyproject.toml`, `setup.py`, `setup.cfg`, `poetry.lock`, `uv.lock`, `Pipfile`(`.lock`), `Gemfile`(`.lock`), Gradle build and settings files, `pom.xml`. | **Waits.** Can be remembered. |
| `project-files` | forbid `auto` | `*.csproj`, `*.fsproj`, `requirements*.txt`. | **Waits.** Can be remembered. |
| `automation` | forbid `auto` | What runs code or steers an agent without anyone running the project. Anything inside `.github`, `.circleci`, `.devcontainer`, `.vscode`, `.idea`, `.zed`, `.kule`, `.husky`, `.claude`, `.cursor`, `.gemini`, `.codex`, `.cargo`, `.mvn`, `.yarn`, `.buildkite`, `.gitea`, `.forgejo`, `.woodpecker`, `.windsurf`, `.continue`, `.roo`, `.clinerules`, or `gradle/wrapper/`; and files like `.gitlab-ci*`, `.envrc`, `.gitattributes`, `.gitmodules`, `.npmrc`, `.yarnrc(.yml)`, `.pnpmfile.cjs`, `build.rs`, `gradlew`, `mise.toml`, `.tool-versions`, `.pre-commit-config.yaml`, `lefthook.yml`, `.mcp.json`, `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `rust-toolchain(.toml)`, `mvnw`, `bunfig.toml`, `dotnet-tools.json`, `azure-pipelines.yml`, `.travis.yml`, `Jenkinsfile`, `bitbucket-pipelines.yml`, `opencode.json`, `.aider.conf.yml`; the version pins a shell prompt reads on entering a folder (`.nvmrc`, `.node-version`, `.python-version`, `.ruby-version`, `.java-version`, `.go-version`, `.sdkmanrc`, `.terraform-version`, `mise.local.toml`, `.rtx.toml`, mise's `config.toml`, anything in `.mise`) and `*.code-workspace`. Kule's terminal and editor refuse a copy where one of these changed. | **Waits.** Cannot be remembered: what runs code on its own is asked every time. |
| `other-vcs` | forbid `auto` | Another version control system's repository, at any depth: anything in `.hg`, `.sl`, `.jj`, `_darcs`, `.pijul`, `.bzr`, `.svn`, `.fossil-settings`, and `.fslckout` or `_FOSSIL_`. Its config and hooks run on its next command, and Kule reads none of it as a repository. Kule's terminal and editor refuse a copy where one changed. | **Waits.** Cannot be remembered. |
| `nested-repository` | forbid `auto` | Files inside a `.git` folder of a repository nested in the workspace. | **Waits.** |
| `nested-git-hooks` | forbid all | A hook created or changed in a nested repository. | **Denied.** |
| `permissions` | forbid `auto` | A file made executable (any `x` bit it did not have in the folder, or a new one that has one), or a file or folder made writable by anyone. A file the folder already ran keeps passing. | **Waits.** Cannot be remembered. |
| `extended-attributes` | forbid `auto` | On macOS, a file whose extended attributes differ from the folder's file (a quarantine mark added or removed, a `user.*` attribute): a commit carries them into the folder. What macOS sets on its own on every write (`com.apple.provenance`) is left out. | **Waits.** Cannot be remembered. |
| `links` | forbid `auto` | A symbolic link created, changed or deleted. | **Waits.** A link leads wherever it points. |
| `mass-delete` | forbid `auto` | More than 200 deletions in one set of changes. A move is not a deletion. | **Waits.** |
| `deletes-outside-git` | forbid `auto` | More than 20 deletions in a folder that is not a git repository. | **Waits.** Only the task's own undo would bring them back. |
| `branch-forward` | permit `auto` | A branch (`refs/heads/*`) moved forward (fast-forward). | **Passes.** |
| `branch-created` | permit `auto` | A new branch. | **Passes.** A new tag, note or replacement waits. |
| `package-registries` | permit `auto` | HTTPS (port 443) to `registry.npmjs.org`, `registry.yarnpkg.com`, `crates.io`, `index.crates.io`, `static.crates.io`, `static.rust-lang.org`, `api.nuget.org`, `globalcdn.nuget.org`, `pypi.org`, `files.pythonhosted.org`, `rubygems.org`, `repo.maven.apache.org`, `repo1.maven.org`. | **Passes.** |
| `git-hooks` | forbid all | A git hook created or changed. Hooks run on your next git command, as you. | **Denied.** |

Not matched by any `auto` permit, and so **waiting** (`no-rule-lets-it-pass`):
git config changes (anything in `.git` that git does not keep as data:
objects, refs, the index, logs), staged entries that differ from the files, history
rewrites, deleted branches, a switched `HEAD`, other refs (tags, notes,
replacements), and every host not listed above, git hosts included (inside
TLS a push cannot be told apart from a fetch).

## Kule's careful policy

`kule policy careful` keeps that choice in `~/.kule/policy-choice`. Kule
then judges by the balanced policy with one more rule at the end:

| Rule | Kind | Matches | Effect |
| --- | --- | --- | --- |
| `careful` | forbid `auto` | Every file change and every git change. | **Every change waits for you.** Only the connections above (and each agent's own hosts) pass on their own. |

Under careful, a remembered file does not pass: `careful` asks about every
file, and it cannot be answered for good.

The choice is kept, never the text, so careful always means this version's
rules. A `~/.kule/policy.cedar` that holds a balanced or careful text an
earlier version wrote (0.2.0-preview.1 to 0.2.0-preview.11) counts as that
choice, not as a policy of your own: it is judged by today's rules, and
`kule policy balanced` or `careful` replaces it.

## Names Kule adds

These do not appear in the policy text; Kule adds them as it judges.

| Name | Where | Means |
| --- | --- | --- |
| `no-rule-lets-it-pass` | waits | No `auto` permit matched. |
| `no-rule-allows-asking` | denied | No `ask` permit matched, so it cannot even be put to you. Happens with a policy of your own that leaves out `ask-by-default`. |
| `policy-error: …` | denied | Cedar could not evaluate a rule; it denies. |
| `remembered` | passes | A file, or a kind of file, you told Kule not to ask about again ([`kule rules`](../cli/rules.md)). |
| `repo:<id>` | as the rule says | A rule from the repository's own `.kule/policy.cedar`. No answer you remembered cancels it, whatever its `@id`. |
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
| `gains` | For `fs.*`: what the entry gains in its permission bits that the folder's had not, `executable` and `world-writable` (`!resource.gains.isEmpty()`). |
| `xattrs_changed` | For `fs.*`: the file's extended attributes differ from the folder's (macOS). |
| `file_kind` | For `fs.*`: the name's extension in lower case (`vue`), or the whole name where it has none (`makefile`). |
| `kind_tracked` | For `fs.*`: the folder's git index holds a file of this kind outside hidden paths. Read from your folder, never the task's copy. |
| `dotted` | For `fs.*`: a part of the path starts with a dot, or the path lies in a repository's own files. |

And the `context`:

| Context | Value |
| --- | --- |
| `deletes` | Paths deleted in this set of changes (a move is not one). |
| `files` | Paths changed. |
| `repository` | Whether the workspace is a git repository. |

## Your own policy, for every task

Save a policy as `policy.cedar` in Kule's home (`~/.kule/policy.cedar`).
Kule writes nothing there itself: that file is only ever a policy of your
own. It **replaces** Kule's for every task (a group with a policy of its own keeps
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
  the repository's own rule (repo:no-migrations) · denied
    db/migrations
    db/migrations/001.sql
$ kule commit 1046fb30 --approve
...
kule: not committed; the policy denies the changes above
```

`forbid (principal, action, resource)` covers both questions, so the change
is **denied**. Write `action == Kule::Action::"auto"` instead to make it
**wait** for approval.

- Kule reads the file from your **real folder**, never from a task's copy,
  so an agent cannot weaken it. Changes to it in a copy wait for you
  (`automation`, `dot-paths`), and cannot be remembered.
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
