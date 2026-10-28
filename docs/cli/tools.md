---
title: kule tools
sidebar_label: kule tools
description: What every task's commands may read beyond their own folders, and how to let them read a folder of tools Kule does not know.
---

# kule tools

A task's commands **read nothing but what is granted**: the task's own
folders, the system, your toolchains, and the folders you add here. This
holds on macOS, Linux and Windows alike. `kule tools` shows the list, with
where each root comes from, and lets you add a folder of tools Kule does not
know, **read-only**, for every task.

## Synopsis

```sh
kule tools [OPTIONS] [COMMAND]
```

| Subcommand | What it does |
| --- | --- |
| [`list`](#kule-tools-list) | Every root a task's commands may read, with its source, and what was named but refused (the default). |
| [`add`](#kule-tools-add) | Let every task read a folder. |
| [`remove`](#kule-tools-remove) | Take it back. |

## Description

The [sandbox](../concepts/sandbox.md) grants reads, it does not assume them.
Where the roots come from:

| Source | What it is |
| --- | --- |
| `task` | The task's copy, its temporary folder and its home layer: read and write. |
| `system` | The operating system's own programs, libraries and settings: `/usr`, `/bin`, `/System/Library`, `/Library/Developer`, Homebrew, Xcode, and `/etc` whole but for a few credential files denied by name (`pip.conf`, `docker/config.json`, `rclone.conf`, `samba/smb.conf`, `environment`, `profile.d`). On Linux also `/proc`, `/sys`, the resolver's files under `/run`, and narrow places under `/var` and `/tmp`; never `/opt`, `/tmp` or `/run` whole. |
| `env` | Where the variables passed to commands say your tools are: each `PATH` entry (and the tool's own folder above a `bin` outside your home, `/opt/gradle/bin` → `/opt/gradle`), `JAVA_HOME`, `GOROOT`, `DOTNET_ROOT`, the others Kule passes on. |
| `tool` | Where toolchains keep their own files in your home by default, read without any variable naming them: `~/.cargo`, `~/.rustup`, `~/.npm`, `~/.m2`, `~/.gradle`, `~/.nuget` (not NuGet's settings in it, which can hold feed credentials: NuGet makes its own), `~/.dotnet`, `~/go`, `~/.cache`, `~/.local/bin`, uv's and pipx's folders under `~/.local/share`, `~/Library/Java/JavaVirtualMachines`, and the settings files tools read at the top of your home (`.gitconfig`, `.editorconfig`, `.tool-versions`, …). |
| `person` | What you added with `kule tools add`. |
| `admin` | What the administrator's file names (see below). |
| `reveal` | A project beside a workspace, for that folder's tasks ([`kule reveal`](reveal.md)). |
| `--reveal` | A path for one command ([`kule exec --reveal`](exec.md)). |

Inside a granted root, credentials stay denied by name: `~/.cargo/credentials.toml`
inside `~/.cargo`, `~/.m2/settings.xml` inside `~/.m2`.

Every source goes through **one validator**, which refuses a path that would
weaken Kule, whoever names it: your home itself or a folder above it, another
person's home, Kule's home (other tasks' copies, the policy, the keys), a
credential folder, one of your own folders (Documents, the projects beside a
workspace), where apps keep settings and data (`~/.config`,
`~/Library/Application Support`), the root of a disk or volume, a first-level
system folder (`/opt`, `/usr`), and what a task writes. A folder *inside* one of
your own folders or stores can be added (`~/Documents/sdk`); the folder itself
cannot. The decision is made on the resolved path, and made again before every
run. Nothing comes from the workspace or a task's copy: an agent cannot widen
its own sandbox.

### When a tool needs a folder Kule does not know

The command sees the refusal as an error (`Operation not permitted`,
`Permission denied`), and the run lists it:

```console
$ kule exec 1046fb30 -- ./build.sh
sandbox: refused file-read-data /opt/corp/sdk/lib/tool.jar (java)
  A task's commands read only what is granted (kule tools list). If a tool
  needs this, `kule tools add '/opt/corp/sdk/lib'` lets every task read that
  folder, never write it; Kule refuses a folder that would weaken it (your
  home, a credential folder, Kule's own).
```

The suggestion comes from Kule's own record of the refusal (the system log on
macOS, `strace` on Linux with [`kule exec --trace`](exec.md)), never from what
the command printed. Add the folder, as narrowly as you can, and run again:

```console
$ kule tools add /opt/corp/sdk
kept as 1: every task reads /opt/corp/sdk, never writes it
```

Linux does not report refusals on its own: run the command once with
`kule exec --trace` to list them.

### The administrator's file

An administrator can grant folders to every task on a computer in a TOML file:

| System | File |
| --- | --- |
| macOS | `/Library/Application Support/Kule/tools.toml` |
| Linux | `/etc/kule/tools.toml` |
| Windows | `%ProgramData%\Kule\tools.toml` |

```toml
paths = ["/opt/corp/sdk", "/srv/toolchains"]
```

The file is read only while **root (an administrator on Windows) owns it and
its folder, and no one else can write either**. Otherwise it is ignored, and
`kule tools list` says so: a file anyone could edit would let anyone widen
every task. Its paths go through the same validator as yours.

## kule tools list

```console
$ kule tools
no folder added: kule tools add PATH lets every task read one

every task's commands read, and never write:
  system  /Library/Developer
  system  /System/Library
  system  /bin
  system  /opt/homebrew
  system  /private/etc
  system  /usr
  env     /Users/me/.cargo/bin
  env     /opt/homebrew
  tool    /Users/me/.cargo
  tool    /Users/me/.gitconfig
  tool    /Users/me/.rustup
  system  /etc whole, but for 7 names (pip.conf, docker/config.json, …)
named but refused:
  admin   /opt: a first-level system folder; name a folder inside it
the task's own folders, and what `kule reveal` opens, besides
```

## kule tools add

```sh
kule tools add <PATH>
```

```console
$ kule tools add /opt/corp/sdk
kept as 1: every task reads /opt/corp/sdk, never writes it
$ kule tools add ~
kule: /Users/me: your home, or a folder above it
$ kule tools add ~/.ssh
kule: /Users/me/.ssh: a path Kule keeps from every command
$ kule tools add /opt
kule: /opt: a first-level system folder; name a folder inside it
```

## kule tools remove

```console
$ kule tools remove 1
removed 1: not readable from the next command on
```

## See also

- [The sandbox](../concepts/sandbox.md): what a command may read and write.
- [`kule reveal`](reveal.md): a project beside a workspace, for one folder.
- [`kule exec`](exec.md): `--reveal` for one command, `--trace` on Linux.
