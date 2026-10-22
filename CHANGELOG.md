# Changes

## Unreleased

### Security

- *Open with its app* opens Kule's own copy of the file, made before it is
  checked and readable only by you, without the file's extended attributes,
  and on macOS marked as downloaded. Kule looks inside Word, Excel,
  PowerPoint, OpenDocument and CSV files first, and opens none that holds
  macros, controls or embedded objects, links to anything outside the file
  but a web page or an email address, or has a field, formula or CSV cell
  that fetches or runs something. Pages, Numbers, Keynote, EPUB and RTF
  files are no longer opened with their app.
- On macOS, while a task's network goes through the proxy, its programs do
  not listen for connections unless you allow servers for it: the new
  `servers` limit (Settings → Limits, `kule limits servers=on`), off by
  default. The macOS sandbox sees the port a server listens on but not its
  address, so allowed servers are kept off your network from outside it,
  which is weaker isolation you accept under Policy. A refused server is
  listed after the command, with how to allow it.
- On macOS a task's commands no longer write to the system's `/tmp`, which
  your other programs and tasks share: `TMPDIR`, and now zsh's `TMPPREFIX`,
  point at the task's own temporary directory.
- A tunnel the proxy does not look inside goes on only once the client's
  TLS hello asks for the host it was opened to, without hiding the name it
  asks for; port 443 carries nothing but TLS. Plain `http://` requests are
  read one by one, each for the host it was let through to. A cut
  connection is logged as `cut`, with why.
- On a host Kule holds a secret for, a request carries no credentials but
  Kule's: headers and URL parameters named as credentials by any spelling,
  `Basic` with the placeholder as the password, and the fields of JSON and
  multipart bodies as well as forms. That host is asked for whole responses
  only, and a response that repeats a secret has it blanked in more of the
  forms it can take (base64, escapes in either case), even split between
  chunks.
- For IPv6, only the global unicast range counts as public, with the IPv4
  address inside 6to4 judged as itself. Host names are judged in lower
  case and without a final dot. `Content-Length` takes digits alone.

## 0.2.0-preview.4

A security preview. Nothing an agent writes in Kule reaches your home:
Kule's own agents keep their settings, history and transcripts in a home
of their own in Kule, start every time from your settings as they are, and
anything else an agent writes in your home stays in the task's copy of it.

### Security

- Nothing an agent writes in Kule reaches your home. Each of Kule's own
  agents has a home of its own in Kule (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`,
  `GEMINI_CLI_HOME`), where its settings, history and transcripts in Kule
  are kept across tasks; `~/.claude`, `~/.codex` and `~/.gemini` are hidden
  from it. Sessions run in Kule before stay where they were.
- At every start Kule copies your settings, hooks, skills, commands,
  extensions, rules, trusted folders and instructions into that home,
  read-only while the agent runs. Whatever an agent left there or beside
  them, the shell snapshots Claude Code sources included, is gone at the
  next start. Claude Code's plugins are not copied.
- An agent's `state` is a copy in the task's home layer: the agent writes
  there, and none of it is applied to your home or offered for review.
- Codex signed in with ChatGPT signs in once more, in its home in Kule
  (see Agents); Kule never copies that sign-in. Claude Code on Linux keeps
  its sign-in in `~/.claude/.credentials.json`, which Kule reads and the
  agent never sees, as on macOS with the keychain.
- Profiles take `outside`, `carry` and `login_file` for the same in your
  own agents. `protected` is no longer needed; a profile that has it still
  loads, and `kule agents show` says so.

## 0.2.0-preview.3

A security preview. Your terminals run in the task's sandbox, and the copy
opens outside it only when you choose, after seeing what changed. Secrets
you keep stay masked inside longer lines, and Kule's own files in a task's
temporary folder go only where it means them to. Releases now carry what
they are built from, and are checked before they are published.

### Security

- A secret you keep is masked wherever it is in the settings files Kule
  masks (the copy's `.env` files, and `.zshrc`, `.profile`, `~/.env` and
  the like in the home layer): inside a longer value, in an alias, a
  command or a comment, as it is or as a URL or JSON writes it. Secrets
  kept for other folders are masked too, under placeholders no host
  fills, since the home is the same for every folder. A commit or an
  apply puts such a line back only where it is as Kule masked it, and no
  more times than the file held it.
- Anything in `.git` that git does not keep as data (objects, refs, the
  index, logs) counts as git config: it waits for you as git config does,
  and keeps the terminal and the editor closed on the copy.
- Kule's terminal, `kule shell` and *Open the copy in your editor* wait
  while an agent is at work in the task, and stay closed while the task
  has changed a file that a shell prompt or an editor runs by itself: a
  toolchain pin (`rust-toolchain.toml`, `.nvmrc`, `.python-version`,
  `.tool-versions` and the like), `.envrc`, an editor's settings and
  tasks, and the rest of what the `automation` rule names. The built-in
  policy decides this, whatever policy you chose.
- A change in another version control system's repository (Mercurial,
  Sapling, Jujutsu, Darcs, Pijul, Bazaar, Subversion, Fossil), at any
  depth, waits for you (`other-vcs`) and keeps the terminal and the editor
  closed on the copy: its config and hooks run on its next command, and
  Kule reads none of it as a repository.
- The version pins a shell prompt reads on entering a folder (`.nvmrc`,
  `.node-version`, `.python-version`, `.ruby-version`, `.java-version`,
  `.go-version`, `.sdkmanrc`, `.terraform-version`, mise's other files) and
  `*.code-workspace` wait for you at commit (`automation`).
- What Kule writes in a task's temporary folder (the proxy's certificate,
  the loopback hooks, the package caches) goes through folders it opens
  without following a link, and replaces whatever stands at the name.
- The window's terminals and `kule shell` run your shell in the task's
  sandbox, as `kule exec` runs a command, on a terminal of their own:
  what you run there, and what your prompt runs by itself, reaches only
  what the agent's commands reach. `kule shell --outside` opens your
  shell outside the sandbox, as before.
- While a terminal is open on a task, no agent starts work there, and
  commit, sync and applying home changes wait until you close it, rather
  than ending what runs in it.
- *Open in terminal app*, *Open the copy in your editor* and `kule shell
  --outside` show the changes not committed yet and ask before they open
  the copy outside the sandbox, each time. Kule remembers what it opened
  the copy in, and before the task's next run asks you to close it
  (`kule closed TASK` from a terminal).
- The desktop app no longer depends on `portable-pty`; its terminals use
  the system's pseudo-terminals directly.

### Fixes

- A setting that is only a switch (`true`, `false`, `yes`, `no`, `on`,
  `off`) or a number of fewer than 8 digits is no longer taken for a
  secret because of its key (`AUTH_ENABLED=true`, `MAX_TOKENS=4096`); a
  number stays one where the key names a password or a PIN. Masked, such a
  setting stopped working in the task, and the task's proxy blanked the
  same word in every response, so installs from crates.io and npm failed
  for anyone whose home held one.
- A search path or a folder under a key that names one
  (`PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`, `GOPATH`,
  `JAVA_HOME`, `XDG_DATA_DIRS`) is no longer taken for a secret by its
  length. It stays as it is in the task's `.zshrc`, `.profile` and `.env`
  files and shows in their diff, so a login shell in the task finds its
  tools. It stays masked where the key names a secret, where a part of it
  is no path or looks like a token, and where a secret Kule keeps or
  masked elsewhere is in it.
- A file you save in your home while Kule makes a task's home layer is
  compared with its clone once the disk's clock has moved on, as the
  workspace copy already is, and cloned again when they differ. On a disk
  whose clock is coarse, a same-size save then could go unseen: the layer
  held what the file was before, and an apply wrote over your save. Each
  file's stamp is now read by the clock of the disk the file is on.

### Development

- The kule CLI in a release carries its dependency tree (built with
  cargo-auditable), so `cargo audit bin` and other scanners can read what
  is inside it. The release workflow audits the very binaries it is about
  to publish, on macOS (each slice of the universal binary) and on Linux,
  and the gate refuses a binary without that tree, of which cargo audit
  reads only part. CI runs osv-scanner over every lockfile on every
  change.
- The release gate runs sandbox-probe in a task, against a home with
  something at every path Kule hides, and installs, builds and tests real
  projects in a task (ripgrep from crates.io, commander from npm). CI runs
  both on macOS and Linux on every change; the release workflow runs them
  on the binaries it is about to publish, and publishes nothing unless
  they pass.

## 0.2.0-preview.2

A small preview: up-to-date certificate checks, and the security model
says what Kule does not cover.

### Security

- Every HTTPS connection Kule makes itself, its proxy's included, checks
  certificates with webpki-roots 1.0.10 and rustls-webpki 0.103.16, which
  no longer trust the distrusted emSign certificates (RUSTSEC-2026-0381,
  RUSTSEC-2026-0382).

### Documentation

- The security model has a section on what Kule does not cover. An action
  with no copy, no diff and no undo (a click, a keystroke, a message sent
  from an app on your screen) is refused in a task by design; tool calls
  have no gate; and nothing outside a task is protected, even while Kule
  is running.

### Development

- A sandbox test no longer fails, or leaves a server running, on a Mac
  with the application firewall on.

## 0.2.0-preview.1

The first preview: the macOS app (universal) and Linux `.deb` and `.rpm`
packages, with the `kule` command-line tool inside each, and the tool
alone for each system. Expect changes from one preview to the next.

### Tasks on a copy of any folder

- A task is a copy-on-write copy of a folder: code, notes, photos or data.
  Everything an agent does there is one uncommitted transaction; commit
  applies it with a checkpoint, rollback throws it away, undo reverts the
  last commit. Checkpoints live in a Lonca store, and every commit and undo
  is an entry in a signed, hash-chained history.
- Review shows what changed inside each file (methods, sections, keys), a
  picture before and after, moves told apart from deletions, and where two
  tasks on one folder meet. A file two tasks changed merges method by
  method; only the same parts changed twice conflict.
- Writes in your home directory (`~/.zshrc`, `git config --global`) are
  caught for review, with a line diff, a mark on files that run at every
  start, and undo.

### A sandbox and a policy

- Commands run in Seatbelt on macOS and Landlock (with a network namespace
  of their own) on Linux: they write only to the copy, credentials are
  hidden, and the network goes through the task's proxy.
- Kule's home is hidden from a task's commands, but for the task's own
  copy and tmp: the one the task is in, and `~/.kule` too when the task's
  is another (`--home`, `KULE_HOME`).
- A Cedar policy decides what passes, what waits for you and what never
  passes. Choose Kule's balanced or careful one, or write your own; a
  repository can add refusals. What you approve can be remembered.
- Secrets reach the hosts they are for and never the agent: it sees a
  placeholder, and the proxy fills in the value, in headers and bodies.
  The built-in agents' API keys go the same way.

### Agents

- Claude Code, Codex and Gemini over the Agent Client Protocol, each with
  a profile that opens only its own state and login. Add any other ACP
  agent from the registry or by hand; it runs once you approve what it
  opens, and a trial suggests what it needs.
- Costs as the agents report them, with a budget per task and a daily one
  for every task together, and a notice at 80%.

### The app

- Live stream, approval inbox, review and timeline, around a daemon that
  keeps the runs going when the window is closed.
- Tasks with names, colours and an order of your own; a ⋮ menu on each.
- Settings for agents, the policy, remembered rules, secrets and the
  budget, and a first-run screen that checks the agents and asks how
  careful to be.

### Documentation

- A documentation site, in English and Turkish, at
  https://kulehq.github.io/kule/: install, a quick start, the concepts,
  the desktop app panel by panel, a page for every `kule` command, the
  files and settings Kule keeps, platforms, troubleshooting, and a video
  tour. It is built from the Markdown in `docs/` (`website/`).
- The README is a shorter, task-first introduction: what Kule does, a
  quick start from source, the commands at a glance and how to develop it.

### Releases

- A version with `-` (`0.2.0-preview.1`) is drafted as a GitHub
  pre-release, never shown as the latest one.
- Publishing a release can also put it on npm (`@kulehq/kule`, a pointer
  to the releases, under the `preview` dist-tag for a pre-release) and on
  crates.io, each once a repository variable turns it on. Both use trusted
  publishing in the `release` environment, so no registry token is kept,
  and the crates are built in a job that can mint no token.

### Writes in your home

- `kule home TASK diff|apply|discard` takes a path however it is typed:
  `~/.zshrc` unquoted (which the shell turns into your full home path),
  quoted, or relative to your home. A path outside your home, or one with
  `..`, is refused with a message that says so.
- A home diff (`kule home TASK diff`, *Show* in Review) hides what looks
  like a secret on both sides, as `‹hidden …›`: settings named like keys,
  tokens or passwords, the secrets Kule keeps, and anything shaped like a
  token. One value has one mark in a diff, so a changed secret still
  reads as changed.

### Keeping up with the folder

- A task brings what its folder changed meanwhile into its copy: your own
  edits, or another task's commit. `kule sync TASK`, *Bring into the
  copy* under Review, the task's menu, or Mod+Shift+G.
  - A file only the folder changed comes in as it is; a file the task
    changed too is merged by method, section or key and stays the task's
    change; when anything does not merge, nothing comes in and Kule
    names what both changed.
  - Commits, branches and staged files in the folder's repository come
    in too; a branch both moved does not.
  - Settings files come in masked, the folder is only read, and steps
    start over as after a commit.
  - The agent hears what came in at the start of its next turn.
- Review, the task list and `kule status` show what the folder changed
  since the copy was last brought up to date, and which of those files
  the task changed too.

### Outside Kule

- A file of the copy opens with its app: right-click it in Files, ⌘O, or
  *Open with its app* where the Preview does not show it (a PDF, a film)
  or shows it in part (Word, Excel). Only documents, pictures, sound and
  video that are what their name says, and only while no agent is at
  work; never a program, a script, a web page or a file with macros.
- *Show in Finder* (Files on Linux) points at a file, a folder or the
  whole copy without opening it; *Copy path* copies where it is on disk.
- The task's menu opens the copy in VS Code, Cursor or Zed, the ones the
  computer has, refused like the terminal while the task changed the
  copy's git config or hooks. The first time for each editor, Kule says
  what an editor runs by itself from a folder and how to keep it from
  doing so.
- `kule open TASK FILE` opens a file of the copy with its app, held to the
  same kinds and checks (`--dry-run` checks it and prints where it is).
  `kule path TASK FILE` prints where a file of the copy is, and `kule
  path` says on stderr when git in the copy would run what the task
  changed.
- Opening a file with its app also waits while an agent `kule run`
  started is at work.

### Disk space

- The guide says what a task really takes on disk, on macOS and on
  Linux, and why Finder and `du` show more than that.

### Budgets and limits for a group

- A group has a budget and limits of its own. Its tasks share the budget:
  over all of their runs, over any 24 hours, or both. Its limits hold each
  of its tasks where the task sets none of its own, and every task's hold
  where the group sets none.
- Every budget over a run holds, its own, its group's and the daily one
  for every task, and the tightest decides; a refusal names the budget
  that is spent. A run counts for the group its task was in when it ran.
- Settings → Budget and Settings → Limits show one scope at a time, picked
  under *Applies to*: every task, one group or one task. The scope is named
  in capitals with whom it holds, each choice says where an inherited
  value comes from, a task's scope lists every budget over its next run
  and the one that holds, and each change is confirmed with the scope's
  name. A group's ⋮ menu opens its budget and limits.
- Moving a task into or out of a group asks first when its limits or group
  budgets would change, and says how.
- A notice at 80% and when it is spent, as for a task's budget, now also
  for a group's budgets and the daily one for every task, each named. A
  24-hour budget that drops below 80% again is told again when it nears.
- `kule limits` shows and sets every task's limits, `--group ID` a group's
  and `--task TASK` a task's, each line naming the scope its value comes
  from. `kule group budget ID AMOUNT [--daily]` sets a group's budgets,
  and `kule budget TASK` without an amount lists every budget over the
  task's next run.
- A run slowed to its processor limit keeps to it, on a busy computer
  too: what its processes take before Kule first sees them counts, and
  so does a slice that ran longer than planned. Before, a run held to
  40% could get about half of a core.
- A slowed run that goes quiet, waiting on a timer or the network, runs
  on; before, it could stay paused until its time limit.

### The window

- The left and right sides grow as far as the window allows, and the
  bottom panel as high: the main area between them always keeps at least
  360 pixels across and 160 high. A window made smaller takes the room
  from the sides first, and gives it back when it grows again.
- Settings → Agents shows each agent as one line, its name, where it
  comes from, whether it can run, how many notes it has and what it runs;
  a click on the line, or Enter, opens what it opens. An agent held for
  approval or not installed starts open. *Open all* and *Close all*, and
  the command "Open or close every agent's details".
- Settings' sections scroll sideways when the window is too narrow for
  all of them, with a mouse wheel, a trackpad or the arrow keys; a faded
  edge shows there are more. Panel tabs scroll the same way.

### Agent runs

- A run that fails before its agent starts (the policy, the proxy, or
  installing or launching the agent) ends and says why; before, it stayed
  *at work* with nothing running, and Stop could not end it.
- Where Kule knows why a refusal keeps a tool from working, Live says so
  under the run's last line, and the agent hears it at its next prompt,
  once, in Kule's words only. New: a program that wants the screen
  (macOS's window server), such as LibreOffice even with `--headless`,
  cannot run in a task; read or convert such files with a command-line
  tool instead.
- The disk measure and limit leave out the package caches as a task
  starts with them (npm's and cargo's are clones of the person's, which
  take no room of their own); what a task adds to them still counts.

- Live sets your prompt apart from the agent's answer: the prompt in a
  block of its own under *You*, the answer under the agent's name.
- Each of your prompts and each of the agent's messages has a copy
  button, which copies it as written, Markdown and all.
- Fonts of your own: Settings → Appearance → Fonts picks one font for
  the window's text and one for code (the agent chat, terminals, diffs),
  from the fonts on your computer, each in a size of its own. Mod+= and
  Mod+- make all text larger or smaller. Profiles carry the fonts; one
  this computer lacks falls back to Kule's own.
- Text selected with the mouse in the window or a terminal is copied at
  once, and a small *Copied* shows by the pointer. Settings → Appearance
  turns it off; so does a command, which can have a shortcut.
- Two messages of the agent's no longer run together: where the agent
  says which message a piece belongs to, as Claude Code's adapter does,
  each message is a paragraph of its own, in the app and in `kule run`.
- Live shows what each tool call printed, such as a command's output,
  as it comes: under the call's line, open while it runs or when it
  failed, closed once it went well. Terminal colours and cursor moves are
  left out, and the last 64 KB of each call's output are kept.
- A command's output comes while it runs:
  - Claude Code's and Codex's adapters send it in pieces once Kule asks
    for them (ACP's terminal extension); Kule passes them on at most every
    150 ms.
  - An agent that asks Kule to run its commands (ACP `terminal/*`) gets
    them run in its own sandbox, behind the same proxy, in the task's copy.
    Kule refuses variables that load code into every program or override
    what the sandbox sets (`DYLD_*`, `LD_*`, `*_PROXY`, `HOME`, `TMPDIR`),
    keeps at most 1 MB of output, and stops what a command left running
    when the run ends.
- Go back to any step in Timeline (or to before the first): the copy is
  put back as it was then. Going back is a step of its own, so it can be
  gone back on, and changes since the last step are kept as a step first.
  - After each step Kule keeps the files it changed: a clone on APFS and
    btrfs, a copy elsewhere, at most 64 MB a file and 1 GB a transaction.
    A step whose later changes were not kept (too large, or a file with a
    second name) cannot be gone back to, and Timeline says why.
  - Files are read without following a link at any step of their path, so
    a link the agent puts in place is kept as a link, never as what it
    points to. Settings files with secrets are masked again on the way back.
  - `.git` is not put back; a commit starts the steps over.
- Fork a task (its menu, `Mod+Shift+D`, or "Fork from here" on any step
  in Timeline): a new task on the same folder starts from this one's copy,
  as it is now or as it was after that step. It takes the task's label
  (with "(fork)"), colour, group, budget and secret stand-ins, and has no
  steps or commits of its own yet. Its changes reach the folder through
  its own review and commit, as any other task's.
- Servers working in a task's copy show in its header as
  `localhost:5173 ↗`, whoever started them (the agent, or you in a
  terminal); a click opens them in the browser. Kule finds them by their
  working directory and listening port (`lsof` on macOS, `/proc` on
  Linux). On Linux a server inside the sandbox's own network is shown
  greyed out, since this computer does not reach it. A server the agent
  started still ends with its run; one started in a terminal stays.
- Servers a task starts listen on this computer only, so nothing on your
  network reaches what an agent runs:
  - On Linux without a network namespace of the command's own (Ubuntu
    24.04 by default, or a policy that allows the network), a seccomp
    filter hands every `bind` and `listen` to Kule, which does the call
    itself on a copy of the socket: every address becomes loopback, an
    address on the network is refused, whatever the program (Go and Rust
    included). With a namespace of its own, the command's servers were
    already out of the network's reach.
  - On macOS, where the sandbox can tell a listening port but not its
    address, hooks loaded into Node, Python and Ruby (through
    `NODE_OPTIONS`, `PYTHONPATH` and `RUBYOPT`, which survive every shell on
    the way) give a server that asks for every address the loopback one.
    Express, Next, Vite, `http.server`, Flask, Django, Puma and the like
    keep working.
  - On macOS, for every other program (Go, Rust, Java, …), Kule shields
    the port while a run goes on: where a process of the run listens on
    every address, Kule takes the same port on each of the computer's
    network addresses (IPv6 and VPNs included). The kernel hands a
    connection to the most specific address, so what comes from the network
    reaches Kule, which resets it, while loopback still reaches the server
    and the browser keeps working. A process that listens on a network
    address of its own is stopped, and Live says why. Kule looks every
    50 ms, reading sockets from the kernel (libproc, under a millisecond a
    look), so only that first moment is open. The chip shows 🛡 for a shielded server and
    turns amber, with a warning, for one the network still reaches (a
    server you started in a terminal, which Kule leaves to you).
- Limits on what a run takes, in *Settings → Limits* for every task and,
  field by field, for each task (its ⋮ menu): processor, memory, disk,
  GPU, one run's time, all of a task's runs' time, and time doing nothing.
  - Over the processor limit a run is slowed down by pausing and resuming
    its processes in turn; over memory they are killed at once; over disk
    or time the run is stopped, and Live says why.
  - Disk counts what the copy, temporary files and home changes grew by
    since the task was made; no single file can be written past it.
  - The GPU is on or off: off, Metal gets no device on macOS and the GPU's
    devices stay read-only on Linux. On Linux it was off before; it is now
    on unless turned off.
  - Doing nothing is no message, tool call, output, processor use or new
    process from the agent; a question waiting on you does not count.
    Processes too short-lived to be seen count once they end, by the
    processor time their parent waited for (a build's compilers, a script's
    commands).
  - Live shows what a run takes against its limits, every two seconds.
  - `kule run` is held to the same limits.
- **Stop** in Live (⌘⇧K) stops the run at work: its turn is cancelled, and
  the agent killed if it does not end it within ten seconds.
- Daemon protocol 26: `StopRun`, `Limits`, `SetLimits`, `SetTaskLimits`,
  the `resources` run event, and the limits and time used in each task.

### Security

- Hardened containment of agents on macOS and Linux, and the integrity of
  what a commit takes.
- Where a computer cannot give an agent's commands all of their isolation
  (Linux without unprivileged user namespaces), Kule runs no agent until
  you accept weaker isolation in Settings → Policy or with `kule isolation
  --accept-weaker on`; the title bar says so while it holds.
- A model's API is reached only during the runs of the agent whose profile
  names it (its keys' hosts count), no longer by every command; a profile
  of your own lists its model's host in `hosts` or `keys`. Go's module
  proxy now asks, as other hosts do.
- Your own files (Documents, Desktop, Downloads, Pictures, Movies, Music,
  iCloud Drive and cloud storage folders) are hidden from a task's
  commands, and so is the folder the workspace lies in, with the projects
  beside it, where it is a folder of yours in your home. The workspace's
  git directory and the folders your `PATH` and SDK variables name stay
  readable there.
- A project beside a folder that the folder builds from opens to its
  tasks, read only, once you open it: `kule reveal add ../shared`, or
  Settings → Rules in the app, where Kule suggests what the folder's own
  manifests name (Cargo, npm, Go, Python, .NET and Gradle), and the task
  header says what its folder builds from and cannot read. A new task's
  copy sits next to links to those projects, so `../shared` works from the
  copy as from the folder. `kule new` names what the folder builds from.
- Daemon protocol 29: `Besides`, `SuggestBeside`, `AddBeside`,
  `RemoveBeside`.
- The values a task masks in its copy's `.env` files are kept sealed in
  the task's `secrets.json`, with the key the kept secrets use, each to
  the setting it came from; a task's file from before is sealed at its
  next read. Kule never makes a second key where the system already keeps
  one for its home.
- Go programs in a task check certificates against the roots the proxy
  names (`GODEBUG=x509sslcertoverrideplatform=1`, after your own
  settings), so their HTTPS works through the proxy on macOS. A refusal
  that keeps a tool from HTTPS says what to do (pip 24.2 and later:
  `--use-deprecated=legacy-certs`), in `kule exec` and in the timeline.
- On Linux each command's processes are its own (a PID namespace, with a
  `/proc` that shows only them): it sees and signals no other program,
  and what it leaves running ends with it. A background server an agent
  starts lives as long as the agent, or the terminal command that started
  it. Kule still sees how the command ended, its code or its signal, and
  the signals sent to it reach it. Where the computer cannot give this,
  it is one more piece of isolation you accept going without.
- On Linux each command has System V shared memory, message queues and
  semaphores of its own (an IPC namespace), and a host name of its own (a
  UTS namespace). Where the computer cannot give this, it is one more
  piece of isolation you accept going without.
- `scripts/release-gate.sh`, the checks before a release: workflows
  (actionlint, zizmor), lockfiles (osv-scanner), the built binary
  (cargo-auditable, `cargo audit bin`), canary secrets kept out of a task,
  and the provenance of a release's assets. CI runs the workflow and
  canary checks.
- pnpm keeps its store in the task's caches with every version: through
  `PNPM_HOME` up to pnpm 10 and `pnpm_config_store_dir` from pnpm 11.
  `npm_config_store_dir` is no longer set, so npm 11 no longer warns about
  it on every command.
- On Linux a command makes terminals of its own (a devpts instance in its
  mount namespace), as `script`, `expect` and test runners need; none of
  your terminals is there to open.
- On Linux a task's commands get devices by name only (`null`, `zero`,
  `full`, `tty`, `random` and `urandom` to read, and the GPU's), never
  `/dev` whole; from Landlock ABI 5 a device takes requests (ioctl) only
  where it was given to be driven. An agent's profile names more under
  `devices`; disks, memory, keyboard and mouse, consoles and terminals are
  refused.
- Settings that look like secrets in the home layer's clones of shell
  startup files (`.zshrc`, `.profile`, `.gitconfig`, …) and env files
  (`~/.env`) are masked as in the copy's `.env` files: commands see
  placeholders, and the real files are hidden from them; an apply puts
  the values back, and the diff shows no change where only masking was.
- A command `kule exec` runs sees nothing any agent holds: its state
  (what you said to it, in every project) and its home of its own, not
  only its login.
- macOS: with the network open (`--open-network`), commands resolve host
  names again.
- The proxy judges every address a host resolves to with Kule's own check
  before it connects, and the address a tunnel reached before a byte goes
  through: this computer, its network and the reserved ranges stay out of
  reach, inside an IPv6 address too. Extensions' `net.fetch` uses the same
  check.
- Releases come with a CycloneDX SBOM of the CLI and of the app, are built
  with the toolchain `rust-toolchain.toml` names, and are published by a
  job of their own that waits for a maintainer's approval. No job of the
  release workflow holds a secret. The macOS app is signed ad hoc, not
  notarized; every release's notes say how to check a download and open
  it.
- Daemon protocol 28: `Isolation` and `SetIsolation`.

### Motion

- The window moves a little where something changes, never just to
  decorate:
  - Menus and the command palette grow out of what opened them.
  - The selected tab's line slides to the next tab.
  - Toasts come in from the right and leave the same way.
  - Buttons, tabs and rows change colour over a moment.
  - A new event in Live rises into place.
  - A line runs along the task header while the agent is at work.
  - A question opens where it is asked and its edge pulses twice; answered,
    it folds away.
  - Rows in the task list slide to their new places, and a new one drops in.
  - A run that ends well draws its tick.
  - The measure line's numbers run to each new measure.
  - The first window marks the step done, the one to take now and those
    after it.
  - A row Kule takes you to, such as a task's limits or budget or a group's
    policy, is scrolled to and lit for a moment.
- Settings → Appearance → **Motion**: *As the computer says* (the default:
  off when the computer's "reduce motion" is on), *On* or *Off*. With it
  off nothing moves. The command *Turn motion on or off* takes a shortcut
  of your choice. The choice is kept with the themes and goes with a
  profile's appearance.
- Daemon protocol 27: the themes carry `motion`.

### Preview

- A file picked in Files opens large in the main area's new Preview tab
  (`Mod+4`), as it is now in the task's copy:
  - text and code with line numbers; JSON or XML packed onto a line or two
    can be shown formatted;
  - Markdown laid out, or as its source;
  - PNG, JPEG, GIF and WebP pictures, fitted or at 100%;
  - CSV and Excel (`.xlsx`) files as a grid from A1, with each sheet as a
    tab and a bar that shows a picked cell's value or formula. Formulas are
    not computed: a cell shows the value Excel saved with it.
  - Word (`.docx`) documents as text to read: headings, paragraphs, list
    items numbered as Word numbers them, and tables. Formatting, pictures,
    notes and deleted tracked changes are left out.
- Ask the agent about a place in the Preview. Pick some text, drag over
  cells or over a region of a picture, and an Ask button shows beside it
  (`Mod+Shift+A` does the same; with nothing picked it asks about the whole
  file). Its box asks the agent now, or adds the place to the prompt as a
  chip; chips stay with their task until the prompt goes.
  - A place is named the way the agent can find it: lines of a file, the
    source lines and headings of a Markdown section, a range of cells and
    the values in it, a region in the picture's own pixels, or a numbered
    paragraph of a Word document.
  - The agent gets the places as a numbered list after the prompt. Kule
    checks that each is a file of the task's copy, reached without a link,
    and sends at most 20, with picked text cut at 500 characters.
  - Live shows a prompt's places as chips over it.
  - A region of a picture also goes as a picture of its own (at most 1024
    pixels a side, three to a prompt) to an agent that says it takes
    pictures. Kule checks each is a PNG or JPEG by its bytes. An agent that
    does not take pictures gets where the region is, and Live says so.
- Files shows each file and folder with its icon as VS Code's Material
  Icon Theme draws it, and the Preview colours code as an editor does
  (Prism, 53 languages, up to 300 KB of a file), as do code blocks in the
  agent's answers. The colours come from the theme in use. Both are
  vendored at pinned versions by `scripts/vendor-ui-assets.py`, which checks
  their digests and refuses an SVG with a script or an outside link.
- An Excel or Word file is read in memory and never unpacked. Each part it holds
  is inflated through a cap, so a small file that expands to gigabytes is
  not read.
- Text shows up to 1 MB of a file. A picture is shown only when its bytes
  prove it to be one of a sane size, the same check Review uses.

### Extensions

- Extensions add commands, panels, themes, settings, notes on a review,
  agent profiles and MCP servers. They are written in JavaScript and shared
  as `.kulext` files (`kule ext new | dev | pack | info | install | approve
  | enable | disable | remove | mcp`, or Settings → Extensions).
- Approval is pinned by digest:
  - an extension asks for permissions from a closed list;
  - it runs only once approved as it stands, and waits again after any
    change;
  - the approval screen names what is new since last time.
- Its code runs in sandboxed frames served from loopback, which reach
  nothing but Kule. Kule answers each call only if the approved permissions
  cover it:
  - `net.fetch` reaches the approved hosts over https only, and never an
    address on the computer or its network;
  - keys pressed in a frame only move the view.
- A `.kulext` is never unpacked. Each entry's path, kind and inflated size
  are checked.
- Agent profiles and MCP servers from an extension are each approved on
  their own. MCP servers are started by the agent inside its sandbox.
- For extension authors (a preview: the format and the SDK may still
  change before 1.0):
  - ES modules work in frames;
  - every frame's console output and errors show in the extension's card;
  - a folder under development has Reload;
  - the guide covers language and tools, lifecycle, debugging, sharing,
    updating and limits.
- Hardened after an OWASP review:
  - frames may come from the extension server's port only;
  - WebRTC is off in the window;
  - files are read without following links;
  - folders under development may not sit where agents can write;
  - a frame can only show panels;
  - MCP servers start one exact registry package;
  - `kule ext` goes through the daemon and asks before approving;
  - the publisher `kule` is reserved;
  - a safe mode, `--no-extensions`, is added.
- Daemon protocol 23. The new `TaskChanged` notice says when a task is
  created, committed, rolled back, undone or removed.
