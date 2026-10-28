---
title: Platforms
sidebar_label: Platforms
description: What Kule supports on macOS, Linux and Windows, how the sandbox differs between them, and what is not done yet.
---

# Platforms

Kule runs on **macOS**, **Linux** and **Windows 11**. The sandbox is a
different mechanism on each (Seatbelt, Landlock with namespaces, a process
container), and the same rule on all three: a command reads nothing but
what Kule grants (see [`kule tools`](cli/tools.md)). What a command may
write, where its network goes and what reaches your folder are the same on
all three too.

## Support at a glance

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Versions | macOS 13 or later | x86_64 release files (built on Ubuntu 22.04, tested on Fedora); other architectures from source | Windows 11 24H2 with update 26100.9278 or later, x64 and ARM64 |
| Release files | Universal `.dmg` (Apple silicon and Intel) | `.deb`, `.rpm`, `kule` alone as `.tar.gz` | A per-user installer (`-setup.exe`), `kule` with its two programs as `.zip` |
| Sandbox | Seatbelt | Landlock (Linux 6.2 or later), seccomp, namespaces | A process container (Microsoft's `mxc-sdk`) around each command |
| Network rules in the sandbox | Yes | Linux 6.7 or later | Yes |
| Sandbox refusals listed | Yes, after the command and in `sandbox.jsonl` | No (Landlock reports only to the audit log); the command still gets a permission error | No; the command gets an access error |
| The task's copy | APFS clone: almost no space | Reflinks on Btrfs and XFS; a full copy on ext4 | Block clones on ReFS and a Dev Drive; a full copy on NTFS |
| Package caches at start | npm and cargo cloned from yours | Empty: the first install downloads again | Empty: the first install downloads again |
| `/tmp` | Closed: `TMPDIR` is the task's own | Each task's own | `TEMP` and `TMP` are the task's own |
| The task's home | Your files linked into the task's home, writes caught for review | The same | Empty, the task's own: nothing of your profile is read or layered |
| Processes of its own | – | Yes: a command sees and signals no other program | Ends with the command: nothing it starts outlives it |
| Network namespace of its own | – | Yes, with only the proxy in reach | The container's own: shut, open, or the proxy alone |
| Servers held to this computer | None listen unless the task's `servers` limit allows them (weaker isolation you accept); then Node, Python and Ruby are pointed at loopback and other programs' ports shielded | Every listening address turned into loopback | A server inside is the task's alone: nothing from this computer or the network reaches it |
| GPU off | Metal (and OpenCL) gets no device | `/dev/dri` and NVIDIA's devices read-only | – |
| Secret store key | Keychain | Secret Service | Credential Manager |
| Show in file manager | Finder | Files (D-Bus `FileManager1`) | – |

## Windows: what the sandbox needs

- **Windows 11 24H2 (build 26100) with update 26100.9278 or later**, or a
  later Windows. The process container Kule uses arrived with that update.
  Windows Server is not covered: its container cannot hide a path.
- **A desktop session.** From a service or a remote shell (SSH, a scheduled
  task), the programs a task runs cannot start.

Where either is missing, sandboxed commands are **refused with the reason**,
in the terminal and under *Settings → Policy*. There is no weaker mode to
accept on Windows: the container gives a command all of its isolation or
none, and none is not a weaker sandbox.

What holds inside the container, measured on a standard user's account
with the firewall at its defaults (see
[`docs/windows-measurements.md`](https://github.com/kulehq/kule/blob/main/docs/windows-measurements.md)
in the repository):

- A command reads only what is granted: the task's copy and temporary
  folder (writable), Windows, Program Files, the tools on your `PATH` and
  the folders toolchains install themselves in (`.cargo`, `.rustup`,
  `%LOCALAPPDATA%\Programs`, npm's and pnpm's, …). Your profile, your other
  projects, the Credential Manager, the clipboard and the registry are out
  of reach; a folder on the way to a grant is listed, not read, and what is
  beside the way is not listed.
- A task's commands get a **home of their own**, empty, in the task's
  folders: `USERPROFILE`, `APPDATA`, `TEMP` and `TMP` point there, and
  `HOMEDRIVE` and `HOMEPATH` are not passed. Nothing of your profile is
  read or layered, and `kule home` has nothing to show.
- The network is shut, open, or the proxy's alone, as the task's policy
  says. A task's **proxy runs as a process of its own**, `kule-proxy.exe`,
  in an app container that holds the task's secrets and reaches the
  network and nothing of yours; a connection past the proxy fails at once.
  Kule writes no firewall rule.
- The **terminal** (`kule shell` and the app's terminals) opens the shell
  on a pseudo console inside the task's container, through Kule's helper
  `kule-pty-helper.exe`, kept beside Kule's programs. Kule refuses a
  terminal whose helper lies in a folder a command could write to, in one
  Kule hides from commands (Kule's home, the workspace), or in one that
  holds your profile.
- **Kule's home** (`%USERPROFILE%\.kule`) and the programs' folder
  (`%LOCALAPPDATA%\Kule` for the installer) must stay apart: a command may
  read the folder of the terminal's helper, and Kule's home it never reads.
  With `KULE_HOME` set inside the programs' folder, or the `.zip` unpacked
  into Kule's home, commands are refused with that reason.
- **Windows PowerShell** inside the container runs in its Constrained
  Language mode, which the lockdown policy of every app container brings:
  cmdlets, scripts and the programs they start work, calls into .NET
  (`[System.IO.File]::...`, `Add-Type`) do not. It cannot find its
  application-data folder there, so it writes its command history beside
  wherever the shell is, in the task's copy, where review shows it.
- **`kule exec --reveal PATH` grants a read** on Windows: a path that was
  never hidden is not readable until granted, so revealing it opens it
  read-only.

CI runs the Windows sandbox tests on GitHub's Windows 11 ARM64 runner for
both targets (x64 under Windows's emulation), as an administrator; the
runs as a standard user, and native x64 hardware, are covered on a test
machine before a release.

## Linux: what the sandbox needs

- **Landlock ABI 3 (Linux 6.2)** at least: the first to cover truncation.
  On an older kernel the sandbox cannot be set up, and commands do not run.
- **Landlock ABI 4 (Linux 6.7)** for network rules. On an older kernel, a
  command whose network must go through the proxy is refused rather than
  left open.
- **Unprivileged user namespaces**, for a network, processes, System V IPC
  and a host name of the command's own. Ubuntu 24.04 refuses them by
  default:

  ```sh
  sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0
  ```

  Without them Kule runs no agent until you accept weaker isolation (see
  [`kule isolation`](cli/isolation.md)), and says what is missing while you
  do.

Unix sockets outside the task's folders are out of reach through
Landlock's rule for them (ABI 9). On older kernels, your runtime directory
(`/run/user/<uid>`, where the session bus and Docker's and Podman's
rootless sockets live) is covered with an empty one for the command, and
each other socket that exists when the command starts is covered too.

An agent's settings inside its state directory are mounted read-only for
the agent alone. Without user namespaces, and for settings that are links
or do not exist yet, they are put back after the run instead; whatever a
put-back replaces is moved to `~/.kule/kept/replaced`, never lost.

## Not yet

- **HTTP/2** through the proxy (clients fall back to HTTP/1.1), and
  WebSocket upgrades.
- On **Windows**, what the sandbox refused is not listed after a command;
  the command gets an access error. A task's home holds none of your
  files, so an agent's own settings do not carry in (`kule home` is
  empty). The window's *Show in Explorer* is not there yet. The installer
  and the programs carry no code signature: SmartScreen says the publisher
  is unknown, and [Install](getting-started/install.md) says how to check
  the download first.
- On **macOS**, a task has no `/tmp` of its own: the macOS sandbox cannot
  move a path, so `/tmp` stays closed and tools use `TMPDIR`. A tool that
  writes to `/tmp` by a fixed path is refused, and the refusal says so.
- On **macOS**, the sandbox cannot tell the address a server listens on, so
  a task's servers listen only once you allow them. Then Node, Python and
  Ruby servers are pointed at this computer, and others are kept off the
  network from outside the sandbox.
- On **macOS**, while a task's network goes through the proxy, the system's
  certificate service stays closed to its commands. A tool that checks
  certificates only through it cannot use HTTPS in a task. pip 24.2 and
  later works with `--use-deprecated=legacy-certs` (older pip refuses the
  option, so Kule does not set it). Go is told to use the roots
  `SSL_CERT_FILE` names.
- **NuGet's and Maven's** tools on macOS trust the system's roots alone, so
  their registries pass through unseen, writes included, as Go's module
  proxy does.
- **Gemini** needs an API key from AI Studio, stored in a file:
  `GEMINI_FORCE_FILE_STORAGE=true gemini`, then `/auth`.

## Related

- [Install](getting-started/install.md)
- [The sandbox](concepts/sandbox.md)
- [Troubleshooting](troubleshooting.md)
