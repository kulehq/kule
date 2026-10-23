---
title: Home layer
sidebar_label: Home layer
description: How writes a task makes to your home directory (~/.zshrc, ~/.config, git config --global) are caught for review instead of reaching your home.
---

# Home layer

## The problem it solves

Agents and the tools they run often write to your home directory: `git
config --global`, a line added to `~/.zshrc`, a tool's settings under
`~/.config`. Refusing those breaks tools. Letting them through means an
agent can change what runs every time you open a shell.

Kule does neither. The writes are **caught**: they land in the task's
**home layer**, and wait for you to apply or discard them.

## How it works

A task's commands see `HOME` as the task's home layer (`<task>/home`), not
your real home:

- the small files at the top of your home and `~/.config` are **clones**
  there (APFS clones them in one go; elsewhere `.config` is copied while it
  stays under 64 MiB);
- new files at the top of your home land there too;
- every other entry is a **link** to your own, so toolchains, caches and
  settings are found where they always are. A write through a link meets
  the sandbox as before;
- the agent's own state is **cloned** too: the agent writes there, and
  what it writes never reaches your home and is never offered for review;
- hidden paths (`~/.ssh`, `~/.aws`, …) are not in the layer at all;
- shell histories and completion dumps are not offered either.

In the clones of what shells and tools read as they start (`.zshrc`,
`.profile`, `.gitconfig`, …) and of env files (`~/.env`), a setting that
looks like a secret (`export GITHUB_TOKEN=…`) is **masked**: commands see a
placeholder (see [Secrets](secrets.md)), and an apply puts your value back.
One Kule cannot mask (not UTF-8, or larger than 1 MB) is withheld from
the commands, in the layer and at its real path, and Kule says so where
they run; it is never offered as a change.

## Seeing what waits

`kule status` and the app's Review list it:

```text
in your home, caught for review (1; `kule home 1a2b3c4d apply PATH` takes one there):
  ~ ~/.zshrc  (runs every time a shell or tool starts)
```

A file that shells or tools read and run as they start (`.zshrc`,
`.profile`, `.gitconfig`, `~/.config/fish`, `~/.config/autostart`, systemd
user units, …) is marked **runs at every start**. Look at those carefully.

## Deciding

```sh
kule home TASK                 # list what waits
kule home TASK diff ~/.zshrc   # what the change would do to your file, line by line
kule home TASK apply ~/.zshrc  # take this one change to your real home
kule home TASK discard --all   # throw them all away
kule home TASK undo            # take the last apply back
```

- **apply** checks that your file is still as it was when the layer was
  made, and keeps what it replaced in the task's `home-kept/`.
- **undo** takes the last apply back, if you have not changed those paths
  since; the changes wait for review again.
- A home change is **never part of a commit**. You decide on it apart.

In the app: Review → *Writes in your home*, with **Show**, **Apply**,
**Discard** and **Undo**. A file that runs at every start opens its diff
from the first look.

## Between runs

While changes wait, the layer stays as it is from run to run. Once none
wait, the next run starts from your home as it is then.

`kule exec` uses the layer too. A trial of an agent profile
(`kule agents try`) does not, since it learns the agent's directories from
what is refused.

## Related

- [`kule home`](../cli/home.md)
- [Review](../app/review.md)
- [The sandbox](sandbox.md)
