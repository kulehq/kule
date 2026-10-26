---
title: Platforms
sidebar_label: Platforms
description: What Kule supports on macOS and on Linux, how the sandbox differs between them, and what is not done yet.
---

# Platforms

Kule runs on **macOS** and **Linux**. Windows is being ported: Kule
builds and its tests run there, the daemon listens on a named pipe only
the same user can open, and the task's copy, commit, rollback and undo
work on NTFS and ReFS; the sandbox, the proxy process and the terminal
come with later work packages, and until then a sandboxed command,
`kule run`, `kule exec` and `kule shell` are refused on Windows with a
message rather than run unsandboxed. Windows 11 24H2 (build 26100.9278)
or later will be required. See `docs/windows-measurements.md` for what was
measured.

## Support at a glance

| | macOS | Linux |
| --- | --- | --- |
| Versions | macOS 13 or later | x86_64 release files (built on Ubuntu 22.04, tested on Fedora); other architectures from source |
| Release files | Universal `.dmg` (Apple silicon and Intel) | `.deb`, `.rpm`, `kule` alone as `.tar.gz` |
| Sandbox | Seatbelt | Landlock (Linux 6.2 or later), seccomp, namespaces |
| Network rules in the sandbox | Yes | Linux 6.7 or later |
| Sandbox refusals listed | Yes, after the command and in `sandbox.jsonl` | No (Landlock reports only to the audit log); the command still gets a permission error |
| The task's copy | APFS clone: almost no space | Reflinks on Btrfs and XFS; a full copy on ext4 |
| Package caches at start | npm and cargo cloned from yours | Empty: the first install downloads again |
| `/tmp` | Closed: `TMPDIR` is the task's own | Each task's own |
| Processes of its own | – | Yes: a command sees and signals no other program |
| Network namespace of its own | – | Yes, with only the proxy in reach |
| Servers held to this computer | None listen unless the task's `servers` limit allows them (weaker isolation you accept); then Node, Python and Ruby are pointed at loopback and other programs' ports shielded | Every listening address turned into loopback |
| GPU off | Metal (and OpenCL) gets no device | `/dev/dri` and NVIDIA's devices read-only |
| Secret store key | Keychain | Secret Service |
| Show in file manager | Finder | Files (D-Bus `FileManager1`) |

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

- **Windows**.
- **HTTP/2** through the proxy (clients fall back to HTTP/1.1), and
  WebSocket upgrades.
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
