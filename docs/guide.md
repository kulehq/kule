# Using Kule

A guide to the desktop app: what Kule does, a task from start to finish,
every part of the window, and what each setting is for. The
[README](../README.md) explains how each part works underneath; this guide
is about using them.

- [The idea in one minute](#the-idea-in-one-minute)
- [First start](#first-start)
- [The window](#the-window)
- [A task from start to finish](#a-task-from-start-to-finish)
- [Watching a run: Live](#watching-a-run-live)
- [Questions: when the agent wants to connect somewhere](#questions-when-the-agent-wants-to-connect-somewhere)
- [Review and commit](#review-and-commit)
- [Timeline: steps, going back and forking](#timeline-steps-going-back-and-forking)
- [Files and Preview](#files-and-preview)
- [Asking the agent about a place in a file](#asking-the-agent-about-a-place-in-a-file)
- [Servers the task runs](#servers-the-task-runs)
- [Keeping runs in check: budgets, limits and Stop](#keeping-runs-in-check-budgets-limits-and-stop)
- [Disk space: what a task really takes](#disk-space-what-a-task-really-takes)
- [Organising tasks](#organising-tasks)
- [Terminals](#terminals)
- [Settings, section by section](#settings-section-by-section)
- [Keyboard](#keyboard)
- [When the window is closed](#when-the-window-is-closed)
- [The same from a terminal](#the-same-from-a-terminal)
- [What is new](#what-is-new)

## The idea in one minute

You give Kule a folder: code, notes, documents, photos, data. Kule makes a
**task**, a copy-on-write copy of it. On macOS, and on Linux with Btrfs or
XFS, the copy costs almost no disk space; only what changes takes room
(see [Disk space](#disk-space-what-a-task-really-takes)). An **agent**
(Claude Code, Codex, Gemini or another one that speaks ACP) works in that
copy, inside a sandbox:

- it can write only to the copy and its own temporary files;
- your credentials (`~/.ssh`, `~/.aws`, …) are hidden from it;
- its network goes through the task's proxy, which lets through what the
  policy allows, asks you about the rest, and logs every connection.

Nothing reaches your real folder until you look at what changed and
**commit** it. A commit keeps a checkpoint, so **undo** puts the folder
back. If you do not like the work, **roll back** and the copy is as it was.

So the loop is: *make a task → tell the agent what to do → watch → review
→ commit or roll back*.

## First start

With no task yet, the window opens on a first-run screen with three steps:

1. **Agents.** Kule checks which agents can start on this computer and,
   for those that cannot, says what to install (Claude Code and Codex need
   Node.js for `npx`, Gemini its own CLI). Sign the agents in the way you
   do without Kule; Kule uses their own logins.
2. **How careful to be.** Pick a policy:
   - **Balanced**: changes to ordinary files pass on their own. What decides
     what gets installed, built or run (manifests, lockfiles, CI files,
     editor settings), git history rewrites and mass deletes wait for you.
   - **Careful**: every change to a file or to git waits for you. Only
     package registries, and each agent's own API while it runs, are
     reached without asking.

   You can change this later in *Settings → Policy*.
3. **A folder.** Pick the folder the first task works on.

Opened from the Finder or a launcher, the app reads `PATH` from your login
shell, so agents find `npx`, `node` and the rest as they do in a terminal.

## The window

The window has four **regions**: left, main, right and bottom. Each holds
**panels** as tabs:

| Panel | What it shows |
| --- | --- |
| **Tasks** | Your tasks and groups; *Needs you* collects the tasks waiting for an answer or an approval. |
| **Files** | The task's copy as a tree, with an icon for each kind of file and changed files marked. |
| **Agent** (Live) | The conversation with the agent and the prompt. |
| **Review** | What the task changed and what the policy says about it, with Commit and Roll back. |
| **Timeline** | Commits, steps, network connections and sandbox refusals. |
| **Preview** | A file of the copy, shown large. |
| **Terminal** | Your own shells in the copy, one tab each. |

By default Tasks and Files are on the left, Review, Timeline and Preview
in the main area, and the Agent panel and terminals share the bottom.

- **Moving panels:** drag a panel's tab to another region, or use the tab's
  menu. Resize a region by dragging its edge.
- **Showing and hiding regions:** use the switches at the right of the title
  bar or the button at the end of a region's tab bar. The bottom panel can
  fill the window. The main area and the bottom panel are never hidden
  together.
- **Getting back to the start:** *Reset the layout* (from the command
  palette, Mod+K) returns to Kule's own layout.
- The layout is kept across restarts.

**Mod** below means ⌘ on macOS and Ctrl on Linux and Windows. **Mod+K**
opens the command palette: every action of the window is there by name,
with its shortcut.

The task header, above the main area, shows:

- the task's name and folder;
- chips for the servers working in its copy (see
  [Servers the task runs](#servers-the-task-runs));
- its state ("agent at work", "3 changes · needs approval", …);
- what its runs have spent so far.

## A task from start to finish

1. **Make a task.** Click *New task…* (Mod+N) and pick a folder. Kule
   copies it; on APFS and btrfs that is a clone, quick even for large
   folders.
2. **Write a prompt.** In the Agent panel, write what the agent should do
   at the prompt line (Mod+L puts the cursor there). Below it:
   - the **agent** picker chooses who takes the prompt;
   - **waits** sets how long the agent waits for your answer when it asks
     to connect somewhere the policy puts to you (1 minute to 8 hours).
     Unanswered, the connection is refused and the agent goes on without
     it.
3. **Run.** Press Enter or click **Run** (Shift+Enter makes a new line).
   The prompt shows in Live, and the agent's work follows as it comes.
4. **Watch, and answer when asked.** See
   [Live](#watching-a-run-live) and
   [Questions](#questions-when-the-agent-wants-to-connect-somewhere).
5. **Review.** When the run is done, Live says so. Open Review (Mod+2) to
   see what changed.
6. **Commit or roll back.** *Commit* (Mod+Shift+Enter) applies the changes
   to your folder and keeps a checkpoint. *Roll back* (Mod+Shift+Backspace,
   press twice) throws them away. You can run the agent again before
   either: changes pile up in the copy until you decide.
7. **Undo, if needed.** Timeline's *Undo* (Mod+Shift+U, press twice) takes
   the newest commit back out of your folder.

One agent works in a task at a time. Make more tasks, on the same folder
or others, to run agents side by side. Where two tasks on one folder
change the same file, Review says where they meet.

## Watching a run: Live

Live shows, as they come:

- **Your prompt**, in a block of its own under *You*, and the agent's
  answer under its name. Each message has a copy button (it shows when
  the pointer is over the message) that copies it as it was written,
  Markdown and all.
- **The agent's answer**, laid out as Markdown. Code blocks are coloured
  like in an editor.
- **Its thinking**, in a quieter style, when the agent shares it.
- **Tool calls**, one line each: `·` while running, `✓` done, `✗` failed,
  and after the arrow the files the call changed. Every call that changes
  something is a **step** you can look at and go back to (see
  [Timeline](#timeline-steps-going-back-and-forking)).
- **What each call printed**, such as a command's output, under its line:
  - it streams while the command runs;
  - it is open while running or when the call failed, and closed once it
    went well (click the line to open or close it);
  - terminal colours and cursor moves are taken out, and the last 64 KB
    of each call's output are kept.
- **Notes**: the agent's plan, permissions it was given, a server Kule
  stopped, pictures an agent could not take.
- **The end of the run**: *Done*, *Stopped: …* with the reason, or *The
  run failed: …*, with the number of operations the sandbox refused.
  Where Kule knows why a refusal keeps a tool from working, it says so
  under that line: a program that wanted the screen (LibreOffice, even
  with `--headless`) cannot run in a task, and macOS may report that it
  quit unexpectedly. The agent is told the same at its next prompt, in
  Kule's words, once.

While a run goes on:

- **The bar under the prompt** shows what it takes of the computer,
  updated every two seconds, against the task's limits, for example
  `CPU 140% / 200% · 1.2 GB / 4 GB · disk +350 MB · 7 processes`. It
  turns amber near a limit and adds *slowed* while the processor limit
  holds the run back. Click it to change the task's limits.
- **Stop** (Mod+Shift+K) takes the place of Run. It cancels the agent's
  turn as you would in the agent itself. The button reads *Stopping…*
  until the run ends; an agent that does not end its turn within ten
  seconds is killed. Whatever it changed so far stays in the copy for you
  to review.

Live keeps a task's latest events in the daemon, so a window opened later
shows what happened meanwhile.

## Questions: when the agent wants to connect somewhere

When the agent's program connects to a host the policy does not let
through on its own, the connection waits and a card shows on the task's
Live panel. The task also moves under *Needs you*. The card says the host
and port, the rule that made it wait, and a countdown to when it is
refused:

- **Approve** (Mod+Shift+Y) lets this connection through.
- **Approve and remember** lets this host through on this folder from now
  on, without asking. *Settings → Rules* lists what you told Kule to
  remember, and forgets a rule.
- **Deny** (Mod+Shift+N) refuses it; the agent goes on without it.

The time a question waits is the task's **waits** setting under the prompt.

## Review and commit

Review (Mod+2) shows the task's uncommitted changes and the policy's
ruling on them:

- **The ruling** at the top: all pass, some wait for your approval, or
  some are denied, and which rule said so.
- **Created, modified, deleted** files, and moves told apart from
  deletions. Inside a file, Review names what changed: the methods of a
  class, the sections of a document, the keys of a settings file. A
  picture shows before and after.
- **Ignored paths**: files the folder's `.gitignore` excludes (build
  output, caches, `.env`) stay in the copy. Tick *Also commit these* to
  take them too.
- **Writes in your home**: an agent that changed `~/.zshrc`, ran
  `git config --global` or wrote under `~/.config` did not reach your home.
  Its writes wait here, apart from the commit:
  - *Show* gives a line diff;
  - *Apply* takes one change to your real home;
  - *Discard* throws it away;
  - *Undo* takes the last apply back.

  A file that runs at every start (`.zshrc`, `.profile`, `.gitconfig`,
  autostart entries) is marked and opens its diff right away.
- **Where tasks meet**: other tasks on the same folder that changed the
  same files, down to the method or section.
- **Changed in the folder**: what the folder changed since the copy was
  made or last brought up to date, such as your own edits or another
  task's commit. The agent works on the folder as it was until
  *Bring into the copy* (Mod+Shift+G, also in the task's menu) brings
  these in:
  - a file only the folder changed comes in as it is;
  - a file the task changed too is merged by method, section or key, and
    stays the task's change;
  - when any of them does not merge, nothing comes in, and Kule names
    what both changed;
  - settings files such as `.env` come in masked, like the rest of the
    copy;
  - steps start over, as after a commit, and the agent hears what came
    in at the start of its next turn.

  It waits while an agent is at work. Programs still running in the
  task's sandbox are stopped first, as before a commit. Your folder is
  only read.

The button follows the ruling:

- **Commit** when everything passes.
- **Approve and commit** when something waits for you.
- Denied changes are held back even when you approve.
- **Roll back** throws the changes away.

A file two tasks changed is merged method by method; only the same part
changed in both is a conflict.

## Timeline: steps, going back and forking

Timeline (Mod+3) is the task's history:

- **Commits**, newest first, each with **Undo** for the newest one. Every
  commit and undo is an entry in a signed history (`kule history TASK`).
- **Steps**: the uncommitted tool calls, each with the files it changed.
- **Connections** the task's commands made, and whether each was let
  through.
- **Refusals**: what the sandbox refused the commands (macOS reports them;
  on Linux the command sees the error).

Two actions on every step, and on "before the first step":

- **Go back here** puts the copy back as it was after that step. Going back
  is itself a step, so you can go back on it. Changes made since the last
  step are kept as a step first, so nothing is lost. A step that cannot be
  gone back to (a file over 64 MB, more than 1 GB in one go, or a file
  with a second name) says why. `.git` is not put back; a commit starts
  the steps over.
- **Fork from here** makes a **new task** on the same folder that starts
  from this one's copy as it was after that step. *Fork task* in the task's
  ⋮ menu (Mod+Shift+D) forks from the copy as it is now. The fork takes the
  task's name with "(fork)", its colour, group, budget and secret
  stand-ins, and has no steps or commits of its own. Its changes reach
  the folder through its own review and commit.

Fork when you want to try another direction without losing this one, or
run two agents on two ideas and keep the better result.

## Files and Preview

**Files** (Mod+Shift+E) shows the task's copy as a tree:

- each file and folder has its icon, as VS Code's Material Icon Theme draws
  it (a Rust file, a `package.json`, a `src` folder, …);
- files the task changed are marked.

Click a file to open it in **Preview** (Mod+4), as it is now in the copy:

| Kind | How it shows |
| --- | --- |
| Text and code | With line numbers and colours, as in an editor (53 languages, up to 300 KB coloured, 1 MB shown). |
| JSON or XML packed onto a line or two | *Formatted* lays it out. |
| Markdown | Laid out, or as its source. |
| PNG, JPEG, GIF, WebP | Fitted to the panel or at 100%. |
| CSV and Excel (`.xlsx`) | A grid from A1, each sheet a tab; a bar shows a picked cell's value or formula. Formulas are not computed: a cell shows the value Excel saved with it. |
| Word (`.docx`) | Headings, paragraphs, numbered lists and tables, to read. Formatting, pictures and notes are left out. |
| Anything else | Its size and why it is not shown. |

The colours follow the theme in use. Files and Preview never follow a link
or leave the copy.

### Outside Kule

Right-click a file or folder in Files, or use the commands:

| | Mac | Linux |
| --- | --- | --- |
| **Open with its app** (the picked file) | ⌘O | Ctrl+O |
| **Show in Finder / Files** (the picked file, or the copy) | ⌥⌘R | Ctrl+Alt+R |
| **Copy path** (where it is on disk) | ⌥⌘C | Ctrl+Alt+C |
| **Open the copy in your editor** (task's ⋮ menu) | ⇧⌘O | Ctrl+Shift+O |

Where the Preview does not show a file (a PDF, a film, a slide deck), it
offers *Open with its app* and *Show in Finder*; above a Word or Excel
file, which it shows only in part, *Open with its app* is there too. The
button at the top of Files shows the whole copy.

- **Open with its app** opens documents, pictures, sound and video: PDF,
  Word, Excel, PowerPoint, OpenDocument, Pages, Numbers, Keynote, EPUB,
  RTF, CSV, plain text, the common picture, sound and video kinds. Never a
  program, a script, a web page or SVG, a link file or an Office file with
  macros: the agent wrote them, and they would run on your computer,
  outside the sandbox. A file must also be what its name says: a script
  named `notes.pdf` is not opened. It waits while the agent is at work.
- **Show in Finder** points at the file in its folder and never opens it,
  so a folder named like an app is not started.
- **Open the copy in your editor**: VS Code, Cursor or Zed, the ones your
  computer has; the menu lists each, the one you used last first. The
  first time for each, Kule says what an editor runs by itself from a
  folder (tasks, settings, recommended extensions, git hooks) and how to
  keep it from doing so: in VS Code answer *no* when it asks whether you
  trust the authors (Restricted Mode); Cursor does not ask unless
  Workspace Trust is on in its settings. Refused, as the terminal is,
  while the task changed the copy's git config or hooks. What you change
  there shows in Review like the agent's work.

From a terminal, `kule open TASK FILE` opens a file the same way, and
`kule path TASK [FILE]` prints where the copy or a file in it is.

## Asking the agent about a place in a file

In Preview, point at what you mean instead of describing it:

- **pick some text** (lines of code, a paragraph, a Markdown section);
- **drag over cells** in a sheet;
- **drag over a region** of a picture.

An **Ask** button shows beside what you picked (Mod+Shift+A does the
same). To ask about the whole file, use *Ask about it* above the Preview,
or Mod+Shift+A with nothing picked. The box that opens offers two things:

- **Ask now**: write the question and the agent gets it at once.
- **Add to prompt**: the place becomes a chip over the prompt line, so you
  can collect several places (up to 20) and write one prompt about them.
  Chips stay with their task until the prompt is sent.

The agent gets the places as a numbered list after your prompt, each
named so it can find it:

- lines of a file;
- the source lines and headings of a Markdown section;
- a range of cells and the values in it;
- a region in the picture's own pixels;
- a numbered paragraph of a Word document.

An agent that takes pictures also gets each picked region as a picture
(three to a prompt); one that does not gets where the region is, and Live
says so. Live shows a prompt's places as chips over it.

## Servers the task runs

When something in the task's copy listens on a port (the agent's
`npm run dev`, or a server you started in the task's terminal), the task
header shows a chip such as `localhost:5173 ↗`. Click it to open the
server in your browser.

Servers a task starts listen on this computer only: nothing on your
network (a café's Wi-Fi, the office LAN) reaches what an agent runs.

- On **Linux**, Kule turns every listening address into loopback, whatever
  the program.
- On **macOS**:
  - Node, Python and Ruby servers are pointed at loopback when they ask
    for every address;
  - for any other program (Go, Rust, Java), Kule **shields** the port while
    the run goes on, and the chip shows 🛡;
  - a server that listens on a network address of its own is stopped, and
    Live says why.

The chips also say what needs your attention:

- **Amber with a warning**: a server the network can still reach,
  typically one you started yourself in a terminal; Kule leaves yours
  alone.
- **Greyed out** (Linux): a server inside the sandbox's own network,
  which this computer cannot reach.

A server the agent started ends with its run; one you started in a
terminal stays.

## Keeping runs in check: budgets, limits and Stop

### Three scopes: every task, a group, one task

Budgets and limits can each be set at three scopes. *Settings → Budget*
and *Settings → Limits* show one scope at a time: pick it under
**Applies to** (**Every task**, **One group** or **One task**, then which
one). The card below names the scope in capitals (EVERY TASK, GROUP, THIS
TASK ONLY), has an edge in its colour (the accent for every task, the
group's or the task's own colour) and says whom what you set there holds,
so you always know what you are changing. Each change is confirmed with
the scope's name ("Group clients: …", "notes only: …"). Under the card,
*Where budgets are set* and *Groups and tasks with limits of their own*
list every scope that has something set; click one to open it.

The ⋮ menus open the right scope: *Budget…* and *Limits…* in a task's
menu open that task, the same items in a group's menu open the group.

Moving a task into or out of a group asks first when its limits or group
budgets would change, and says which ("Its limits: memory 8 GB → 2 GB").

### Budgets

- **Every task**: what all tasks may cost together in any 24 hours.
- **A group**: what the group's tasks may cost together, over all of their
  runs, and over any 24 hours; each is optional. A run counts for the group
  its task was in when it ran, so moving a task out keeps what it spent in
  the group's account.
- **One task**: what that task's runs may cost together.

Each budget holds: no run starts once one is spent, and a run is held to
the tightest of what is left. On a task's scope, *What holds its next run*
lists each budget over it and marks the one that holds. The costs are
those the agents report:

- Claude Code reports the cost as each turn ends, so a turn can go over
  and the next run is refused.
- Agents signed in with a subscription may report tokens only. A budget
  does not hold what is not reported.

The header's chip shows what the task spent and what is left. When the
window is not in front, a notice tells you at 80% and when a budget is
spent: the task's own, its group's (all runs, and the last 24 hours) and
the daily one for every task, each named ("Group clients spent 80% of its
daily budget"). Budgets already past these when the app starts are not
told again; a 24-hour budget that drops below 80% as older runs fall out
of the day is told again when it nears.

### Limits

*Settings → Limits*, *Limits…* in a task's or a group's ⋮ menu, or a click
on the measure bar in Live.

- **Every task**: the limits that hold where a group and a task set none.
- **A group**: field by field, **From every task** (the default), **No
  limit**, or **The group's own** value, for each of its tasks.
- **One task**: field by field, what it inherits (**From group clients**
  when its group sets that field, else **From every task**), **No limit**,
  or **Its own** value.

A task's own value stands over its group's, and a group's over every
task's. Each limit below is one task's, also when a group sets it: a group
sets the processor limit for each of its tasks, not for all of them
together.

| Limit | Unit | Over it |
| --- | --- | --- |
| **Processor** | cores (1.5 = one and a half) | The run is **slowed down**, not stopped: its processes are paused and resumed in turn. |
| **Memory** | GB, all the run's processes together | The processes are **killed at once**; the run stops. |
| **Disk** | GB the copy, its temporary files and home changes may grow by since the task was made | The run **stops**. No single file can be written larger than this. |
| **GPU** | may be used / off | Off: Metal (and OpenCL) gets no device on macOS; the GPU's devices stay read-only on Linux. |
| **One run** | minutes | The run **stops**. |
| **All of a task's runs** | hours, added up | The run **stops**, and no new run starts until you raise it. |
| **Doing nothing** | minutes | The run **stops**. |

Some notes on how they count:

- **Doing nothing** means no message, tool call or output from the agent,
  no processor use and no new process. A long, quiet build is not doing
  nothing. A question waiting on you does not count either.
- **One run** counts all of the run's time, including time spent waiting
  for your answers.
- **Disk** counts only what the task added, so a large folder does not use
  up its own limit. Nor do the package caches a task starts with (npm's
  and cargo's start as clones of yours, which take no room of their own);
  what its commands add to them counts.
- A run keeps the limits it started with; a change holds from the next run.
- `kule run` from a terminal is held to the same limits.

When a limit stops a run, Live says which one and what it measured
("Stopped: the run's processes held 4.2 GB, over its memory limit of
4 GB"). If the task is not the one on screen, a notice says so.

### Stop

**Stop** in Live (Mod+Shift+K) stops the run at work whenever you want. See
[Watching a run](#watching-a-run-live).

## Disk space: what a task really takes

Finder, `du` and disk-usage tools can show Kule's home (`~/.kule`, or
`KULE_HOME`) as many gigabytes. They count every copy at its full size.
On macOS most of it is shared with your own files and takes no space of
its own; on Linux that depends on the filesystem.

A task keeps everything under `<home>/tasks/<id>/`:

| Folder | What it is |
| --- | --- |
| `w/<folder name>` | The copy the agent works in. |
| `base/` | The folder as it was at the last sync, for merging. |
| `tmp/` | The task's temporary files; `tmp/caches/` holds its own package caches (npm, cargo, pip, Go and others). |
| `home/` | The home layer: what the task's commands see as your home. |
| `steps-store/`, `checkpoints/` | What going back to a step and undo need. |

### On macOS

The folder and Kule's home are on APFS, and Kule makes **clones**: real
files that share their disk blocks with the originals. A clone takes
space only for what changes in it afterwards. These are clones:

- the copy and `base/`;
- `tmp/caches/npm` and `tmp/caches/cargo`, which start as clones of your
  `~/.npm` and `~/.cargo` caches: a task's npm cache shows as large as
  yours, and takes nothing until the task downloads a package yours does
  not have;
- in the home layer, `.config` and the small files at the top of your
  home (`.zshrc`, `.gitconfig`).

The rest of the home layer is **symbolic links** to your own files and
folders, which take no space at all.

To see what is really used, look at the free space: `df -h`, or
System Settings → General → Storage. Cloning a 500 MB file twice, for
example, makes `du` show 1.5 GB, and the free space does not change.

Two things to know:

- Clones are made only on the same volume. A folder on another disk
  (an external drive, say) is copied in full, and the caches start empty.
- A clone keeps its blocks when the original goes. If you delete
  `~/.npm`, its space comes back once the tasks that cloned it are
  deleted too.

### On Linux

Linux has no call that clones a folder in one go, so Kule copies file by
file, and what that costs depends on the filesystem:

- **Btrfs and XFS** (XFS made with reflink support, the default for some
  years) share blocks the way APFS clones do: the copy takes space only
  for what changes. `du` counts the shared blocks in full here too; ask
  the filesystem (`btrfs filesystem du`, or the free space in `df -h`).
- **ext4**, the default on many distributions, cannot share blocks. The
  copy is a full copy and takes as much space as the folder, `.git` and
  `node_modules` included, and `base/` holds a full copy of the folder's
  files up to 1 MiB (outside `.git`, `node_modules`, `target` and the
  like). Here `du` tells the truth.

Everywhere on Linux, the package caches in `tmp/caches/` start empty: a
task's first install downloads again, through Kule's proxy, and keeps
what it downloads. In the home layer, `.config` and the small files at
the top of your home are copied (shared where the filesystem allows), and
the rest are symbolic links.

### What takes space, everywhere

- Files the agent changes or creates in the copy, build output included.
- Packages a task downloads that are not in its caches yet.
- Steps and checkpoints: what files held before a step or a commit
  changed them.

A task's **disk** limit counts only what the task added since it was
made (see [Limits](#limits)). Deleting a task (the ⋮ menu, or
`kule rm TASK`) moves it to `<home>/trash`, which Kule empties in the
background right away and again whenever the daemon starts; then its
space comes back.

## Organising tasks

- **Names**: a task shows its folder's name and yours after it
  (`Downloads – march invoices`). *Rename…* in its ⋮ menu (F2).
- **Colours**: the swatches in the ⋮ menu (Mod+. opens it for the current
  task; a right click works too).
- **Order**: drag a task, or Alt+Shift+↑/↓ (Alt+↑/↓ with the list
  focused).
- **Groups**: the + next to *Tasks* makes a group with a name and colour.
  Drag a task onto it, or use *Group…* in the task's menu. A group can
  have a **policy of its own**:
  - the one for every task (the default);
  - Kule's balanced or careful one;
  - one you write for the group in *Settings → Policy*.

  When moving a task would change the verdict on any of its pending
  changes, Kule lists them and asks first.
- **Fork** and **delete** are in the ⋮ menu as well. Deleting a task never
  touches your folder.

Ctrl+Tab and Ctrl+Shift+Tab move between tasks.

## Terminals

*Open terminal* in a task's menu (Mod+T), or the Terminal panel
(Ctrl+\`), opens your own shell in the task's copy. Ctrl+Shift+\` opens
another; each is a tab.

The terminal is yours, outside the sandbox. What you change there is
reviewed and committed like the agent's work. Inside it:

- ⌘F (Ctrl+Shift+F) finds text;
- ⌘/Ctrl+click opens a link;
- on Linux and Windows only Ctrl+Shift shortcuts and function keys leave
  the shell, so Ctrl+L and the rest stay the shell's.

Kule refuses to open a terminal while the copy's git config or hooks were
changed by the task: your prompt's `git status` would run them outside
the sandbox. Review and commit or roll them back first.

## Settings, section by section

Open Settings with Mod+, or the gear at the bottom left.

- **Agents**: the agents Kule can run.
  - Kule's own (Claude Code, Codex, Gemini) and any you add, from the
    [ACP registry](https://agentclientprotocol.com/registry) or by hand.
  - An agent you add runs only once you approve what it opens (its own
    folders, hosts and keys), and again after any change.
  - **Try it** runs a draft once in an empty folder and suggests what it
    was refused, as checkboxes.
- **Policy**: balanced or careful for every task, and a policy for each
  group. A repository can add refusals of its own in
  `.kule/policy.cedar`. Where this computer cannot give agents all of
  their isolation (Linux without unprivileged user namespaces), Policy
  lists what is missing, and no agent runs until you tick *Run agents
  with what this computer gives*; the title bar then says *Weaker
  isolation* while it holds.
- **Rules**: the approvals you told Kule to remember (a connection's
  *Approve and remember*, or *Don't ask again in this folder* on a held
  file in Review, which first shows what it would have let through), each with
  *Forget*.
- **Secrets**: API keys a task's commands use without ever seeing them.
  - The agent sees a placeholder; the proxy puts the real value into
    requests to the hosts you name, and only those.
  - Values in the copy's `.env` files that look like secrets are masked
    the same way.
- **Budget**: every task's daily budget, a group's and a task's, one scope
  at a time (see [Budgets](#budgets)).
- **Limits**: every task's limits, a group's and a task's own, one scope at
  a time (see [Limits](#limits)).
- **Appearance**: Kule's themes and your own.
  - Kule's themes: System (follows the computer), Light, Dark, Coral,
    Nord, Solarized Light and two high-contrast ones.
  - Make a theme from any of them with a live preview; Kule warns about
    colour pairs too faint to read.
  - Themes go to and come from `.kule-theme.json` files.
  - **Motion**: the window moves a little where something changes. Menus
    grow out of what opened them, the tab line slides, a new event rises
    into Live, a question opens and pulses twice, task rows slide to their
    new places, a run that ends well draws its tick. *As the computer says*
    (the default) turns it off when the computer's "reduce motion" is on;
    *On* and *Off* decide for themselves. The command *Turn motion on or
    off* can have a shortcut.
  - **Fonts**: one for the window's text, one for code (the agent chat,
    terminals, diffs and the Preview's code), each from the fonts on your
    computer and in a size of its own: text 11 to 20 px (Kule's is 14),
    code 10 to 22 px (13). The code list holds the fonts whose letters are
    all one width; you can type any font's name. *Kule's fonts* puts both
    back. *Make the text larger* (Mod+=) and *smaller* (Mod+-) change both
    sizes at once. A font a profile brings that this computer lacks falls
    back to Kule's own, and the import says so; one removed later is
    marked here, and the window uses Kule's meanwhile.
  - **Copying**: text you select with the mouse in the agent chat,
    Review or a terminal goes to the clipboard at once, and a small
    *Copied* shows by the pointer. Fields you type in copy as usual. Turn
    it off here, or with the command *Turn copying selected text on or
    off*, which can have a shortcut.
- **Shortcuts**: every command and its keys. Set your own, search them, or
  *Put back every default*.
- **Extensions**: install, approve, turn on or off and remove extensions
  (see [docs/extensions.md](extensions.md)), and approve the MCP servers
  they offer agents.
- **Profile**: your choices in one `.kule-profile.json` file to take to
  another computer.
  - It can hold your themes, shortcuts, policy choice, daily budget,
    limits and agents; you tick the parts.
  - On import Kule shows what the file holds and you pick the parts to
    apply.
  - Secrets, remembered approvals and a policy you wrote never go into a
    profile.

## Keyboard

Every action is a command with a shortcut you can change in *Settings →
Shortcuts*; those with none can be given one. **Mod+K** finds any command
by name, **Mod+/** lists the shortcuts. Kule's defaults:

| Action | Keys |
| --- | --- |
| All commands | Mod+K |
| Keyboard shortcuts | Mod+/ |
| Settings | Mod+, |
| New task | Mod+N |
| Next / previous task | Ctrl+Tab / Ctrl+Shift+Tab |
| Move the task up / down | Alt+Shift+↑ / Alt+Shift+↓ |
| Task actions (⋮ menu) | Mod+. |
| Rename the task | F2 |
| Open a terminal in the copy | Mod+T |
| Fork the task | Mod+Shift+D |
| Bring the folder's changes into the copy | Mod+Shift+G |
| Delete the task (press twice) | Mod+Backspace |
| Show Tasks / Files | Mod+0 / Mod+Shift+E |
| Show the agent chat / Review / Timeline / Preview | Mod+1 / Mod+2 / Mod+3 / Mod+4 |
| Show or hide the agent chat | Mod+Shift+J |
| Show or hide the terminal / new terminal | Ctrl+\` / Ctrl+Shift+\` |
| Show or hide the left side / bottom / right side | Mod+B / Mod+J / Mod+Alt+B |
| Write to the agent | Mod+L |
| Run the prompt | Enter or Mod+Enter (Shift+Enter: new line) |
| Stop the agent at work | Mod+Shift+K |
| Ask the agent about what is picked in Preview | Mod+Shift+A |
| Commit, or approve and commit | Mod+Shift+Enter |
| Roll back (press twice) | Mod+Shift+Backspace |
| Undo the last commit (press twice) | Mod+Shift+U |
| Approve / deny the waiting connection | Mod+Shift+Y / Mod+Shift+N |
| Make the text larger / smaller | Mod+= / Mod+- |

Commands without a default include the task's budget and limits, how long
the agent waits, groups and group policies, the two policies, themes,
Kule's fonts again, copying selected text, profile export and import,
and the terminal's next, previous, clear and close. Combos the system or
text editing owns (⌘Q, copy and paste, …) are never taken, and in a text
field only the shortcuts meant for it fire.

The menu bar (File, Edit, View, Task, Help) runs the same commands and
shows the keys you set.

## When the window is closed

The window is a view onto the **Kule daemon**, which the app starts when
none is running. The daemon owns the tasks, the agents' runs and the
questions they wait on:

- Closing the window stops nothing; a run goes on, and its questions wait
  for their time.
- Reopening the window shows what happened meanwhile.
- The daemon exits after ten minutes with nothing going on.

Kule keeps everything in its home, `~/.kule` (or `KULE_HOME`): tasks and
their copies, settings, the policy, secrets. No sandbox can see it.

## The same from a terminal

The `kule` command does most of what the app does, on the same tasks:

| | |
| --- | --- |
| `kule new ~/notes` | Make a task. |
| `kule list` | List tasks, in the app's order. |
| `kule run TASK --agent claude-code "prompt"` | Give an agent a prompt, held to its budgets (its own, its group's, every task's) and limits. |
| `kule exec TASK -- npm test` | Run a command in the sandbox, then show what it changed. |
| `kule shell TASK` | Your shell in the copy. |
| `kule path TASK [FILE]` | Where the copy, or a file in it, is on disk: `cd "$(kule path TASK)"`, `code "$(kule path TASK)"`. Warns when git there would run what the task changed. |
| `kule open TASK FILE` | Open a file of the copy with its app, as *Open with its app* does (`--dry-run` checks it and prints where it is). |
| `kule status TASK` / `kule steps TASK` | What changed, and the policy's ruling / each tool call's changes. |
| `kule commit TASK [--approve]` | Commit; `--approve` for changes that wait for you. |
| `kule rollback TASK` / `kule undo TASK` | Throw the changes away / take the last commit back. |
| `kule sync TASK` | Bring what the folder changed meanwhile into the copy. |
| `kule history TASK` | The signed history, checked. |
| `kule budget TASK 5` / `kule group budget ID 20 [--daily]` / `kule daily-budget 20` | One task's, a group's (all runs, or any 24 hours) and every task's budget (`off` takes one away). `kule budget TASK` lists every budget over the task's next run. |
| `kule limits [--group ID \| --task TASK] memory=4 gpu=off` | Every task's, a group's or one task's limits; each line says which scope a limit comes from (`none` for no limit, `inherit` for the one above). |
| `kule name`, `kule color`, `kule group` | Names, colours, groups and group policies. |
| `kule home TASK diff\|apply\|discard\|undo` | Writes the task made in your home. |
| `kule secrets add NAME --host HOST` | Keep a secret for a host. |
| `kule agents …`, `kule ext …`, `kule policy …`, `kule rules` | Agents, extensions, the policy, remembered rules. |

`kule help COMMAND` says more about each.

## What is new

What came after 0.2.0 (see [CHANGELOG.md](../CHANGELOG.md) for the full
list):

- **Live command output**: what each tool call prints streams under its
  line as it runs. → [Live](#watching-a-run-live)
- **Go back to a step, and fork**: put the copy back as it was after any
  step, or start a new task from there. →
  [Timeline](#timeline-steps-going-back-and-forking)
- **Servers in the copy**: chips in the header open them in the browser,
  and what an agent runs listens on this computer only. →
  [Servers](#servers-the-task-runs)
- **Preview**: text, code, Markdown, pictures, CSV, Excel and Word files,
  shown large. → [Files and Preview](#files-and-preview)
- **Ask about a place**: pick text, cells or part of a picture and ask
  the agent about it. →
  [Asking the agent](#asking-the-agent-about-a-place-in-a-file)
- **File icons and colours**: VS Code's file icons in Files, editor colours
  in Preview and in the agent's code blocks.
- **Limits and Stop**: processor, memory, disk, GPU and time limits for
  every task and for each, a measure bar in Live, and a Stop button. →
  [Keeping runs in check](#keeping-runs-in-check-budgets-limits-and-stop)
- **Group budgets and limits**: a group's tasks share a budget (all runs,
  and any 24 hours) and take its limits; Settings shows every task, a
  group or one task at a time, and `kule limits` / `kule group budget` do
  the same from a terminal. →
  [Three scopes](#three-scopes-every-task-a-group-one-task)
- **Extensions**: commands, panels, themes, settings, agent profiles and
  MCP servers from `.kulext` files, each approved as it stands. →
  [docs/extensions.md](extensions.md)
