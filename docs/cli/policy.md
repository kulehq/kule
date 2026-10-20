---
title: kule policy
sidebar_label: kule policy
description: Print the policy that holds for every task, or switch between Kule's balanced and careful policies.
---

# kule policy

Print the policy that holds for every task, or switch between Kule's two
built-in policies.

## Synopsis

```sh
kule policy [OPTIONS] [CHOICE]
```

## Description

The **policy** is a set of [Cedar](https://www.cedarpolicy.com/) rules that
judges everything a task does before it reaches your folder: each file
change, each git change, and each network connection. Every judgement is one
of three verdicts:

- **passes on its own**: `kule commit` applies it without asking;
- **waits for approval**: it needs `kule commit --approve`, or *Approve and
  commit* in the app. A connection that would wait is put to you in the app
  and refused at once from the command line;
- **denied**: it never passes, even with approval.

Kule has two policies of its own:

| Policy | What passes on its own | What waits |
| --- | --- | --- |
| `balanced` (the default) | Ordinary file changes, fast-forward branch moves and new branches, package registries over HTTPS. | Manifests and lockfiles, CI and editor settings, what runs code or steers an agent, links, nested repositories, git config, history rewrites, mass deletes. Git hooks are denied. |
| `careful` | Only the connections `balanced` lets through (package registries, and each agent's own hosts while it runs). | Every change to a file or to git. |

`kule policy` without a choice prints the rules that hold for every task.
`kule policy balanced` and `kule policy careful` put one of them in place. A
task in a [group](group.md) with a policy of its own is judged by the group's
instead.

### Your own policy

To write your own, save it as `policy.cedar` in Kule's home
(`~/.kule/policy.cedar`). It **replaces** Kule's policy for every task, and
Kule never overwrites it: `kule policy balanced` and `careful` refuse while it
is there. Move the file away to go back. See
[Policy rules](../reference/policy-rules.md) for every built-in rule and a
worked example.

### A repository's rules

A repository can add **`forbid` rules** in `.kule/policy.cedar` at its root.
They only tighten the policy, never loosen it, and they are read from your
real folder, never from the copy an agent edits.

## Arguments

| Argument | Meaning |
| --- | --- |
| `CHOICE` | `balanced` or `careful`. Leave it out to print the policy that holds. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--home` | path | `~/.kule` (or `KULE_HOME`) | Kule's home. See [global options](index.md). |
| `-h`, `--help` | | | Print help. |

## Examples

Print the policy that holds (the first lines of the balanced one):

```console
$ kule policy | head -3
// Kule's built-in policy. Each effect is asked twice: may it pass on its own
// (Action "auto"), and if not, may a person approve it (Action "ask").
// A forbid always wins over a permit.
```

Make every change wait for you:

```console
$ kule policy careful
policy: careful
```

Go back to the default:

```console
$ kule policy balanced
policy: balanced
```

See what a task's changes get under the policy:

```console
$ kule status 1046fb30
1 file changes: +0 ~1 -0
  ~ Cargo.toml
policy: waits for approval
  Cargo.toml · waits for approval (manifests-and-lockfiles)
```

The word in brackets is the rule's `@id`; [Policy rules](../reference/policy-rules.md)
explains each.

## Output

- `kule policy` prints the Cedar text itself: Kule's balanced policy,
  Kule's careful one (the balanced text with a `careful` rule added at the
  end), or your own file.
- `policy: careful` / `policy: balanced` confirms a switch.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: strict: the policies to choose are balanced and careful` | Use one of the two. For anything else, write your own `policy.cedar`. |
| `kule: ~/.kule/policy.cedar is a policy of your own; Kule does not replace it` | Move or delete your file first if you want Kule's policy back. |
| `kule: policy: unexpected end of input` (or another Cedar error) from `exec`, `status`, `commit` or `run` | Your `policy.cedar`, a group's policy or the repository's `.kule/policy.cedar` does not parse. Kule refuses to judge with a broken policy, so nothing runs or commits until it is fixed. |
| `kule: .kule/policy.cedar may only contain forbid rules: a repository can tighten the policy, never loosen it` | Remove the `permit` rules from the repository's file. |

## In the app

*Settings → Policy* chooses balanced or careful for every task and a policy
for each group. The first-run screen asks for the same choice.

## See also

- [Policy](../concepts/policy.md): how verdicts work.
- [Policy rules](../reference/policy-rules.md): every built-in rule.
- [`kule rules`](rules.md): what you told Kule not to ask about again.
- [`kule group`](group.md): a group's policy.
- [`kule commit`](commit.md): `--approve` and `--remember`.
