# Changes

## Unreleased

### Agent runs

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
  with the toolchain `rust-toolchain.toml` names, and are signed in a job
  of their own that waits for a maintainer's approval: the builds never
  see the signing certificate.
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

## 0.2.0

The first release: the macOS app (universal, signed and notarized), Linux
`.deb` and `.rpm` packages, and the `kule` command-line tool inside each.

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
