# Windows measurements behind the port plan

What was measured on a real Windows 11 machine before Kule's Windows port
depends on it, and what each result decides. The tools are in
`spikes/windows` (AppContainer, job objects, named pipes, block cloning) and
`spikes/windows-mxc` (Microsoft's `mxc-sdk`). Results are recorded at the
level of decisions; this file is not a test report and not a how-to for
getting around a boundary.

Machine: Windows 11 Pro 24H2, build 26100.9711, ARM64, Defender on, no
Developer Mode, UAC off. `mxc-sdk` 1.0.1 (MIT), ProcessContainer backend,
tier `base-container`. Dates: 2026-10-26.

## What holds

- **ProcessContainer is available** on this build: deny paths, enumerate
  paths, host-loopback ingress, identity-less loopback proxy and denial
  capture are all reported, for an administrator and for a standard user.
- **Filesystem policy is deny by default and the precedence is sane.** A
  read-only folder inside a read-write one stays read-only; a denied folder
  inside either is unreadable and unwritable; a junction that points outside
  the grants is refused ("untrusted mount point"); paths outside the grants
  are refused whether spelled as a drive path, a `\\?\` path, a `GLOBALROOT`
  path or a UNC loopback path. The contained process cannot create symlinks
  or junctions. Create, rename and delete inside the read-write grant work.
  Denied names are still visible in a listing of their parent; their content
  is not.
- **The identity-scoped proxy path works without an administrator step.** A
  listener running in an AppContainer profile named `Kule.Proxy` received the
  container's HTTP traffic when the request named that profile as the proxy
  peer; a direct connection past the proxy was cut at once; a listener
  outside the profile, with the profile still named, was never reached. The
  identity-less mode reaches a plain host listener and not the profiled one.
  Measured twice with the same result: as an administrator (UAC off) and as
  a **standard user in an interactive console session**, both with the
  firewall on all three profiles at its defaults, no package-scoped rule and
  no loopback exemption. Kule's installer therefore writes no firewall rule
  and no exemption for the proxy; a machine policy that blocks outbound
  traffic by default would stop the proxy's own egress, and Kule's end-to-end
  self-check names that instead of weakening anything.
- **The container cannot reach what is the person's.** Another user's files,
  the Credential Manager's stored credentials (`cmdkey /list` shows none),
  an owner-only named pipe (access denied), writes to the registry hive and
  the clipboard are all refused. The process runs at Low integrity.
- **ConPTY works inside the container** when the UI policy is not disabled:
  a pseudo console can be created, `cmd` and `powershell` run under it and
  their output is captured. Kule's terminal helper is feasible.
- **Developer tools run** with their folders granted read-only and the
  ancestors of every grant marked enumerable: git (init, add, commit,
  status, log), node (including `realpath.native`), npm install over an open
  network, powershell, cargo and rustc.
- **Processes end with the container.** No process was found lingering two
  seconds after the foreground command ended, including after a timeout
  kill. Kule still tracks children itself and tests this on its own code.

## What decides the design

- **The UI lockdown cannot be used.** With `ui.disable = true` every program
  that loads `user32.dll` dies at start (`STATUS_DLL_INIT_FAILED`): whoami,
  node, git, powershell, tasklist. Only `cmd` and `curl` survive. Kule runs
  containers with the UI enabled and keeps the job-level UI isolation
  (same-job handles, private atom table, no clipboard, no input injection).
- **Containers need an interactive desktop session.** Started from an SSH
  session (session 0), the same programs die regardless of the UI policy
  (microsoft/mxc#1465). Kule's daemon and CLI run in the person's desktop
  session, so this is a precondition Kule checks and names, not a gap. It
  also means GitHub-hosted runners cannot run the sandbox tests; the test
  VM does. A standard user cannot register a scheduled task for itself
  (the ScheduledTasks module's CIM session is refused), so the standard-user
  measurements were started by an administrator as a task that runs in the
  standard user's console session.
- **Every ancestor of a grant must be enumerable.** Without it, tools that
  resolve paths upward fail in confusing ways: `dir` of a granted folder
  says "Access is denied", a batch file given by absolute path does not run.
  With `C:\`, `C:\Users`, the profile and the parents of the task copy as
  enumerate-only paths, listing works and file contents there stay
  unreadable.
- **PSEC replaces `LOCALAPPDATA`, `TEMP` and `TMP`** with a per-container
  profile folder even when the caller supplies them, and requires
  `SYSTEMROOT` and `LOCALAPPDATA` to be present in a supplied environment
  (presence only). Kule points tools at the task's own temporary folder
  through the variables PSEC leaves alone and treats the container profile
  folder as part of the task.
- **A listener inside the container is not reachable from the host** on this
  build, in every network mode (connections time out). Kule's port chips say
  so instead of offering a dead link; nothing is weakened to get around it.
- **DPAPI is reachable from inside**: a blob protected by the user can be
  unprotected by a contained process. Hiding the files that hold protected
  secrets (browser profiles, credential stores, `UserSecrets`) is therefore
  a real boundary and stays in the hidden list.
- **Block cloning is a space saver, not a speed-up** (earlier round, on a Dev
  Drive): clones are independent but not faster to make than a copy on this
  machine. Dev Drive stays optional and unadvertised.
- **The restricted-token design was dropped** (earlier round): the
  AppContainer holds more and `mxc-sdk` builds on it.

## Open

- `git` inside a container refuses a repository whose folder is owned by
  `Administrators` ("dubious ownership"); a standard user's copies are owned
  by the user, so Kule only has to carry `safe.directory` for people who run
  as an administrator with UAC off.
- ChangeTime and FileId semantics on NTFS and ReFS are measured by the
  WP1 tests themselves.
