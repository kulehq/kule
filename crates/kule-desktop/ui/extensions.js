// Kule's extension host, in the window.
//
// Each extension that may run (on, and approved as it is) gets sandboxed
// frames: a hidden one for its main script and one per panel. The frames
// are served from loopback by the app, sandboxed to an opaque origin, and
// reach Kule through one MessagePort each, handed over on every load. What
// arrives on a port belongs to the extension that port was made for; the
// frame's own words about who it is count for nothing.
//
// Calls go to the app's `ext_call`, which checks the approved permissions
// again; this file only adds what lives in the window (commands, notes on
// a review, keys) and the events the window hears.
"use strict";

const EXT = {
  /** Where extension files are served: `<prefix><id>/<path>`. */
  prefix: null,
  /** The daemon's list, with why any cannot run. */
  listed: [],
  /** id → { id, digest, manifest, perms, frames } of those running. */
  running: new Map(),
  /** task → (extension id → notes) for the review. */
  notes: new Map(),
  /** The themes running extensions bring, as the Appearance list takes them. */
  themes: [],
  /** The MCP servers running extensions offer, approved or not. */
  mcp: [],
  /** task → review digest last told, so a note never loops into a new review. */
  reviewTold: new Map(),
  /** The task the frames were last told about. */
  taskTold: undefined,
};

/** What each permission lets an extension do, as the person reads it
 * (the same words as `kule ext info`). */
const PERMISSION_WORDS = {
  "tasks.read": "See your tasks: their names, folders and state",
  "events.read": "Read what agents say and do while they run, and when tasks are committed or undone",
  "questions.read": "See the questions waiting for you (it cannot answer them)",
  "review.read": "Read a task's changes and their diffs",
  "review.annotate": "Add notes to a task's review (notes only: it cannot approve or refuse)",
  "files.read": "Read files in a task's copy",
  storage: "Keep a little data of its own",
  notify: "Show notifications",
  net: "Send and receive data over the internet, to the hosts listed",
};

const READS_WORK = ["events.read", "review.read", "files.read"];

/** Kule's commands an extension may set off, from a key pressed in its
 * frame or from `commands.execute`: ones that only show a panel or a
 * region. Never one that starts something (the terminal starts the
 * person's shell, outside the sandbox), takes the keyboard (the palette,
 * whose Enter could run a commit), switches the task or rearranges the
 * layout; and the keyboard goes back to the frame after (see `steady`). */
const VIEW_COMMANDS = new Set([
  "view.tasks",
  "view.files",
  "view.live",
  "view.review",
  "view.timeline",
  "chat.toggle",
  "layout.left",
  "layout.bottom",
  "layout.right",
  "layout.main",
]);
const viewOnly = (id) => VIEW_COMMANDS.has(id);

/** Runs a command for a frame, then gives the keyboard back to the frame,
 * so what the person goes on typing never lands somewhere else. */
function steady(rec, run) {
  run();
  if (document.activeElement !== rec.el && rec.el.isConnected) rec.el.focus();
}

/** Calls a frame may make in a burst before it waits. */
const CALLS_PER_10S = 200;
/** The largest request a frame may send on, as JSON. */
const MAX_PARAMS = 1024 * 1024;

async function loadExtensions() {
  try {
    EXT.prefix = (await call("extension_host")).prefix;
    EXT.listed = await call("extensions");
    EXT.mcp = await call("mcp_servers");
    const brought = await call("extension_themes");
    const counts = new Map();
    EXT.themes = brought.map((t) => {
      const n = counts.get(t.extension) || 0;
      counts.set(t.extension, n + 1);
      return {
        id: `x-${t.extension.replace(/\./g, "-")}-${n}`,
        name: t.theme.name,
        scheme: t.theme.scheme,
        colors: t.theme.colors,
        from: t.extension_name,
      };
    });
  } catch (err) {
    fail(err);
  }
  syncExtensions();
}

/** Starts what should run, stops what should not, and puts their commands,
 * panels and themes in the window. */
function syncExtensions() {
  const runnable = new Map(
    EXT.prefix ? EXT.listed.filter((l) => l.enabled && !l.held && l.manifest && l.approved).map((l) => [l.id, l]) : []
  );
  for (const [id, ext] of EXT.running) {
    const l = runnable.get(id);
    if (!l || l.digest !== ext.digest) stopExtension(id);
  }
  for (const [id, l] of runnable) if (!EXT.running.has(id)) startExtension(l);
  registerContributions();
  layout = normalLayout(layout);
  renderTask();
  if (state.view === "extensions") renderExtensions();
  if (state.view === "appearance") renderAppearance();
  if (!appearance.editing) applyTheme(currentTheme());
}

function startExtension(l) {
  const m = l.manifest;
  const ext = { id: l.id, digest: l.digest, manifest: m, perms: new Set(l.approved.permissions), frames: new Set(), calls: [] };
  EXT.running.set(l.id, ext);
  if (m.main) $("ext-hosts").append(makeFrame(ext, "_kule/main.html", { main: true }).el);
  for (const p of m.contributes.panels || []) {
    const id = `ext.${m.id}.${p.id}`;
    const frame = makeFrame(ext, p.entry, { panel: p.id });
    $("panels").append(
      h(
        "section",
        { class: "panel ext-panel", id: `panel-${id}`, "data-panel": id, hidden: true },
        // Drawn by Kule, outside the frame: the extension cannot paint over
        // it to pass for Kule.
        h("div", { class: "ext-strip" }, h("span", {}, `Extension: ${m.name}`), h("span", { class: "muted" }, `${m.publisher} · not verified`)),
        frame.el
      )
    );
  }
}

function stopExtension(id) {
  const ext = EXT.running.get(id);
  if (!ext) return;
  for (const rec of ext.frames) {
    if (rec.port) rec.port.close();
    rec.el.remove();
  }
  for (const p of ext.manifest.contributes.panels || []) $(`panel-ext.${id}.${p.id}`)?.remove();
  for (const notes of EXT.notes.values()) notes.delete(id);
  EXT.running.delete(id);
}

function makeFrame(ext, path, context) {
  const el = h("iframe", {
    class: "ext-frame",
    // Scripts, nothing else: no same origin, forms, popups or navigating the window.
    sandbox: "allow-scripts",
    src: `${EXT.prefix}${ext.id}/${path}`,
    title: ext.manifest.name,
    referrerpolicy: "no-referrer",
  });
  const rec = { ext, el, context, port: null };
  // Every load (a panel moved to another region reloads) gets a new port;
  // the old one closes. Only the extension's own code can navigate its frame.
  el.addEventListener("load", () => connectFrame(rec));
  ext.frames.add(rec);
  return rec;
}

function connectFrame(rec) {
  if (rec.port) rec.port.close();
  const channel = new MessageChannel();
  rec.port = channel.port1;
  channel.port1.onmessage = (e) => fromFrame(rec, e.data);
  const m = rec.ext.manifest;
  rec.el.contentWindow?.postMessage(
    {
      kule: "connect",
      extension: { id: m.id, name: m.name, version: m.version },
      context: { ...rec.context, task: state.current || null },
      theme: themeForFrames(),
    },
    "*",
    [channel.port2]
  );
  // What happened before the frame was there: the review on screen.
  const task = state.current;
  if (task && rec.ext.perms.has("review.read") && EXT.reviewTold.has(task)) {
    rec.port.postMessage({ event: "review", data: { task, digest: EXT.reviewTold.get(task) } });
  }
}

function fromFrame(rec, m) {
  if (!m || typeof m !== "object") return;
  const ext = rec.ext;
  const now = Date.now();
  ext.calls = ext.calls.filter((t) => now - t < 10000);
  if (ext.calls.length >= CALLS_PER_10S) {
    if (typeof m.id === "number") rec.port.postMessage({ id: m.id, error: "too many calls; slow down" });
    return;
  }
  ext.calls.push(now);
  if (m.key && typeof m.key === "object") {
    keyFromFrame(rec, m.key);
    return;
  }
  if (typeof m.id !== "number" || typeof m.method !== "string") return;
  // The app takes at most 1 MiB a request; a larger one goes no further.
  if (m.params !== undefined && JSON.stringify(m.params).length > MAX_PARAMS) {
    rec.port.postMessage({ id: m.id, error: "the request is larger than 1 MiB" });
    return;
  }
  handleCall(rec, m.method, m.params).then(
    (result) => rec.port?.postMessage({ id: m.id, result: result === undefined ? null : result }),
    (err) => rec.port?.postMessage({ id: m.id, error: String((err && err.message) || err) })
  );
}

async function handleCall(rec, method, params) {
  const ext = rec.ext;
  switch (method) {
    case "commands.execute":
      return executeFor(rec, params || {});
    case "review.annotate":
      // The app decides on the permission; the notes stay in the window.
      await call("ext_call", { ext: ext.id, method, params: null });
      return annotate(ext, params || {});
    case "ui.notify": {
      // The app checks the permission and the rate; with the window in
      // front it hands the words back to show here.
      const told = await call("ext_call", { ext: ext.id, method, params: params === undefined ? null : params });
      if (told && told.shown === "window") toast(told.body ? `${told.title} — ${told.body}` : told.title);
      return null;
    }
    default:
      return call("ext_call", { ext: ext.id, method, params: params === undefined ? null : params });
  }
}

/** One of the extension's own commands, or one of Kule's that only moves the view. */
function executeFor(rec, { id, args }) {
  const ext = rec.ext;
  if (typeof id !== "string") throw new Error("a command id is a string");
  const command = commandById(id);
  if (command && command.ext === ext.id) {
    if (command.local) runExtCommand(ext.id, command.local, args);
    else steady(rec, () => command.run());
    return null;
  }
  if (command && !command.ext && viewOnly(id)) {
    steady(rec, () => command.run());
    return null;
  }
  throw new Error(`${id} is not a command this extension can run`);
}

/** A key pressed in a frame, for the window's shortcuts: only view moves
 * and the extension's own commands answer it. */
function keyFromFrame(rec, k) {
  if (typeof k.key !== "string") return;
  if ($("palette") || $("task-menu") || keys.recording) return;
  const e = {
    key: k.key.slice(0, 32),
    code: typeof k.code === "string" ? k.code.slice(0, 32) : "",
    metaKey: Boolean(k.metaKey),
    ctrlKey: Boolean(k.ctrlKey),
    altKey: Boolean(k.altKey),
    shiftKey: Boolean(k.shiftKey),
  };
  const combos = eventCombos(e).map(normalCombo);
  if (!combos.length) return;
  const command = COMMANDS.find((c) => {
    const combo = bindingOf(c);
    return combo && combos.includes(combo) && (c.ext === rec.ext.id || (!c.ext && viewOnly(c.id)));
  });
  if (command) steady(rec, () => command.run());
}

function runExtCommand(extId, local, args) {
  const ext = EXT.running.get(extId);
  if (!ext) return;
  for (const rec of ext.frames) rec.port?.postMessage({ command: local, args: args === undefined ? null : args });
}

/** Text from an extension made fit to show. */
function extText(text, max) {
  return String(text)
    .replace(/[\u0000-\u001f\u007f-\u009f‪-‮⁦-⁩‎‏؜]/g, "")
    .slice(0, max);
}

/** Replaces the extension's notes on a task's review. */
function annotate(ext, { task, notes }) {
  if (typeof task !== "string" || !Array.isArray(notes)) throw new Error("annotate(task, [notes])");
  // Notes on tasks that exist only: made-up ids would grow without end.
  if (!state.tasks.some((t) => t.id === task)) throw new Error(`no task ${task}`);
  if (notes.length > 50) throw new Error("at most 50 notes on a task");
  const kept = notes.map((n) => {
    if (!n || typeof n.text !== "string" || !n.text.trim()) throw new Error("a note needs text");
    if (n.level !== undefined && n.level !== "info" && n.level !== "warn") throw new Error('a note\'s level is "info" or "warn"');
    if (n.path !== undefined && typeof n.path !== "string") throw new Error("a note's path is a string");
    return { level: n.level || "info", text: extText(n.text, 500), path: n.path === undefined ? null : extText(n.path, 256) };
  });
  if (!EXT.notes.has(task)) EXT.notes.set(task, new Map());
  if (kept.length) EXT.notes.get(task).set(ext.id, kept);
  else EXT.notes.get(task).delete(ext.id);
  if (state.current === task && isShown("review")) loadReview(task);
  return null;
}

/** The review's "From extensions" section: their notes, never a verdict. */
function extensionNotesGroup(task) {
  const byExt = EXT.notes.get(task);
  if (!byExt || !byExt.size) return [];
  const all = [...byExt].filter(([id]) => EXT.running.has(id));
  if (!all.length) return [];
  const count = all.reduce((n, [, notes]) => n + notes.length, 0);
  return [
    h("h2", { class: "group-title", "data-tab": "From extensions" }, `From extensions · ${plural(count, "note")}`),
    h("p", { class: "muted ext-notes-about" }, "Notes only: extensions cannot approve or refuse a change."),
    ...all.map(([id, notes]) =>
      h(
        "section",
        { class: "ext-notes" },
        h("h3", {}, EXT.running.get(id).manifest.name),
        h(
          "ul",
          { class: "files" },
          notes.map((n) =>
            h("li", { class: `ext-note ${n.level}` }, h("span", { class: `chip ${n.level === "warn" ? "ask" : ""}` }, n.level), n.path ? h("code", {}, n.path) : null, h("span", {}, n.text))
          )
        )
      )
    ),
  ];
}

/** Tells the frames of extensions holding `permission` (all, without one). */
function tellExtensions(name, data, permission) {
  for (const ext of EXT.running.values()) {
    if (permission && !ext.perms.has(permission)) continue;
    for (const rec of ext.frames) rec.port?.postMessage({ event: name, data });
  }
}

function tellExtension(id, name, data) {
  const ext = EXT.running.get(id);
  if (ext) for (const rec of ext.frames) rec.port?.postMessage({ event: name, data });
}

/** After a redraw: tells the frames when the chosen task changed. */
function extensionsSeeTask() {
  const task = state.current || null;
  if (EXT.taskTold === task) return;
  EXT.taskTold = task;
  tellExtensions("task-selected", { task });
}

/** After a review loads: tells extensions that read reviews, once per change. */
function extensionsSeeReview(task, digest) {
  if (EXT.reviewTold.get(task) === digest) return;
  EXT.reviewTold.set(task, digest);
  tellExtensions("review", { task, digest }, "review.read");
}

function themeForFrames() {
  const theme = currentTheme();
  const scheme = theme.scheme || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  const base = theme.scheme ? theme : themeById(scheme);
  return { scheme, colors: Object.fromEntries(THEME_PARTS.map(([p]) => [p, themeColor(base, p)])) };
}

function registerContributions() {
  for (let i = COMMANDS.length - 1; i >= 0; i--) if (COMMANDS[i].ext) COMMANDS.splice(i, 1);
  for (const key of Object.keys(PANELS)) if (PANELS[key].ext) delete PANELS[key];
  for (const ext of EXT.running.values()) {
    const m = ext.manifest;
    for (const c of m.contributes.commands || []) {
      // A suggested key Kule or another extension uses already is dropped;
      // the person can give the command one in Settings → Shortcuts.
      const wanted = c.keys ? normalCombo(c.keys) : "";
      const free = wanted && isUsable(wanted) && !isReserved(wanted) && !COMMANDS.some((o) => bindingOf(o) === wanted);
      COMMANDS.push({ id: `ext.${m.id}.${c.id}`, title: c.title, group: m.name, keys: free ? wanted : "", ext: m.id, local: c.id, run: () => runExtCommand(m.id, c.id) });
    }
    for (const p of m.contributes.panels || []) {
      const id = `ext.${m.id}.${p.id}`;
      PANELS[id] = { title: p.title, needsTask: Boolean(p.needsTask), ext: m.id };
      COMMANDS.push({ id, title: `Show ${p.title}`, group: m.name, keys: "", inFields: true, ext: m.id, run: () => revealPanel(id) });
    }
  }
  syncMenu();
}

// --- Settings → Extensions.

async function openExtensions() {
  state.view = "extensions";
  renderSidebar();
  renderTask();
  await loadExtensions();
}

async function extensionAction(work) {
  try {
    await work();
  } catch (err) {
    fail(err);
  }
  await loadExtensions();
}

function renderExtensions() {
  if (state.view !== "extensions") return;
  const install = h(
    "button",
    { class: "button", type: "button", "data-key": "ext-install", onclick: () => extensionAction(async () => {
      const l = await call("install_extension");
      if (l) toast(`Installed ${l.manifest ? l.manifest.name : l.id}; look at what it asks for, then approve it`);
    }) },
    "Install from a file…"
  );
  const dev = h(
    "button",
    { class: "button ghost", type: "button", "data-key": "ext-dev", onclick: () => extensionAction(async () => {
      const l = await call("install_dev_extension");
      if (l) toast(`Loaded ${l.id} from its folder`);
    }) },
    "Load a folder you are writing…"
  );
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Extensions"),
      h(
        "p",
        { class: "muted" },
        "Extensions add commands, panels, themes and helpers to Kule. Each runs in a sandboxed frame and may do only what you approve below; a change to it waits for you again. None can answer a question, commit, roll back, read your secrets or change a policy."
      ),
      h("div", { class: "agent-actions" }, install, dev)
    ),
    EXT.listed.length === 0
      ? h("p", { class: "muted" }, "No extensions yet. A .kulext file installs one; `kule ext pack FOLDER` makes one.")
      : h("ul", { class: "rule-list" }, ...EXT.listed.map(extensionCard))
  );
}

function extensionCard(l) {
  const m = l.manifest;
  const isNew = (p) => l.new_permissions.includes(p);
  const status = l.held ? `Held: ${l.held}` : l.enabled ? "Runs" : "Turned off";
  const kind = l.held ? "ask" : l.enabled ? "allow" : "";
  const parts = [
    h(
      "div",
      { class: "agent-card-head" },
      h("strong", {}, m ? m.name : l.id),
      h("span", { class: "muted" }, `${l.id} ${l.version}`),
      l.dev ? h("span", { class: "chip" }, "DEV") : null,
      h("span", { class: `chip ${kind}` }, status)
    ),
  ];
  if (m) {
    parts.push(h("div", { class: "muted" }, `Publisher: ${m.publisher} (not verified by anyone)`));
    if (m.description) parts.push(h("p", {}, m.description));
    const perms = m.permissions || [];
    parts.push(
      h(
        "ul",
        { class: "ext-perms" },
        perms.length
          ? perms.map((p) => h("li", {}, h("code", {}, p), ` ${PERMISSION_WORDS[p] || ""}`, isNew(p) && l.approved ? h("span", { class: "chip ask" }, "new") : null))
          : h("li", { class: "muted" }, "Asks for nothing beyond its own panels and commands")
      )
    );
    for (const host of m.hosts || []) {
      parts.push(h("div", {}, "Reaches ", h("code", {}, host), l.new_hosts.includes(host) && l.approved ? h("span", { class: "chip ask" }, "new") : null));
    }
    if (perms.includes("net") && perms.some((p) => READS_WORK.includes(p))) {
      parts.push(h("p", { class: "ext-warning" }, "It can read your work and send data to the hosts above."));
    }
    const c = m.contributes || {};
    const adds = [
      c.commands?.length ? plural(c.commands.length, "command") : null,
      c.panels?.length ? plural(c.panels.length, "panel") : null,
      c.themes?.length ? plural(c.themes.length, "theme") : null,
    ].filter(Boolean);
    if (adds.length) parts.push(h("div", { class: "muted" }, `Adds ${adds.join(", ")}`));
    if (c.agents) {
      parts.push(
        h(
          "div",
          {},
          "Brings agent profiles; each waits for you in ",
          h("button", { class: "link-button", type: "button", onclick: () => openSettings("agents") }, "Settings → Agents"),
          " before it runs."
        )
      );
    }
    for (const offered of EXT.mcp.filter((o) => o.extension === l.id)) parts.push(mcpRow(offered));
    parts.push(h("div", { class: "muted ext-digest" }, `Digest ${l.digest.slice(0, 16)}…`));
  }
  const actions = [];
  if (l.held && m) {
    const approve = armed(l.approved ? "Approve the change" : "Approve", "Click again to let it run as shown", false, () =>
      extensionAction(() => call("approve_extension", { id: l.id, digest: l.digest }))
    );
    approve.className = "button";
    approve.dataset.key = `ext-approve:${l.id}`;
    actions.push(approve);
  }
  actions.push(
    h(
      "button",
      { class: "button ghost", type: "button", "data-key": `ext-toggle:${l.id}`, onclick: () => extensionAction(() => call("set_extension_enabled", { id: l.id, enabled: !l.enabled })) },
      l.enabled ? "Turn off" : "Turn on"
    )
  );
  const remove = armed("Remove", "Click again to remove it and what it kept", false, () => extensionAction(() => call("remove_extension", { id: l.id })));
  remove.dataset.key = `ext-remove:${l.id}`;
  actions.push(remove);
  parts.push(h("div", { class: "agent-actions" }, ...actions));
  if (m && (m.contributes.configuration || []).length) parts.push(extensionSettings(l));
  return h("li", { class: "agent-card ext-card", "data-key": `ext:${l.id}` }, ...parts);
}

/** An MCP server an extension offers agents: what starts, and whether agents get it. */
function mcpRow(o) {
  const s = o.server;
  const env = Object.keys(s.env || {});
  const action = o.approved
    ? h(
        "button",
        { class: "button ghost", type: "button", "data-key": `mcp-revoke:${o.extension}/${s.name}`, onclick: () => extensionAction(() => call("revoke_mcp_server", { extension: o.extension, name: s.name })) },
        "Stop offering it"
      )
    : (() => {
        const b = armed("Let agents use it", "Click again: agents start it in their sandbox", false, () =>
          extensionAction(() => call("approve_mcp_server", { extension: o.extension, name: s.name, digest: o.digest }))
        );
        b.className = "button";
        b.dataset.key = `mcp-approve:${o.extension}/${s.name}`;
        return b;
      })();
  return h(
    "div",
    { class: "ext-mcp" },
    h("div", {}, h("strong", {}, `MCP server ${s.name}`), " ", h("span", { class: `chip ${o.approved ? "allow" : "ask"}` }, o.approved ? "agents get it" : "waits for you")),
    h("code", { class: "ext-mcp-command" }, [s.command, ...(s.args || [])].join(" ")),
    env.length ? h("div", { class: "muted" }, `Sets ${env.join(", ")}`) : null,
    h("div", { class: "muted" }, "Started by the agent, inside its sandbox; what it reaches the policy asks about."),
    action
  );
}

/** The extension's settings, drawn by Kule from its manifest. */
function extensionSettings(l) {
  const box = h("div", { class: "ext-settings" }, h("h3", {}, "Settings"));
  call("extension_config", { id: l.id })
    .then((values) => {
      for (const f of l.manifest.contributes.configuration) {
        const set = (value) =>
          call("set_extension_config", { id: l.id, key: f.key, value })
            .then(() => tellExtension(l.id, "config", { key: f.key, value }))
            .catch(fail);
        const value = values[f.key];
        let input;
        if (f.type === "boolean") input = h("input", { type: "checkbox", checked: value === true, onchange: (e) => set(e.currentTarget.checked) });
        else if (f.type === "enum")
          input = h(
            "select",
            { onchange: (e) => set(e.currentTarget.value) },
            f.values.map((v) => h("option", { value: v, selected: v === value }, v))
          );
        else if (f.type === "number")
          input = h("input", { type: "number", value: value ?? "", onchange: (e) => set(e.currentTarget.value === "" ? null : Number(e.currentTarget.value)) });
        else input = h("input", { type: "text", maxlength: 500, value: value ?? "", onchange: (e) => set(e.currentTarget.value === "" ? null : e.currentTarget.value) });
        box.append(h("label", { class: "ext-field" }, h("span", {}, f.title), input, f.description ? h("span", { class: "muted" }, f.description) : null));
      }
    })
    .catch(fail);
  return box;
}
