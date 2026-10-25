# Changes

## 0.2.0-preview.14

On macOS, what a task's sandbox refused is read as the system log streams
it and checked against the log's store, and when macOS dropped the kernel's
log messages while the sandbox was in use, a quiet line says when. Each
sandbox's mark is chosen at random.

### Security

- On macOS, Kule reads what a task's sandbox refused as the system log
  streams it, through one stream per Kule process opened before the
  sandbox's first command, and lists it as all the log kept only once the
  log's store, too, shows a refusal Kule makes after the commands'.
  Otherwise `kule exec`, `kule run`, `kule shell` and the app's run and
  terminal notes say some refusals may be missing. When the log noted that
  it dropped the kernel's messages while the sandbox was in use, which a
  busy Mac does every few seconds, a quiet grey line says when: a refusal
  made just then is not listed. The exit code stays the command's.
- Each sandbox's mark is chosen at random rather than taken from the clock,
  so sandboxes made at the same moment are told their own refusals, and
  what one ends is never another's.

## 0.2.0-preview.13

Kule tells you, where you work, more of what it cut, ended or refused, and
says it once: in the terminal and Live, after `kule exec`, `run` and
`shell`, and for a task whose record cannot be read, which no longer fails
the whole list and can be deleted. A task's first command, terminal or run
no longer waits silently for its package caches: the app makes them as the
task is made, and a terminal shows what it waits for. A connection cut for
Encrypted Client Hello says what turns it off in each tool, as measured.

### Fixes

- On Linux, a command starting in one task is no longer ended by another
  task's clean-up (before a commit, sync or rollback) that happens at the
  same moment: Kule looks again once the command runs its program, and
  ends it only if it still holds the task's files then.
- A connection cut for Encrypted Client Hello says what turns it off, as
  measured in #34: for Chromium-based browsers the policy
  `EncryptedClientHelloEnabled` set to false (the `--disable-features`
  switch Kule named before no longer does anything), for Firefox
  `security.tls.ech.grease_probability` set to 0. Troubleshooting has a
  table per tool, also of those with no setting (Playwright's headless
  shell, Electron) and of those that need none.
- A task's first command, terminal or run no longer waits silently while
  Kule makes the task's own package caches (on macOS, a clone of your npm
  and cargo caches, which takes seconds for large ones). The app starts
  making them as the task is made or forked; one that comes before they are
  ready waits and says so, in the terminal, in Live, or on the command
  line, as does the CLI's first command when it makes them itself.
- A terminal tab shows a turning line until its shell starts, saying what
  it waits for (still with less motion turned on), and keeps a line saying
  so once the shell is there.
- Two terminals opened at once on a task (the app's own and one you open)
  both open: one waits a moment while the other masks the copy's secrets,
  instead of being refused as if a commit were under way.
- `kule exec`, `kule run` and `kule shell` tell all of what came after a
  command, a run or a shell even when one part cannot be shown (a folder in
  the copy that cannot be read back, say): that part is a `kule: … cannot
  be shown` line, what the sandbox refused is still listed and recorded, and
  the command's, run's or shell's own exit code or error still comes back.
  One that succeeded fails, as part of it went untold. On Linux, `kule exec`
  says after a failed command that Linux does not tell Kule what the
  sandbox refused, as `kule run` and `kule shell` do.
- `kule run` gives the agent the MCP servers you approved from extensions,
  as the app's runs do; it ran without them before, saying nothing. When
  what you approved cannot be read, the run goes without them and says so.
- What the sandbox refused a terminal in the app is no longer lost when you
  close its tab before its shell ends (the lines go to the task's Live) or
  quit Kule with it open (its shell ends with Kule, and the refusals are
  kept for the Timeline and the task's next run; the window closes at once
  meanwhile).
- A turned-over `agent.log` starts with a line saying so, and that the log
  before was let go when there was one. A failed run points to
  `agent.log.1` only when the log was turned over during that run, not to
  one an earlier run left.
- A run whose agent reported no cost, in a task with a budget, says in the
  app that the budget does not count it, as `kule run` already did.
- A shell's probes of the terminals around it are one line (`/dev/ttys*`,
  `/dev/pts/*`, counted) whichever terminals it tried: in the Timeline, in
  Live, where a terminal's shell ends, and after `kule exec`, `kule run` and
  `kule shell`, which also list the same refusal once with how many times.
  In the Timeline, *… and N more refusals* counts the refusals left out,
  not the rows.
- One task whose record cannot be read no longer fails the whole task list:
  `kule list` names it on stderr and lists the rest, and the app lists it
  by its id as one that cannot be read; selected, it shows why, with
  *Delete task*, instead of trying to load it. What needs every task (commands and
  runs in any task, since Kule cannot tell which folder that one hides, and
  budgets) still refuses while it is there, and now says which task and
  that `kule rm <id>` (or *Delete task* in the app) removes it; that works
  without reading its record.
- `kule` says an error's cause once, not twice (`state file …: EOF …`).

## 0.2.0-preview.12

What passes on its own under the balanced policy is now an allow-list:
files of a kind known not to run by themselves, or of a kind your project
already holds. Everything else, scripts and hidden files among them,
waits for you, shown put together by why on the one review screen, and a
kind can be answered for good, except what runs code when a tool opens
the folder. Choosing Kule's careful policy is kept as a choice, so it
always means this version's rules. Releases are built and published on
pinned images, and are immutable once published.

### Changed

- Choosing Kule's careful policy is kept as a choice
  (`~/.kule/policy-choice`), not a copy of its text, so it always means this
  version's rules. A careful policy an earlier version wrote out whole is
  that choice too: it is judged by today's rules and can be changed again,
  where it read as a policy of your own before.
- Under the balanced policy, a file passes on its own only when it is of a
  kind known not to run by itself (source, docs, data, pictures and fonts)
  or of a kind your folder's git repository already holds. Every other kind
  waits for you (`unfamiliar-kinds`), scripts such as `.sh` among them, and
  so does whatever the task writes in a hidden file or folder, of any kind,
  for that reason alone (`dot-paths`). The kinds your project has are read from the folder's own
  repository, never the task's copy. Deletions and folders pass as before.
  Policies can read `file_kind`, `kind_tracked` and `dotted`.
- A kind of file can be answered for good, as the project's from then on:
  `kule rules allow --kind vue`, or "Don't ask again in this folder" on a
  file held for its kind. A hidden file is answered by its path. What runs
  code on its own (CI, editor and agent settings: `automation`) is asked
  every time; an answer an earlier version kept for it is listed as no
  longer applying, in `kule rules` and in the app, and can be forgotten.
- What holds a commit is shown put together by why, in `kule status`,
  `kule commit` and the app's review: one heading per reason in plain words
  ("a kind of file this project has none of: weird", "runs code when a tool
  or an agent opens the folder"), the rules after it, and the paths under
  it. Files that wait only for their kind are one group per kind, answered
  at once. Rule names a repository's own policy gives are shown with their
  control characters escaped.

### Development

- Release builds, the gate and publishing run on pinned runner images
  (`macos-26`, `ubuntu-22.04` for the Linux packages, `ubuntu-24.04`), and
  the npm publish on an exact Node version, so a change of image shows as a
  change here first. A release run by hand publishes nothing, whatever ref
  it runs on. Published releases are immutable, and no one, admins
  included, publishes a release or the docs site past the environments'
  rules.

## 0.2.0-preview.11

A hardening preview. An approval card says what approving lets through,
and what became of the connection is told where you work. A commit and a
home apply take exactly the changes you reviewed, and a file made
executable or writable by anyone waits for you. Extensions are approved
together, and a folder under development as every file it carries. The
command line talks only to a daemon of your own user, and home changes
are written through folders opened without following a link. A task's
first run no longer waits on its package caches, and on macOS the last
refusals of a run are no longer lost on a busy machine.

### Changed

- An approval card says what approving lets through: a tunnel Kule does not
  look into (it checks only that the TLS hello asks for that host), or a
  connection Kule looks inside, naming the secrets it fills there or that a
  registry stays read only. Extensions with `questions.read` learn the
  kind, never the secrets' names.
- Once answered, a question stays in the run as a line. A connection you
  approved that Kule then cuts, or refuses because its name led to an
  address on this computer or its network, says so on that line with why,
  while the run is live, and in a notification. The Timeline and what
  `kule run` and `kule exec` print say why a connection was cut, refused or
  failed.
- A question Kule refused without your answer is told as Kule's, never as
  "denied by you": nobody answered in time, the run ended first, or too
  many of the run's questions were waiting (told once per host and port).
  Each is a line in the run and a notification, and the log records it as
  `timed_out`, `withdrawn` or `too_many`.
- A connection refused for the address its name led to names that address
  to you; the command is told only that such addresses are out of reach.
- Approving a commit takes the changes you reviewed: the app names them,
  and `kule commit --approve` takes `--reviewed DIGEST` (the digest
  `kule status` prints) or, on a terminal, lists them and asks. Without
  either, nothing is approved.
- Applying home changes takes them as reviewed too (`kule home` prints a
  home digest; `kule home apply --reviewed`, or a yes on a terminal), and
  never brings a setuid or setgid file into your home where yours was not.
- A file made executable, or a file or folder made writable by anyone,
  waits for you under the balanced policy (`permissions`). On macOS, a file
  whose extended attributes differ from the folder's (a quarantine mark
  added or removed, say) is part of what you review and waits too
  (`extended-attributes`); what macOS sets on its own on every write is left
  out. Policies can read both (`gains`, `xattrs_changed`).
- A repository's own rules are named `repo:<id>`, and no answer you
  remembered cancels one, whatever its `@id`.
- A permission note says what Kule did, from the kind of option it chose
  ("Kule allowed", "Kule did not allow"), with the agent's label only
  quoted after.
- An extension's approval warns when, together with another that runs, it
  can read your work and send data (one reads, the other sends). Approving
  it approves the pair; a pair nobody approved holds both until one is
  approved beside the other.
- A folder under development is approved as all its files are: any change
  to its code waits for approval again. One that a task's folder takes in
  after it was installed is held from then on, saying why.
- Extensions learn which task is chosen only with `tasks.read`.
- What an extension shows (names, notifications) is checked against the
  same set of characters that show nothing as the window uses, zero-width
  ones and line separators included.
- Approve on a question's card, and its shortcut, take nothing for a moment
  after the card appears, so a card that shows up under the pointer is not
  approved by accident.
- The command line and the app talk only to a daemon of your own user: what
  listens at the daemon's place as anyone else is sent nothing, its token
  included. Kule's home must be a folder of yours and not a link; one open
  to others is closed to `0700`, saying so.
- A task is refused on a folder that holds `~/.kule`, also while its own
  home is elsewhere (`--home`, `KULE_HOME`).
- A scan reads at most 1,000,000 files and folders, and a review hashes no
  file over 8 GiB; past either, Kule refuses, naming the path and the limit.
- `kule exec --allow-write`, `--reveal` and `--pass-env` each say what they
  open, as `--no-sandbox` does.
- On Linux without namespaces of its own, a command's filter refuses
  `ptrace`, `process_vm_writev`, `keyctl`, `userfaultfd`, `perf_event_open`
  and `setns` by name, whatever the host's sysctls allow.
- Applying and undoing home changes write, remove and link through the
  folders on the way, opened without following a link, as a commit does in
  the workspace; an undo refuses a link on the way before it puts anything
  back.

- A task's first run no longer waits while Kule walks the package caches it
  starts with. Only a disk limit needs them measured: the first run under
  one measures them once and says so ("Measuring the task's package caches
  for its disk limit"). Without a disk limit nothing walks them, and the
  disk figure a run shows leaves them out.
- On macOS, what the sandbox refused is read once the system log shows all
  of it, so a busy machine no longer drops the last refusals from what a
  run, a terminal or `kule exec` tells. Where the log still has not shown
  all of them after ten seconds and three reads, what it showed is listed
  and Kule says that some refusals may be missing, in the command's output,
  the terminal and Live; the command's exit code stays its own.
- `kule exec` says what of the sandbox's refusals Kule could not read or
  record, and still exits with the command's own code; a command that
  succeeded fails then, since part of what it did went untold.

## 0.2.0-preview.10

A security preview. A run sets aside what it may spend before it starts,
so runs started at once under one budget share what is left and a run
whose usage never comes still counts. A request Kule refuses inside a
connection is logged as cut, with why. And Kule tells you what it cuts,
ends or refuses, where you work: in the command's output, the app's
terminal, Live, the Timeline and the task list.

### Changed

- A `kule run` that fails or is stopped still prints what the run changed, its
  connections and home writes, what the sandbox refused, what Kule put back,
  the policy's ruling, what was spent and how long it took, and only then
  the error. The error names `agent.log`, and `agent.log.1` beside it when
  the log was turned over during the run; a stop says why, without a log.
  What Kule could not record, put back or read after the run is a `kule:`
  line each, and the command fails.
- On Linux, a failed run or shell, and every finished run in Live, say that
  Linux does not tell Kule what the sandbox refused, rather than list no
  refusals.
- Kule names what it ended in a task's sandbox, once per name with how many
  (`sleep ×2`): before a commit, a rollback, a sync, an undo, going back to a
  step, or applying or undoing home changes (on stderr; in the app as a
  notice and a note in Live), and as `kule exec`, an agent's run, `kule shell`
  or the app's terminal ends.
- The app's terminal says, as its shell ends, what the sandbox refused it
  (the same refusal once, with how many times), Kule's hints and what Kule
  ended. A terminal Kule did not open says so and how to try again, and its
  cursor stops blinking.
- The Timeline's *Refused by the sandbox* gives the total, lists the same
  refusal once with how many times, and says how many older refusals are
  left out after the newest 50.
- In Live, a finished run's line folds a list of what the sandbox refused
  (the newest 20, and how many more the Timeline holds). A stop you asked
  for shows as a stop, not a failure. A run's notes say when Kule cannot
  read the extensions' MCP servers (the run goes without them) and when it
  cannot write the agent's log (the agent goes on). A window opened later
  says how many earlier events are no longer kept.
- A task Kule cannot read stays in the task list, marked *cannot be read*,
  with why.
- A record of what a run spent that cannot be read is counted, not taken
  for nothing spent: every budget it could count for (the task's, both of
  its group's, the daily one) holds the next run, and says why, until the
  record is mended or removed or the budget turned off. `spent:` lines and
  the app's spent chip say how many records cannot be read. A record cut
  short by a crash no longer makes the next run's record unreadable too.
- After *Open with its app*, the app says the file opened as Kule's own
  copy, which Kule removes after 24 hours.
- An extension that is on but cannot run in the window now says *Does not
  run now*, with why (safe mode, extensions cannot be served, or waiting
  until the window's WebRTC is off), instead of *Runs*.

### Security

- Before an agent starts, Kule sets aside the run's share of its budgets
  (the tightest of the task's, its group's and the daily ones), with what
  other runs set aside counted as spent, one run at a time across Kule's
  processes. Runs started at once under one budget share what is left, and
  a run is refused, naming the budget and the tasks, while what is left is
  set aside. A share that cannot be written starts no run.
- A run whose usage never comes (Kule was stopped, or what it spent could
  not be recorded or read) counts at its whole share: for a daily budget
  until the 24 hours pass, for the task's and the group's over all runs
  until `kule budget TASK --release` lets go of it. That is refused while an
  agent is at work in the task, and what is let go of stays on record.
  `spent:` lines, `kule budget`, `kule daily-budget`, `kule group budget`,
  `kule status --json` and the app say what is set aside.
- Where Kule looks inside a connection and answers a request itself (a
  write to a read-only registry, a credential Kule does not hold, a body it
  cannot check whole, a request it cannot read one way only), the
  connection is logged as `cut`, with why in Kule's words. A connection
  that ends in an error of its own (the host went away, a reset) is logged
  as `failed`, with why, for plain HTTP too.

## 0.2.0-preview.9

A security preview. A settings file Kule cannot mask no longer shows its
values to a task's commands: in the copy nothing starts until it can be
masked, and in your home it is withheld and Kule says so. A connection
cut for reaching this computer's network is logged as such.

### Security

- A `.env`-style file in a task's copy that Kule cannot mask (not UTF-8,
  or larger than 1 MB) no longer reaches the task's commands with its
  values: no agent run, `kule exec`, `kule shell` or terminal starts while
  the copy holds one, and Kule names the file and why.
- A settings file in your home that Kule cannot mask (not UTF-8, or larger
  than 1 MB) is withheld from the commands, in the home layer and at its
  real path, rather than shown or linked; the run's notes in the app, the
  output of `kule run`, `exec` and `shell`, and the terminal say so.
- A connection whose name led to an address on this computer or its
  network is logged as `cut`, with why, rather than as a tunnel that went
  through.
- The messages Kule writes into the app's terminal carry no control
  characters, so a file name in them cannot steer the terminal.

## 0.2.0-preview.8

A security preview. Rolling back, undoing and going back to a step hold
the task's copy still, as commit and sync do, and Kule puts files back
through folders it opens without following a link. The copy does not
open outside the sandbox while a `package.json` names another package
manager.

### Security

- Rolling back, undoing, going back to a step and undoing home changes
  work as commit and sync do: while a terminal Kule opened is open on the
  task they wait, saying so, and what still runs in the task's sandbox is
  ended before Kule writes in the copy. No terminal opens while Kule is
  writing there.
- Kule puts files back in a task's copy, and into your folder, through
  folders it opens one at a time without following a link, so a folder
  swapped for a link on the way, at any moment, takes nothing anywhere
  else.
- The copy does not open in a terminal app or an editor outside the
  sandbox while a `package.json` in it names another package manager
  (`packageManager`, `devEngines.packageManager`), which the next `pnpm` or
  `yarn` there would fetch and run.
- A terminal open on the task is named as such in the message (the app's,
  or `kule shell`), without a process id.

## 0.2.0-preview.7

A security preview. A task's commands read neither the other projects on
your computer that Kule opened or that lie at the top of your home, nor
where apps and tools keep their settings, sign-ins and data, but for what
toolchains, shells and prompts need there.

### Security

- A task's commands no longer read the other projects Kule opened,
  wherever they lie, in your home or outside it, nor the projects beside
  them; nor anything at the top of your home but its settings (dot folders
  and `~/Library`), so your projects there, opened in Kule or not, and your
  files there are hidden as your own. The tools your `PATH` names stay
  readable, and *builds from* opens one of those projects read-only.
- Where apps and tools keep settings and data (`~/.config`,
  `~/.local/share`, `~/Library/Application Support`,
  `~/Library/Preferences`) is hidden from a task's commands, but for what
  toolchains, shells and prompts read there (git, uv, pip, Poetry, Ruff,
  mise, Go, fish, Starship and the like) and the folders of the tools on
  your `PATH`, links into them included. An app's sign-in, and what a new
  app keeps there, stay hidden; on macOS also through the system's
  preferences service. The task's home layer holds only what is shown.
- Also hidden: other tools' sign-ins (GitHub Copilot, Firebase, Wrangler,
  Fly, Doppler, SOPS, Podman's `auth.json`, Poetry's `auth.toml`, Yarn's and
  Bun's settings in your home, NuGet's own `NuGet.Config`), the stores of
  editors built on VS Code (Insiders, Cursor, Windsurf, VSCodium), and saved
  shell sessions.
- A task whose home layer holds changes waiting for review asks you to
  apply or discard them before the next run, once, as the layer is made
  anew for what is hidden now.

## 0.2.0-preview.6

A security preview. An agent's output and the files it writes keep to
limits: Kule's memory, disk and window stay in hand however much the agent
says or writes, and nothing a run or `kule exec` started outlives it, nor
escapes the run's limits. *Open with its app* looks inside PDFs too, and on
macOS a task's servers are kept off your network wherever they run.

### Security

- An agent keeps to what Kule can hold: a message longer than 16 MiB, or
  more than 1024 tool calls left open at once, stops the run; a tool call's
  title is cut to 200 characters; Kule runs at most 32 of its commands at
  once. Its log keeps its last 8 MB, with the 8 MB before in
  `agent.log.1`. What the daemon keeps of a task's runs for the window
  stays near 32 MB.
- Every run, from the app, `kule run` or an agent's trial, and `kule exec`
  end whatever was left running in the sandbox, whatever became of its
  parent; a run's limits count such a process while it runs.
- A preview of a document, sheet or CSV holds at most 16 MB; JSON or XML
  nested more than 256 levels deep is shown as written; a list marker
  keeps to 64 characters.
- Connections that have not shown the daemon's token yet never keep its
  clients out.
- The window lays out an agent's messages and Markdown files in time that
  grows with their length, nests quotes at most 32 deep, and shows a
  message or file it cannot lay out as plain text, so the stream, Review,
  Files, Preview and Timeline keep working. A message is laid out once,
  not again on every event of the run.
- *Open with its app* looks inside PDFs too, the objects packed into
  compressed streams included, and opens none with a script, a program to
  start, a form that sends itself, another document or an attached file,
  media that runs, or an action that runs by itself as it opens; nor an
  encrypted one. Links you click stay.
- On macOS, where a task's servers may listen, Kule finds its processes by
  its sandbox, whatever started them and whatever became of their parent,
  and keeps their servers off your network in the agent's runs, the task's
  terminals, `kule exec`, `kule run` and `kule shell` alike. A process that
  a connection from the network reached in the moment before Kule looked
  is stopped.

## 0.2.0-preview.5

A security preview. What an agent wrote opens with its app only as Kule's
own checked copy, keeping to itself. On macOS a task's servers listen only
where you allow them, and the system's `/tmp` stays closed to it. The proxy
passes a tunnel only to the host it was opened to, and on hosts Kule holds
a secret for, only your credentials.

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
  address, so allowing servers is weaker isolation you accept under
  Policy: Node, Python and Ruby servers are pointed at this computer, and
  while an agent runs in the app the others are kept off your network from
  outside the sandbox. A refused server is
  listed after the command, with how to allow it.
- On macOS a task's commands no longer write to the system's `/tmp`, which
  your other programs and tasks share: `TMPDIR`, and now zsh's `TMPPREFIX`
  and Claude Code's `CLAUDE_CODE_TMPDIR`, point at the task's own temporary
  directory.
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
