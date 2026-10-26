---
title: Troubleshooting
sidebar_label: Troubleshooting
description: Common problems with Kule, what causes them and how to fix them - installing, agents, the sandbox on Linux, certificates, disk space, commits and the daemon.
---

# Troubleshooting

Each entry has the **symptom**, the **cause** and the **fix**.

## Installing and starting

### macOS refuses to open Kule

**Symptom:** "Kule can't be opened because Apple cannot check it for
malicious software", or the app does not start after you download it.

**Cause:** Kule's macOS app is signed ad hoc, not notarized by Apple.

**Fix:** first check the download (`SHA256SUMS` and `gh attestation
verify`, see [Install](getting-started/install.md)). Then either open
**System Settings → Privacy & Security** and choose **Open Anyway** for
Kule, or run once:

```sh
xattr -dr com.apple.quarantine /Applications/Kule.app
```

### macOS asks for access to "Kule secrets" in the keychain

**Symptom:** a keychain dialog asks to let Kule use the item **Kule
secrets**.

**Cause:** Kule seals your [secrets](concepts/secrets.md) with a key the
keychain keeps under that name. macOS asks when a program it does not yet
trust for that item reads it. A build from source gets a new ad-hoc
signature each time you rebuild, so macOS asks again after every rebuild.

**Fix:** allow it (enter your login password; *Always Allow* stops the
question for this build of Kule).

### `kule: command not found`

**Cause:** `kule` is not on your `PATH`.

**Fix:** link it into a folder on your `PATH`, for example
`ln -sf /Applications/Kule.app/Contents/MacOS/kule ~/.local/bin/kule`, or
`target/release/kule` for a source build. Check with `kule --version`.
On Windows, add the programs' folder (`%LOCALAPPDATA%\Kule` for the
installer) to your user `PATH` and open a new terminal; see
[Install](getting-started/install.md).

### Windows: SmartScreen says the publisher is unknown

**Cause:** Kule's Windows installer and programs carry no code signature.

**Fix:** check the download first (`SHA256SUMS` and `gh attestation
verify`, see [Install](getting-started/install.md)), then choose **More
info → Run anyway**.

### Windows: "sandboxed commands are refused on this computer"

**Symptom:** no command, terminal or agent runs; the message names the
reason, in the terminal and under *Settings → Policy*.

**Cause:** Kule's Windows sandbox needs Windows 11 24H2 with update
26100.9278 or later, and a desktop session. A service's or a remote shell's
session (SSH, a scheduled task) cannot start the programs a task runs.
Windows Server has no process container that can hide a path.

**Fix:** install the update (`winver` shows the build and revision) and
run Kule from your desktop. There is no weaker mode to accept on Windows:
the container gives a command all of its isolation or none.

### Windows: "the command could read …, and with it …, which Kule keeps from every command"

**Cause:** a folder a command may read holds Kule's home. That happens when
`KULE_HOME` points inside the folder Kule's programs are in
(`%LOCALAPPDATA%\Kule` for the installer), or the `.zip` was unpacked into
Kule's home: a command reads the folder of the terminal's helper, and
Kule's home it never reads.

**Fix:** keep the two apart. Leave `KULE_HOME` unset (the default is
`%USERPROFILE%\.kule`), or point it at a folder of its own; unpack the
`.zip` into a folder that holds nothing else.

## Agents

### An agent shows as missing or cannot start

**Symptom:** the first-run screen or `kule agents list` says an agent cannot
run.

**Causes and fixes:**

- **Node.js is missing.** Claude Code and Codex start with `npx`. Install
  Node.js, then check `npx --version` in a terminal.
- **The app was opened from the Finder or a launcher.** The app reads `PATH`
  from your login shell, so tools set up in `~/.zprofile`, `~/.zshrc` or
  `~/.bash_profile` are found. A tool that only an interactive,
  non-login shell sets up is not. Put its `PATH` line in your login
  profile.
- **The agent is not signed in.** Sign in once in a terminal, as you would
  without Kule (`codex login`, Claude Code's own sign-in, Gemini's `/auth`).
- **A profile you added is not approved,** or changed since: run `kule
  agents show NAME`, then `kule agents approve NAME`. See
  [Agents](using-agents.md).

`kule agents list` says why each one that cannot run cannot.

### Gemini will not sign in

**Cause:** Google no longer serves the Code Assist sign-in to clients like
Kule, and the sandbox keeps the keychain read-only.

**Fix:** use an API key from Google AI Studio, stored in a file:

```sh
GEMINI_FORCE_FILE_STORAGE=true gemini
```

then `/auth` inside Gemini.

### A run is refused before it starts: budget spent

**Cause:** one of the budgets over the run (the task's, its group's, or the
daily one) is spent. The refusal names it.

**Fix:** `kule budget TASK` lists every budget over the task's next run.
Raise or remove the one that holds (`kule budget TASK off`, `kule group
budget ID off`, `kule daily-budget off`). See
[Budgets and limits](concepts/budgets-and-limits.md).

### A run is refused before it starts: what is left is set aside

**Cause:** runs that started before it hold what is left of a budget over
it, until their usage is recorded. The refusal names the budget and the
tasks:
`what is left of <budget> (<amount>) is set aside for runs at work, or that did not end, in task <id>: …`.
A run Kule was stopped during, or whose
usage could not be recorded, keeps its share counted.

**Fix:** wait for the runs at work to end. For a run that did not end, let
go of its share in the task named: `kule budget TASK --release`. See
[`kule budget`](cli/budget.md).

### A run stopped by itself

**Cause:** a [limit](concepts/budgets-and-limits.md) stopped it. Live says
which one and what it measured (`Stopped: the run's processes held 4.2 GB,
over its memory limit of 4 GB`).

**Fix:** `kule limits --task TASK` shows the limits and where each comes
from; raise the one that held.

## Linux

### Linux: agents do not start, "weaker isolation"

**Symptom:** no agent runs; *Settings → Policy* or `kule isolation` lists
what this computer cannot give a sandboxed command.

**Cause:** Kule's Linux sandbox needs **unprivileged user namespaces**.
Ubuntu 24.04 refuses them by default.

**Fix (recommended):** allow them:

```sh
sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0
```

To keep it after a restart, put the setting in a file under
`/etc/sysctl.d/`.

**Or** accept weaker isolation knowingly: `kule isolation --accept-weaker
on`, or *Run agents with what this computer gives* in *Settings → Policy*.
The title bar then says **Weaker isolation** while it holds. `kule
isolation --accept-weaker off` goes back. See
[`kule isolation`](cli/isolation.md).

### Linux: commands fail with a Landlock error

**Cause:** the kernel is too old. Kule's sandbox needs Landlock ABI 3
(Linux 6.2) at least, and ABI 4 (Linux 6.7) for network rules. Kule
refuses to run a command less isolated than it means to.

**Fix:** use Linux 6.7 or later. Check with `uname -r`.

### Linux: a command gets "permission denied" but nothing is listed

**Cause:** Landlock reports refusals only to the audit log, so Kule cannot
list them as it does on macOS. The command still gets the error.

**Fix:** the path is outside what the sandbox allows (see
[The sandbox](concepts/sandbox.md)). For a project beside the workspace,
`kule reveal add PATH`; for one command, `kule exec --reveal PATH` or
`--allow-write PATH`.

## Network and certificates

### A command cannot reach a host

**Symptom:** `CONNECT tunnel failed, response 403`, or
`net: example.com:443 · refused, would wait for approval; the command line asks no one`.

**Cause:** the [policy](concepts/policy.md) puts that host to a person. The
command line has nobody to ask, so it refuses at once.

**Fix:** run the work in the app and approve the question; or let the host
through on this folder with `kule rules allow HOST:443 --yes`. For one
command only, `kule exec --open-network` bypasses the proxy and policy.

### pip fails with certificate errors on macOS

**Cause:** pip 24.2 and later checks certificates through the system's own
service, which stays closed in a task while its network goes through the
proxy.

**Fix:** `pip install --use-deprecated=legacy-certs …`.

### A connection is cut for Encrypted Client Hello

**Symptom:** a browser or a tool built on one cannot reach a host the task
may reach, and the cut says *hides the name it asks for (Encrypted Client
Hello)*.

**Cause:** Chromium and Firefox send an Encrypted Client Hello extension to
every host, also to hosts that publish no ECH config for it ("GREASE").
Kule cannot check the name such a hello asks for, so it cuts the tunnel. A
real one and a GREASE one look alike on the wire.

**Fix:** turn it off in the tool. Measured in October 2026 ([#34](https://github.com/kulehq/kule/issues/34)):

| Tool | Sends it through Kule | What keeps it working |
| --- | --- | --- |
| Chromium (Debian), Chrome for Testing | yes | The policy `{"EncryptedClientHelloEnabled": false}` in a JSON file in its managed policy folder: `/etc/chromium/policies/managed/` for Chromium, `/etc/opt/chrome_for_testing/policies/managed/` for Chrome for Testing. `--disable-features=EncryptedClientHello` no longer does anything. |
| Chrome, Brave | yes | `--disable-features=EncryptedClientHello` does nothing here either; the same policy in their own policy folder was not measured. |
| Playwright with `channel: "chromium"` | yes | The Chrome for Testing policy above. |
| Playwright's default Chromium (the headless shell) | yes | None found: the headless shell ignores the policy. Use `channel: "chromium"`. |
| Electron (`net`, `fetch` in its windows) | yes | None found. Electron's Node side (`https`, `fetch` in the main process) does not send it. |
| Firefox, Firefox ESR, Playwright's Firefox | yes | The preference `security.tls.ech.grease_probability` set to `0` (in `user.js`, or `firefoxUserPrefs` in Playwright). `network.dns.echconfig.enabled` does not stop it. |
| Node (`tls`, `fetch`), Python, Go, curl, Claude Code, Gemini CLI | no | Nothing needed. |

On macOS a browser does not start in a task in the first place: it needs
the window server, and Chromium's headless shell needs to register a
service unless it runs with `--single-process --no-sandbox`.

### NuGet, Maven or Go on macOS

Their tools on macOS trust the system's roots alone, so their registries
pass through the proxy unseen (writes included), as Go's module proxy does.
Go is told to check certificates against the roots `SSL_CERT_FILE` names.
Nothing to fix; know that the proxy does not look inside these
connections.

### A client fails on HTTP/2 or WebSocket

**Cause:** the proxy does not speak HTTP/2 or WebSocket upgrades yet.
Clients usually fall back to HTTP/1.1.

## Disk space

### `~/.kule` looks huge

**Cause:** Finder, `du` and disk-usage tools count every clone at its full
size. On macOS most of it is shared with your own files.

**Fix:** look at the free space instead (`df -h`, or System Settings →
General → Storage). On Btrfs use `btrfs filesystem du`. See
[Tasks and copies](concepts/tasks-and-copies.md).

### On Linux each task takes the folder's full size

**Cause:** ext4 cannot share blocks, so the copy is a full copy, `.git` and
`node_modules` included.

**Fix:** keep workspaces and Kule's home (`KULE_HOME`) on Btrfs or XFS with
reflink support, on the same volume. Delete tasks you no longer need
(`kule rm TASK`); their space comes back once the trash is emptied.

## Review and commit

### `kule commit`: "not committed; rerun with --approve"

**Cause:** some changes wait for your approval (the rule is named, for
example `manifests-and-lockfiles`).

**Fix:** look at them (`kule status TASK`), then `kule commit TASK
--approve`, or `kule rollback TASK`.

### `kule commit`: "the workspace changed since Kule last saw it"

```text
kule: the workspace changed since Kule last saw it, at: hello.txt (both changed lines 1–3)
hint: roll back, or rerun the task on the current workspace
```

**Cause:** you (or another task) changed a file in the folder that this task
changed too.

**Fix:** bring the folder's changes in first with `kule sync TASK`, which
merges what it can; or roll back and run the task again. See
[Sync and merging](concepts/sync-and-merging.md).

### `kule sync`: "nothing was brought in"

**Cause:** a file both changed does not merge (the same lines, method,
section or key).

**Fix:** commit or roll back the task's change to that file first, or
leave the copy as it is.

### A terminal or editor will not open on the copy

**Symptom:** "the copy's git config changed, and git there would run what
it says outside the sandbox; review it, then commit or roll it back before
opening a terminal".

**Cause:** the task changed what would run in the copy by itself, outside
the sandbox: its git config or hooks (your shell prompt's `git status`), a
toolchain pin such as `rust-toolchain.toml` or `.nvmrc`, the package
manager a `package.json` names, `.envrc`, or an
editor's settings. Or an agent is at work in the task: wait until it is
done.

**Fix:** review the change in Review (or `kule status`), then commit or roll
it back. Such changes stop only what opens the copy outside the sandbox
(*Open in terminal app*, *Open the copy in your editor*,
`kule shell --outside`); the task's terminal and `kule shell` run in the
sandbox and open anyway.

### Commit, sync or a run waits for a terminal

**Symptom:** "a terminal is open on this task's copy (in Kule's app, or kule shell);
close it first, then commit" (or *then sync*, *then roll back*, *then
undo*, *then go back to a step*, *then apply its home changes*, *then undo
its home changes*, *then start work*).

**Cause:** a terminal is open on the task: a tab of the app's **Terminal**
panel, or `kule shell`. Kule does not end what runs there, so nothing you
have not saved is lost.

**Fix:** close the terminal's tab, or type `exit` in it, then try again.

### Nothing starts: Kule cannot mask a settings file

**Symptom:** "the secrets in .env.local cannot be masked (it is not UTF-8), so
no command may read it" (or *larger than 1 MB*, *unreadable*) when you
start a run, `kule exec`, `kule shell` or a terminal.

**Cause:** a `.env`-style file in the task's copy is not UTF-8 (one Latin-1
byte is enough, or a UTF-16 file a Windows editor saved), or is larger
than 1 MB. Kule cannot mask its values, so nothing that would read them
starts.

**Fix:** save the file as UTF-8 under 1 MB in your folder (or move it out
of the folder), bring it in with Sync, then try again.

A file like that in your home is not refused but withheld: the run's notes
say *~/.bashrc is hidden from the commands here*. Save it as UTF-8 to give
it back.

### Nothing starts: a task's record cannot be read

**Symptom:** "task 6c2816d8's record cannot be read (…): Kule cannot tell
which folder it was made from, so no command or run starts in any task" when
you start a run, `kule exec`, `kule shell` or a terminal in any task, or
look at budgets. The task list shows that task by its id as *cannot be
read*.

**Cause:** that task's `task.json` under Kule's home was damaged (a disk
that filled up, an edit by hand). Kule hides every folder it opened from the
other tasks' commands, and cannot tell which folder this one was, so it
starts nothing rather than leave that folder open.

**Fix:** delete the task: `kule rm 6c2816d8`, or *Delete task* from its menu
in the app. Its copy goes with it; no folder of yours is touched.

### A run is refused after opening the copy in an app

**Symptom:** the app asks *Close … first* before a run, or `kule run` and
`kule exec` say "Kule opened task …'s copy in … outside the sandbox; …
Close it, then say so: in the app, or with `kule closed …`".

**Cause:** *Open in terminal app* or *Open the copy in your editor* opened
the copy outside the sandbox, and Kule cannot see that app close.

**Fix:** close the terminal app or editor, then choose *I closed it, start
the run* in the app, or run [`kule closed TASK`](cli/closed.md).

### A program that needs the screen fails (LibreOffice and others)

**Cause:** a task cannot reach the window server. LibreOffice aborts even
with `--headless`, and macOS may report that it quit unexpectedly. Live
shows a line about it, and the agent is told once.

**Fix:** none inside a task. Do that step outside Kule.

### "Open with its app" is refused

**Cause:** Kule opens only documents, pictures, sound and video that are
what their name says, reached without a link, while no agent is at work.
Never a program, a script, a web page, SVG or an Office file with macros:
they would run outside the sandbox. PDF, Word, Excel, PowerPoint,
OpenDocument and CSV files are refused when they would load something from elsewhere or
run something: the message says what.

**Fix:** wait for the run to end, or look at the file in Preview.

## The daemon

### The app says the daemon speaks another protocol

**Symptom:** "the Kule daemon speaks protocol N (version), this app M".

**Cause:** a daemon from another build of Kule is still running for this
home, for example after you rebuilt or updated.

**Fix:** quit the app and let the old daemon end: it exits by itself after
ten minutes with no window, no agent at work and no question waiting. If no
run is going on, you can also end its process (`kule daemon`, or
`kule-desktop daemon` when the app started it). Then open the app again.

### "socket path … is longer than … bytes; use a shorter KULE_HOME"

**Cause:** the daemon listens on `<home>/kuled.sock`, and Unix socket paths
have a short length limit.

**Fix:** set `KULE_HOME` to a shorter path.

### Where are the daemon's logs?

In `kuled.log` in Kule's home (`~/.kule/kuled.log`). Run `kule daemon` in a
terminal to watch it in the foreground. See [`kule daemon`](cli/daemon.md).

## Still stuck?

- `kule help COMMAND` explains every command; see also the
  [CLI reference](cli/index.md).
- Open an issue at [github.com/kulehq/kule](https://github.com/kulehq/kule/issues).
- A security problem? Report it privately, as
  [SECURITY.md](https://github.com/kulehq/kule/blob/main/SECURITY.md) says,
  never in a public issue.
