---
title: Writing a Kule extension
sidebar_label: Extensions
description: Add commands, panels, themes, settings, review notes, agent profiles and MCP servers to Kule with a sandboxed JavaScript extension.
---

# Writing a Kule extension

:::note A preview

The extension format (`"kule_extension": 1`) and the SDK may still change
before Kule 1.0. Any change that breaks extensions will be named in the
[changelog](https://github.com/kulehq/kule/blob/main/CHANGELOG.md), with what
to do about it. Tell us what gets in your way:
[issues](https://github.com/kulehq/kule/issues).

:::

To install, approve and manage extensions from a terminal, see
[`kule ext`](cli/ext.md). In the app, use **Settings → Extensions**
(see [Settings](app/settings.md)).

An extension adds to Kule: commands, panels, themes, settings, notes on a
review, agent profiles and MCP servers for agents. It is plain JavaScript and
HTML in a folder, shared as a `.kulext` file.

Kule treats an extension the way it treats an agent: as code it does not
trust. The extension asks for permissions from a closed list. The person
approves exactly the version they saw, and any change waits for them again.
Its code runs in sandboxed frames that can reach nothing but Kule, and Kule
answers only the calls those permissions cover.

## Start

```sh
kule ext new me.hello        # a folder: manifest, main script, panel, types
kule ext dev hello           # install it from the folder, read live
kule ext approve me.hello    # let it run
```

Open the panel from the command palette (`Show Hello`), or drag its tab where
you want it. Its digest covers every file in the folder that a package of it
would carry, so any change, to the code or to `kule-extension.json`, waits
for approval again (`kule ext approve`, or the card's **Approve the
change**) before it runs.

`kule ext approve` shows what it approves and asks first; a script passes
`--yes`. With the app open, `kule ext` makes its changes through the daemon,
so the app hears of them at once.

The folder must be somewhere only you can write. Kule refuses folders an
agent could rewrite: those under `/tmp`, `/var/tmp`, `/dev/shm`, Kule's home,
an agent's own directories, or a folder a task works on (an agent's work
reaches it with a commit). A task made later on a folder that holds one
holds the extension from then on, and its card says why.

To share it:

```sh
kule ext pack hello          # me.hello-0.1.0.kulext
kule ext info me.hello-0.1.0.kulext
kule ext install me.hello-0.1.0.kulext
```

The desktop app does the same under **Settings → Extensions**.

[`examples/extensions/`](https://github.com/kulehq/kule/tree/main/examples/extensions)
has three to read:

| Example | What it shows |
| --- | --- |
| `hello` | A panel, a command, a setting and a theme |
| `lockfile-notes` | A review helper: it notes changed lockfiles |
| `webhook-notify` | Watching tasks and posting to a webhook |

## Language and tools

An extension runs in the window's web engine (WebKit on macOS, WebKitGTK on
Linux), not in Node: modern JavaScript, the DOM and CSS. There is no
`require`, no file system and no network but what the SDK offers.

- **Scripts:** classic `<script src="…">` and ES modules (`<script
  type="module">`, `import "./util.js"`) both work. Imports come from the
  extension's own files only.
- **TypeScript:** compile to `.js` (with `tsc` or `esbuild`) and ship the
  `.js`. For types, add `kule-ext.d.ts` (which `kule ext new` writes) to your
  `tsconfig.json`, or put `/// <reference path="kule-ext.d.ts" />` at the
  top of a file. A `.ts` file may travel in the package, but it does not run.
- **npm packages:** a frame has no network and no bare imports, so `import x
  from "lodash"` fails. Bundle them into a file of your own, e.g. `esbuild
  src/main.ts --bundle --format=esm --outfile=main.js`.
- **No inline code:** the page's policy allows scripts and styles from the
  extension's files only. So there is no inline `<script>`, no `style="…"`
  attribute and no `eval` or `new Function`. Setting `element.style` from a
  script is fine.
- **No browser storage:** `localStorage`, cookies and IndexedDB refuse an
  opaque origin. Use `kule.storage`.
- **Files:** html, js, css, json, png, svg, woff2, toml, md, txt, ts, or a
  name without an extension. Names hold letters, digits, `-`, `_` and `.`.
  Hidden files (`.git`, `.DS_Store`) are left out when packing.

## The manifest: `kule-extension.json`

```json
{
  "kule_extension": 1,
  "id": "acme.notes",
  "name": "Notes",
  "version": "1.0.0",
  "publisher": "acme",
  "description": "One sentence.",
  "main": "main.js",
  "permissions": ["tasks.read", "net"],
  "hosts": ["hooks.slack.com"],
  "contributes": {
    "commands": [{ "id": "scan", "title": "Scan", "keys": "Mod+Alt+S" }],
    "panels": [{ "id": "notes", "title": "Notes", "entry": "panel.html", "needsTask": true }],
    "configuration": [
      { "key": "level", "title": "Level", "type": "enum", "values": ["info", "warn"], "default": "warn" }
    ],
    "themes": ["dusk.kule-theme.json"],
    "agents": "agents.toml",
    "mcpServers": [{ "name": "db", "command": "npx", "args": ["-y", "@acme/db-mcp@1.2.3"] }]
  }
}
```

Kule checks every field. An unknown field or permission refuses the
extension; nothing is ignored.

| Field | Rules |
| --- | --- |
| `id` | `publisher.name`, each part 1–20 of `a-z 0-9 -`; it starts with `publisher` |
| `publisher` | Shown as "not verified by anyone" |
| `name`, `description` | Text without control or text-direction characters (40 and 300 characters) |
| `version` | `X.Y.Z`, with an optional `-pre` or `+build` |
| `main` | A `.js` file that runs in a hidden frame while the app is open; needed for commands |
| `hosts` | Plain host names (`api.example.com`), with `net` and only with it |
| command and panel ids | 1–18 of `a-z 0-9 -`. A panel's id is also the id of the command that shows it, so a command and a panel cannot share one |
| `configuration` | Up to 32 settings of type `string`, `boolean`, `number` or `enum`. Kule draws the form and keeps the values |

Paths in the manifest are plain: letters, digits, `-`, `_` and `.`, with no
`..` and no hidden names.

## Permissions

| Permission | Lets the extension |
| --- | --- |
| `tasks.read` | See tasks: names, folders, how many changes, whether an agent works. Not budgets, spending or home changes |
| `events.read` | Hear what agents say and do while they run, and when tasks are created, committed, rolled back, undone or removed |
| `questions.read` | See the questions waiting for the person. It cannot answer them |
| `review.read` | Read a task's changes and the policy's verdicts. Not what waits in the person's home |
| `review.annotate` | Add notes to a task's review. Notes only |
| `files.read` | Read files in a task's copy |
| `storage` | Keep up to 1 MiB of JSON of its own |
| `notify` | Show notifications, named for the extension, 5 a minute |
| `net` | `fetch` over https to the listed `hosts` |

No permission reaches any of these:
- secrets;
- answering a question;
- commit, roll back or undo;
- policies, rules, budgets or settings files;
- agent approvals;
- the terminal;
- the daemon's token.

The approval screen warns when an extension can both read the person's work
and send data out. It warns too when it can do so together with another
extension that runs: one reads, the other sends ("Together with acme.sender,
it can read your work and send data"). Approving it approves the pair; a
pair nobody approved (each approved while the other was off, say) holds both
until one of them is approved beside the other.

## The SDK

Every page of an extension loads Kule's SDK from beside it:

```html
<script src="_kule/sdk.js"></script>
```

`kule-ext.d.ts`, which `kule ext new` writes, has the types. Every call
returns a promise. A refused call rejects with an `Error` that says why,
such as `files.read needs the files.read permission, which Notes was not
given`.

```js
await kule.ready;                          // connected
kule.extension;                            // { id, name, version }
kule.context;                              // { main: true } or { panel, task }

kule.commands.register("scan", (args) => { /* … */ });
await kule.commands.execute("ext.acme.notes.scan");
await kule.commands.execute("view.files"); // a few of Kule's that only show a panel (below)

kule.events.on("task", ({ task, change }) => { /* … */ });

await kule.tasks.list();                   // tasks.read
await kule.tasks.get(id);
await kule.review.get(task);               // review.read
await kule.review.annotate(task, [{ path, level: "warn", text }]); // review.annotate
await kule.files.list(task, "src");        // files.read
await kule.files.read(task, "README.md");
await kule.storage.get();                  // storage
await kule.storage.set({ seen: 3 });
await kule.config.get();                   // the extension's own settings
await kule.ui.notify("Title", "Body");     // notify
await kule.net.fetch("https://hooks.slack.com/…", { method: "POST", headers, body }); // net
```

### Events

| Event | Data | Needs |
| --- | --- | --- |
| `run` | `{ task, seq, event }`: an agent's events | `events.read` |
| `task` | `{ task, change }`: `created`, `committed`, `rolled_back`, `undone`, `removed`, `went_back`, `synced` | `events.read` |
| `questions` | The questions waiting now. Each says what kind of connection it is (`inside`: `null` for a tunnel, or `{ read_only, fills_secrets }`), never the names of the secrets Kule would fill | `questions.read` |
| `review` | `{ task, digest }`: a review was drawn with new changes | `review.read` |
| `task-selected` | `{ task }`: the person chose another task | `tasks.read` |
| `theme` | `{ scheme, colors }`. The SDK already set `--kule-<part>` CSS variables (`--kule-bg`, `--kule-text`, `--kule-accent`, …) | — |
| `config` | `{ key, value }`: the person changed a setting | — |

Events arrive only while the desktop app is open.

## Lifecycle

- **When the code runs:** an extension runs while the desktop app is open
  and the extension is on and approved as it is. Nothing runs while the app
  is closed, and events from then are not replayed. While no extension may
  run in the window, the card of one that is on says *Does not run now*,
  with why: *Kule started without extensions this time (safe mode:
  --no-extensions or KULE_NO_EXTENSIONS)*, *extensions cannot be served: …*,
  or *extensions wait until the window's WebRTC is off*.
- **The main script:** it runs in a hidden frame from the app's start (or
  from the approval) until the extension is turned off, removed or changed.
  A change stops it until it is approved again.
- **Panels:** each panel's page loads at start too, even while its tab is
  hidden. Moving a panel to another region reloads its page.
- **Each load is fresh:** the page gets a new connection, and `kule.ready`
  resolves again. Keep what must last in `kule.storage`, not in variables.
  With `tasks.read`, `kule.context.task` is the task chosen when the page
  loaded (`null` without it); listen to `task-selected` for later ones.
- **While you write:** a folder installed with `kule ext dev` is read live,
  and each change waits for approval again. Once approved, its card in
  Settings → Extensions has **Reload**, which starts its frames again with
  the code as it is now.

## Debugging

- **Messages:** every frame's `console.log`, `info`, `warn` and `error`
  calls, uncaught errors and unhandled rejections go to the window. The
  extension's card in Settings → Extensions lists the last 100, newest
  first. The list opens by itself when one of them is an error.
- **Refused calls:** a refused call rejects with an `Error` that says why.
  Catch it and `console.error` it, and it shows there too.
- **Inspector:** in a development build of Kule (`cargo run -p
  kule-desktop`), Tauri enables WebKit's inspector. Right-click inside a
  panel and choose Inspect Element.
- **Before installing:** `kule ext info FOLDER` reads the manifest as Kule
  will, and `kule ext pack FOLDER` refuses whatever a package cannot carry.

## Contributions

- **Commands** appear in the palette and in Settings → Shortcuts as
  `ext.<id>.<command>`. A suggested key that Kule or another extension already
  uses is dropped; the person can give the command one.
- **Panels** sit in the window's regions like Kule's own; a new one goes to
  the right. Kule draws a line above each, naming the extension, which the
  frame cannot paint over.
- **Themes** are `.kule-theme.json` files, colours only, checked as Kule's
  own. They are listed in Appearance under "From extensions".
- **Notes on a review** appear under "From extensions" with the extension's
  name. They are plain text and cannot approve or refuse anything.
- **Agent profiles** (`agents.toml`, the format of `kule agents show
  claude-code`) go through the same checks as the person's own. Each waits in
  Settings → Agents (or `kule agents approve NAME`) until approved.
- **MCP servers** each wait until the person lets agents use it, in the
  extension's card or with `kule ext mcp approve EXT/NAME`. The card shows
  every variable's value.
  - A server starts one exact registry package and nothing else:
    `npx [-y] name@1.2.3 …` or `uvx name==1.2.3 …`.
  - Options before the package (`--with`, `--from`, `--package`,
    `--registry`, …), URLs, paths, git specs, aliases and extras are
    refused.
  - Variables that move a package manager or an interpreter (`NPM_*`,
    `UV_*`, `PIP_*`, `PYTHON*`, `NODE_*`, `GIT_*`, …) are refused too.
  - The agent starts the server inside its own sandbox and behind the
    task's proxy. The server can read what the agent can, the agent's
    sign-in included, and the policy asks about every host it reaches.

## What a frame can and cannot do

Kule serves an extension's files from `127.0.0.1` under a random key. Its
frames are sandboxed (`allow-scripts` only, an opaque origin) and their
content policy keeps them to their own files:
- no network (`connect-src 'none'`);
- no frames of their own;
- no forms;
- no `localStorage` or cookies (use `kule.storage`);
- no devices, clipboard or fullscreen (`Permissions-Policy`).

The window lets frames come from the extension server's exact port only, so
a frame that navigates itself cannot reach another server on the computer.
WebRTC is off in the window: the content policy cannot govern it. No
extension runs until it is off.

To Tauri a frame there is a remote origin, which reaches none of Kule's
commands. A frame talks to Kule only over the MessagePort it is handed when
it loads. What comes over that port belongs to the extension the port was
made for, whatever the frame says it is.

A frame can run only the extension's own commands and a short list of
Kule's that show a panel or a region. The list is `view.tasks`, `files`,
`live`, `review` and `timeline`, `chat.toggle`, and
`layout.left/main/bottom/right`. This holds both for `commands.execute` and
for a shortcut pressed inside the frame. After any of them, the keyboard
goes back to the frame. A frame cannot start the terminal, open the
palette, commit, roll back or delete.

The publisher `kule`, and names whose first word is "Kule", are Kule's own;
no extension may take them.

If an extension makes the window unusable, start the app with
`--no-extensions` (or `KULE_NO_EXTENSIONS=1`) and turn it off with `kule ext
disable ID`, which works without the app.

## The package

A `.kulext` is a zip that Kule never unpacks to disk. When a package is
installed, every entry is checked:
- a plain path of a kind extensions carry: html, js, css, json, png, svg,
  woff2, toml, md, txt, ts, or a name without an extension;
- no links and no encryption;
- the file list is consistent: no name twice, and the headers agree;
- at most 5 MiB per file and 40 MiB in all once inflated, 20 MiB packed, 500
  files.

Approval pins the blake3 of the whole package. Kule checks it again before
serving a file. `kule ext pack` makes the same bytes from the same folder, so
the digest the person approved names exactly what they read.

## Sharing and updating

There is no marketplace yet. You share the `.kulext` file itself (a
download, a release asset). The person installs it from Settings →
Extensions → Install from a file, or with `kule ext install FILE`.

- **The digest:** `kule ext pack` prints it. Publish it beside the file, so
  people can check it with `kule ext info FILE` before installing. Nobody
  verifies the publisher; the digest is what proves the file is yours.
- **Updating:** raise `version`, pack again, and share the new file.
  Installing it over the old one keeps the extension's storage and settings
  (a setting the new manifest no longer has is dropped). It holds the
  extension until the person approves again, and the approval names any
  permission or host that is new.
- **Identity:** the id is the extension's identity. A package with the same
  id is an update of it, whoever made it, so pick a publisher name that is
  yours.

## Limits at a glance

| What | Limit |
| --- | --- |
| Package | 20 MiB packed; 40 MiB inflated; 500 files; 5 MiB a file |
| Manifest | 64 KiB; 50 commands, 8 panels, 32 settings, 8 themes, 8 MCP servers, 16 hosts |
| A request from a frame | 1 MiB |
| Calls | 200 every 10 seconds per extension, keys and messages included |
| `storage` | 1 MiB of JSON |
| `ui.notify` | 5 a minute; title 80 characters, body 300 |
| `net.fetch` | 60 a minute; https on 443 to the listed hosts; 15 s; 1 MiB out, 4 MiB in; no redirects |
| Notes on a review | 50 per task per extension, 500 characters each |
| Messages kept | the last 100 per extension, 2,000 characters each |

## Where things are

`<KULE_HOME>/extensions/` holds:

| Path | What it holds |
| --- | --- |
| `installed.json` | What is installed, on or off |
| `approved.json` | What the person approved |
| `mcp-approved.json` | The MCP servers agents may use |
| `packages/` | The packages |
| `data/<id>/storage.json` | What an extension keeps |
| `data/<id>/config.json` | Its settings |
| `data/<id>/network.jsonl` | One line per fetch: method, host, status, size. Never the path or the query |

Removing an extension removes all of it, its approvals included.
