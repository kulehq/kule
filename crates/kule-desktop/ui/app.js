// Kule's window. Everything is built with DOM calls and textContent: paths,
// agent output and policy text come from outside and are never parsed as HTML.
"use strict";

// --- Backend ---------------------------------------------------------------

// The Tauri bridge, or a stand-in (window.kuleMock) when the page is opened
// in a browser to work on the layout.
const backend = window.__TAURI__
  ? {
      invoke: (cmd, args) => window.__TAURI__.core.invoke(cmd, args),
      onRun: (handler) => window.__TAURI__.event.listen("run", (e) => handler(e.payload)),
      onQuestions: (handler) => window.__TAURI__.event.listen("questions", (e) => handler(e.payload)),
      onDaemon: (handler) => window.__TAURI__.event.listen("daemon", () => handler()),
      onTasks: (handler) => window.__TAURI__.event.listen("tasks", () => handler()),
    }
  : window.kuleMock;

if (!backend) {
  document.body.textContent = "Kule's window runs inside the Kule app.";
  throw new Error("no Kule backend");
}

const call = (cmd, args) => backend.invoke(cmd, args);

// --- DOM helpers -----------------------------------------------------------

/** h("li", { class: "x", onclick: f }, "text", child) */
function h(tag, attrs, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs || {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on")) el.addEventListener(key.slice(2), value);
    else if (key === "class") el.className = value;
    else el.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children.flat()) {
    if (child === undefined || child === null || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
}

const $ = (id) => document.getElementById(id);

/**
 * Replaces el's children. When the focus was inside el, it moves to the new
 * element with the same data-key, so a redraw (a run's events, a refresh)
 * does not throw a keyboard or screen reader user back to the top.
 */
function replace(el, ...children) {
  const active = document.activeElement;
  const key = active && active !== el && el.contains(active) ? active.dataset.key : undefined;
  el.replaceChildren(...children);
  if (!key) return;
  const again = el.querySelector(`[data-key="${CSS.escape(key)}"]`);
  if (again) again.focus();
}

function toast(message, kind) {
  const el = h("div", { class: `toast ${kind || ""}`, role: kind === "error" ? "alert" : "status" }, message);
  $("toasts").append(el);
  setTimeout(() => el.remove(), kind === "error" ? 9000 : 4000);
}

const fail = (e) => toast(String(e), "error");

function baseName(path) {
  const parts = String(path).split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : String(path);
}

// --- Words -----------------------------------------------------------------

const VERDICT = {
  allow: { chip: "passes", title: "Passes on its own" },
  ask: { chip: "needs approval", title: "Waits for your approval" },
  deny: { chip: "denied", title: "Denied by the policy" },
};

const CHANGE_MARK = { created: "+", modified: "~", deleted: "−" };

const inGit = (path) => path === ".git" || path.startsWith(".git/");

function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many || one + "s"}`;
}

function when(unix) {
  return new Date(unix * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

// --- State -----------------------------------------------------------------

const state = {
  tasks: [],
  current: null,
  tab: "live",
  agents: [],
  /** Live stream per task: what the window saw since it opened. */
  streams: new Map(),
  /** Tasks whose next commit also takes the paths the workspace ignores. */
  includeIgnored: new Set(),
  /** Questions agents are waiting on, from every task. */
  questions: [],
  /** Tasks whose stream came from the daemon, and those on their way. */
  loaded: new Set(),
  loading: new Set(),
  /** The last event folded into each loaded stream, by the daemon's order. */
  lastSeq: new Map(),
  /** Live events that came while their task's stream was loading. */
  buffered: new Map(),
};

const currentTask = () => state.tasks.find((t) => t.id === state.current);

function streamOf(id) {
  if (!state.streams.has(id)) state.streams.set(id, []);
  return state.streams.get(id);
}

// --- Sidebar ---------------------------------------------------------------

const questionsOf = (id) => state.questions.filter((q) => q.task === id);

function taskChip(task) {
  if (questionsOf(task.id).length) return h("span", { class: "chip ask" }, "waiting for you");
  if (task.running) return h("span", { class: "chip running" }, "working");
  if (task.workspace_gone) return h("span", { class: "chip deny" }, "folder gone");
  if (task.verdict) return h("span", { class: `chip ${task.verdict}` }, VERDICT[task.verdict].chip);
  return null;
}

function taskItem(task, list) {
  const parts = [task.scanning ? "checking…" : task.changes ? plural(task.changes, "change") : "no changes"];
  if (task.ignored) parts.push(`${task.ignored} ignored`);
  const others = new Set((task.overlaps || []).map((o) => o.task)).size;
  if (others) parts.push(`meets ${plural(others, "task")}`);
  const sub = `${parts.join(", ")} · ${task.id}`;
  return h(
    "li",
    {},
    h(
      "button",
      {
        class: "task-item",
        "data-key": `${list}:${task.id}`,
        type: "button",
        "aria-current": task.id === state.current ? "true" : "false",
        title: task.workspace,
        onclick: () => select(task.id),
      },
      h("span", { class: "task-item-name" }, baseName(task.workspace)),
      taskChip(task),
      h("span", { class: "task-item-sub" }, sub)
    )
  );
}

function renderSidebar() {
  const waiting = state.tasks.filter(
    (t) => questionsOf(t.id).length || (!t.running && (t.verdict === "ask" || t.verdict === "deny"))
  );
  $("inbox-section").hidden = waiting.length === 0;
  replace($("inbox"), ...waiting.map((t) => taskItem(t, "inbox")));
  replace($("tasks"), ...state.tasks.map((t) => taskItem(t, "tasks")));
  $("tasks-empty").hidden = state.tasks.length > 0;
}

// --- Task view -------------------------------------------------------------

function renderTask() {
  const task = currentTask();
  $("empty").hidden = Boolean(task);
  $("task").hidden = !task;
  if (!task) return;

  $("task-name").textContent = baseName(task.workspace);
  $("task-path").textContent = `${task.workspace} · task ${task.id} · ${when(task.created_unix)}`;
  const chip = $("task-state");
  chip.className = "chip";
  if (task.running) {
    chip.classList.add("running");
    chip.textContent = "agent at work";
  } else if (task.workspace_gone) {
    chip.classList.add("deny");
    chip.textContent = "folder moved or deleted · commit and undo refused";
  } else if (task.scanning) {
    chip.textContent = "checking the copy…";
  } else if (task.verdict) {
    chip.classList.add(task.verdict);
    chip.textContent = `${plural(task.changes, "change")} · ${VERDICT[task.verdict].chip}`;
  } else {
    chip.textContent = task.ignored
      ? `in sync · ${task.ignored} ignored in the copy`
      : "in sync with the folder";
  }

  const count = $("review-count");
  count.hidden = !task.changes;
  count.textContent = String(task.changes);

  for (const tab of document.querySelectorAll(".tab")) {
    const selected = tab.dataset.tab === state.tab;
    tab.setAttribute("aria-selected", String(selected));
    // One stop for Tab; the arrow keys move between the tabs.
    tab.tabIndex = selected ? 0 : -1;
  }
  for (const panel of document.querySelectorAll(".panel")) {
    panel.hidden = panel.dataset.panel !== state.tab;
  }

  renderComposer(task);
  renderQuestions();
  renderStream(task.id);
  loadStream(task.id);
  if (state.tab === "review") loadReview(task.id);
  if (state.tab === "timeline") loadTimeline(task.id);
}

function renderComposer(task) {
  const busy = task.running;
  $("send").disabled = busy || state.agents.length === 0;
  $("prompt").disabled = busy;
  $("agent").disabled = busy;
  $("remove-task").disabled = busy;
  $("composer-hint").textContent = busy
    ? "The agent is at work. Its steps appear here as it goes."
    : "Runs in the sandbox, network through the task's proxy.";
  $("timeout").value = String(Math.round(task.approval_timeout_secs / 60));
}

// --- Questions -------------------------------------------------------------

function clock(unix) {
  return new Date(unix * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

function left(deadline) {
  const secs = Math.max(0, deadline - Math.floor(Date.now() / 1000));
  const m = Math.floor(secs / 60);
  const s = String(secs % 60).padStart(2, "0");
  return m >= 60 ? `${Math.floor(m / 60)} h ${m % 60} min` : `${m}:${s}`;
}

/** The current task's questions, as cards the person answers. */
function renderQuestions() {
  const task = currentTask();
  const cards = task
    ? questionsOf(task.id).map((q) =>
        h(
          "div",
          { class: "question" },
          h(
            "div",
            { class: "question-text" },
            h("p", { class: "question-title" }, `The agent wants to connect to ${q.host}:${q.port}`),
            h(
              "p",
              { class: "muted" },
              `No rule lets it pass on its own (${q.rules.join(", ")}). Unanswered, it is refused at ${clock(q.deadline_unix)} (in `,
              h("span", { class: "countdown", "aria-hidden": "true", "data-deadline": q.deadline_unix }, left(q.deadline_unix)),
              ") and the agent goes on without it."
            )
          ),
          h(
            "div",
            { class: "question-actions" },
            h(
              "button",
              { class: "button ask", type: "button", "data-key": `approve:${q.id}`, "aria-label": `Approve ${q.host}:${q.port}`, onclick: (e) => inFlight(e.currentTarget, () => answer(q.id, true)) },
              "Approve"
            ),
            h(
              "button",
              { class: "button ghost danger", type: "button", "data-key": `deny:${q.id}`, "aria-label": `Deny ${q.host}:${q.port}`, onclick: (e) => inFlight(e.currentTarget, () => answer(q.id, false)) },
              "Deny"
            )
          )
        )
      )
    : [];
  replace($("questions"), ...cards);
}

async function answer(id, approve) {
  try {
    await call("answer", { id, approve });
  } catch (err) {
    fail(err);
  }
}

async function onQuestions(waiting) {
  const before = new Set(state.questions.map((q) => q.id));
  state.questions = waiting;
  for (const q of waiting) {
    if (!before.has(q.id) && q.task !== state.current) {
      toast(`Task ${q.task} waits for you: ${q.host}:${q.port}`);
    }
  }
  renderSidebar();
  renderQuestions();
}

async function setTimeoutFor() {
  const task = currentTask();
  if (!task) return;
  try {
    await call("set_approval_timeout", { task: task.id, minutes: Number($("timeout").value) });
    await refresh();
  } catch (err) {
    fail(err);
  }
}

// --- Live ------------------------------------------------------------------

function streamItem(item) {
  switch (item.kind) {
    case "prompt":
      return h("div", { class: "msg prompt" }, item.text);
    case "message":
      return h("div", { class: "msg" }, item.text);
    case "thought":
      return h("div", { class: "msg thought" }, item.text);
    case "tool":
      return h(
        "div",
        { class: `event ${item.done ? (item.failed ? "failed" : "done") : ""}` },
        h("span", { class: "event-mark" }, item.done ? (item.failed ? "✗" : "✓") : "·"),
        h("span", {}, item.title),
        item.files && item.files.length
          ? h("span", { class: "event-files" }, `→ ${item.files.join(", ")}`)
          : null
      );
    case "note":
      return h("div", { class: "event" }, h("span", { class: "event-mark" }, "·"), h("span", {}, item.text));
    case "finished":
      return h(
        "div",
        { class: `event finished ${item.error ? "error" : ""}` },
        h("span", { class: "event-mark" }, item.error ? "!" : "■"),
        h("span", {}, item.text)
      );
    default:
      return null;
  }
}

function renderStream(id) {
  const items = streamOf(id);
  const stream = $("stream");
  const stick = stream.scrollTop + stream.clientHeight >= stream.scrollHeight - 40;
  if (items.length === 0) {
    replace(stream,
      h(
        "p",
        { class: "stream-empty" },
        "Give the agent a task below. Every tool call it makes becomes a step you can review."
      )
    );
  } else {
    replace(stream, ...items.map(streamItem));
  }
  if (stick) stream.scrollTop = stream.scrollHeight;
}

/** Folds one agent event into the task's stream. */
function absorb(id, event) {
  const items = streamOf(id);
  const last = items[items.length - 1];
  switch (event.type) {
    case "message":
    case "thought":
      if (last && last.kind === event.type) last.text += event.text;
      else items.push({ kind: event.type, text: event.text });
      break;
    case "tool_call":
      items.push({ kind: "tool", id: event.id, title: event.title, done: false });
      break;
    case "tool_done": {
      // Calls that only read or think take no step.
      const files = event.step ? event.step.changes.map((c) => c.path) : [];
      const open = items.find((i) => i.kind === "tool" && i.id === event.id && !i.done);
      const entry = open || { kind: "tool", id: event.id };
      Object.assign(entry, { title: event.title || entry.title, done: true, failed: event.failed, files });
      if (!open) items.push(entry);
      break;
    }
    case "permission":
      items.push({ kind: "note", text: `Permission “${event.title}”: ${event.chosen}` });
      break;
    case "plan":
      items.push({ kind: "note", text: `Plan: ${event.entries.join(" / ")}` });
      break;
    default:
      break;
  }
}

/** Folds one run event into the task's stream. */
function fold(task, event) {
  if (event.type === "prompt") {
    streamOf(task).push({ kind: "prompt", text: event.text });
  } else if (event.type === "agent") {
    absorb(task, event.event);
  } else if (event.type === "finished") {
    const refused = event.denials.length ? ` · the sandbox refused ${plural(event.denials.length, "operation")}` : "";
    streamOf(task).push({
      kind: "finished",
      error: Boolean(event.error),
      text: event.error ? `The run failed: ${event.error}` : `Done. Review the changes before they reach the folder${refused}.`,
    });
  }
}

async function onRun({ task, seq, event }) {
  if (state.loaded.has(task)) {
    // Already in the loaded stream: the daemon sent it before the load.
    if (seq > (state.lastSeq.get(task) || 0)) {
      fold(task, event);
      state.lastSeq.set(task, seq);
    }
  } else if (state.loading.has(task)) {
    // Held until the load returns, then folded if it was not in it.
    if (!state.buffered.has(task)) state.buffered.set(task, []);
    state.buffered.get(task).push({ seq, event });
  }
  if (event.type === "prompt" || event.type === "finished") await refresh();
  if (task === state.current) renderStream(task);
}

/**
 * Loads a task's stream from the daemon, which keeps it while the window
 * is closed, so reopening the app shows what the agent did meanwhile.
 */
async function loadStream(id) {
  if (state.loaded.has(id) || state.loading.has(id)) return;
  state.loading.add(id);
  try {
    const events = await call("stream", { task: id });
    state.streams.set(id, []);
    let last = 0;
    for (const { seq, event } of events) {
      fold(id, event);
      last = Math.max(last, seq);
    }
    for (const { seq, event } of state.buffered.get(id) || []) {
      if (seq > last) {
        fold(id, event);
        last = seq;
      }
    }
    state.lastSeq.set(id, last);
    state.loaded.add(id);
  } catch (err) {
    fail(err);
  } finally {
    state.loading.delete(id);
    state.buffered.delete(id);
  }
  if (id === state.current) renderStream(id);
}

/** The daemon came (back): what the window holds may be stale. */
async function onDaemon() {
  state.loaded.clear();
  state.lastSeq.clear();
  try {
    state.questions = await call("questions");
  } catch (err) {
    fail(err);
  }
  await refresh();
}

async function submitPrompt(e) {
  e.preventDefault();
  const task = currentTask();
  const prompt = $("prompt").value.trim();
  if (!task || !prompt) return;
  try {
    await call("start_run", { task: task.id, agent: $("agent").value, prompt });
    $("prompt").value = "";
    await refresh();
  } catch (err) {
    fail(err);
  }
}

// --- Review ----------------------------------------------------------------

/** An entity's edit, in the colours of a file's change. */
const EDIT = { added: "created", modified: "modified", removed: "deleted" };

/** Why a file has no outline, for the reasons worth telling. */
const NOT_OUTLINED = {
  before_unknown: "changed in the folder too; not outlined",
  too_large: "too large to outline",
  too_many: "too much in it to outline",
  unparsed: "could not be read to outline",
};

/** What changed inside a file: one line per function, section or key. */
function entityLines(outline) {
  if (!outline || outline.outline !== "changes") return [];
  return outline.changes.map((c) =>
    h(
      "li",
      { class: "entity" },
      h("span", { class: `change ${EDIT[c.edit]}` }, CHANGE_MARK[EDIT[c.edit]]),
      h("span", {}, h("span", { class: "entity-kind" }, c.kind.replace("_", " ")), c.name ? ` ${c.name}` : "")
    )
  );
}

function filesGroup(title, effects, holding, outlines) {
  if (effects.length === 0) return [];
  return [
    h("h2", { class: "group-title" }, title),
    h(
      "ul",
      { class: "files" },
      effects.flatMap((e) => {
        const held = holding.get(e.path);
        const outline = outlines.get(e.path);
        const why = outline && outline.outline === "skipped" ? NOT_OUTLINED[outline.reason] : null;
        return [
          h(
            "li",
            {},
            h("span", { class: `change ${e.change}` }, CHANGE_MARK[e.change]),
            h("span", {}, e.kind === "dir" ? `${e.path}/` : e.path),
            held ? h("span", { class: "held-mark" }, VERDICT[held].chip) : null,
            why ? h("span", { class: "entity-note" }, why) : null
          ),
          ...entityLines(outline),
        ];
      })
    ),
  ];
}

function gitGroup(git) {
  if (!git) return [];
  const lines = [];
  for (const ref of git.refs) {
    const short = (sha) => (sha ? sha.slice(0, 8) : "∅");
    let what = `${short(ref.old)} → ${short(ref.new)}`;
    if (ref.commits) what += ` (${plural(ref.commits, "commit")})`;
    if (ref.fast_forward === false) what += " · rewrites history";
    lines.push(`${ref.name}: ${what}`);
  }
  if (git.object_files) lines.push(`${plural(git.object_files, "new object file")}`);
  if (git.index) lines.push("index changed");
  if (git.config) lines.push("config changed");
  for (const hook of git.hooks) lines.push(`hook ${hook} changed`);
  for (const other of git.other) lines.push(other);
  if (lines.length === 0) return [];
  return [h("h2", { class: "group-title" }, "Git"), h("ul", { class: "files" }, lines.map((l) => h("li", {}, l)))];
}

let reviewToken = 0;

async function loadReview(id) {
  const token = ++reviewToken;
  const include = state.includeIgnored.has(id);
  let review;
  try {
    review = await call("review", { task: id, includeIgnored: include });
  } catch (err) {
    fail(err);
    return;
  }
  if (token !== reviewToken || id !== state.current) return;
  const task = currentTask();
  const outside = review.effects.effects.filter((e) => !inGit(e.path));
  const files = outside.filter((e) => !e.ignored);
  const ignored = outside.filter((e) => e.ignored);
  const taken = include ? outside : files;
  const empty = taken.length === 0 && !review.effects.git;

  const ruling = $("ruling");
  const holding = review.ruling.judgements.filter((j) => j.verdict !== "allow");
  ruling.className = `ruling ${empty ? "" : review.ruling.verdict}`;
  if (empty) {
    replace(ruling,
      h("p", { class: "ruling-title" }, "Nothing to commit"),
      h(
        "p",
        { class: "muted" },
        ignored.length
          ? `${plural(ignored.length, "change")} to ignored paths stay in the copy.`
          : "The copy holds what the folder holds."
      )
    );
  } else {
    replace(ruling,
      h("p", { class: "ruling-title" }, VERDICT[review.ruling.verdict].title),
      holding.length
        ? h(
            "ul",
            { class: "holding" },
            holding.map((j) =>
              h("li", {}, `${j.subject} · ${VERDICT[j.verdict].chip} `, h("span", { class: "rules" }, `(${j.rules.join(", ")})`))
            )
          )
        : h("p", { class: "muted" }, `${plural(taken.length, "file")} changed; the policy lets all of it through.`)
    );
  }

  const busy = task && task.running;
  const actions = [];
  if (!empty) {
    if (review.ruling.verdict === "allow") {
      actions.push(h("button", { class: "button primary", type: "button", "data-key": "commit", disabled: busy, onclick: (e) => inFlight(e.currentTarget, () => commit(id, false, include, review.digest)) }, "Commit to folder"));
    } else if (review.ruling.verdict === "ask") {
      actions.push(
        h("button", { class: "button ask", type: "button", "data-key": "commit", disabled: busy, onclick: (e) => inFlight(e.currentTarget, () => commit(id, true, include, review.digest)) }, "Approve and commit")
      );
    }
  }
  if (!empty || ignored.length) {
    const back = armed("Roll back", "Click again to throw the changes away", busy, () => rollback(id));
    back.dataset.key = "rollback";
    actions.push(back);
  }
  replace($("review-actions"), ...actions);

  const moves = review.effects.moves || [];
  const moved = new Set(moves.flatMap((m) => [m.from, m.to]));
  const byChange = (c) => files.filter((e) => e.change === c && !moved.has(e.path));
  const held = new Map(review.ruling.judgements.filter((j) => j.verdict !== "allow").map((j) => [j.subject, j.verdict]));
  const outlines = new Map((review.outline || []).map((o) => [o.path, o]));
  replace($("effects"),
    ...filesGroup("Created", byChange("created"), held, outlines),
    ...filesGroup("Modified", byChange("modified"), held, outlines),
    ...filesGroup("Deleted", byChange("deleted"), held, outlines),
    ...movesGroup(moves),
    ...gitGroup(review.effects.git),
    ...meetsGroup(task),
    ...ignoredGroup(id, ignored, include, held)
  );
}

/**
 * Paths the copy renamed: the same file under a new name, not a deletion.
 * What moved along with its directory is not listed apart.
 */
function movesGroup(moves) {
  const withParent = (m) =>
    moves.some((d) => m.to.startsWith(`${d.to}/`) && m.from === d.from + m.to.slice(d.to.length));
  const shown = moves.filter((m) => !withParent(m));
  if (shown.length === 0) return [];
  return [
    h("h2", { class: "group-title" }, "Moved"),
    h(
      "ul",
      { class: "files" },
      shown.map((m) => h("li", {}, h("span", { class: "change modified" }, "»"), h("span", {}, `${m.from} → ${m.to}`)))
    ),
  ];
}

/**
 * Files another task on the same folder changes too, and what in them:
 * whichever commits second will meet a conflict there.
 */
function meetsGroup(task) {
  const overlaps = (task && task.overlaps) || [];
  if (overlaps.length === 0) return [];
  return [
    h("h2", { class: "group-title" }, "Other tasks change these too"),
    h(
      "ul",
      { class: "files" },
      overlaps.map((o) =>
        h(
          "li",
          {},
          h("span", { class: "change modified" }, "!"),
          h(
            "span",
            {},
            o.path,
            h("span", { class: "entity-note" }, o.entities.length ? ` ${o.entities.join(", ")}` : " the whole file"),
            h("span", { class: "entity-note" }, ` · task ${o.task}`)
          )
        )
      )
    ),
  ];
}

/**
 * Paths the workspace's .gitignore excludes: build output, caches, secrets.
 * They stay in the copy unless the person asks for them.
 */
function ignoredGroup(id, ignored, include, held) {
  if (ignored.length === 0) return [];
  const toggle = h("input", {
    type: "checkbox",
    "data-key": "include-ignored",
    checked: include,
    onchange: () => {
      if (toggle.checked) state.includeIgnored.add(id);
      else state.includeIgnored.delete(id);
      loadReview(id);
    },
  });
  return [
    h(
      "details",
      { class: "ignored-group", open: include },
      h("summary", { class: "group-title" }, `Ignored by the folder's .gitignore · ${ignored.length} ${include ? "included" : "stay in the copy"}`),
      h("label", { class: "check" }, toggle, "Also commit these"),
      h(
        "ul",
        { class: "files" },
        ignored.map((e) =>
          h(
            "li",
            {},
            h("span", { class: `change ${e.change}` }, CHANGE_MARK[e.change]),
            h("span", {}, e.kind === "dir" ? `${e.path}/` : e.path),
            include && held.get(e.path) ? h("span", { class: "held-mark" }, VERDICT[held.get(e.path)].chip) : null
          )
        )
      )
    ),
  ];
}

/** A button for an action that cannot be taken back: the first click arms it. */
function armed(label, armedLabel, disabled, action) {
  const button = h("button", { class: "button ghost danger", type: "button", disabled }, label);
  let timer = null;
  const disarm = () => {
    clearTimeout(timer);
    button.classList.remove("armed");
    button.textContent = label;
  };
  button.disarm = disarm;
  button.addEventListener("click", async () => {
    if (button.disabled) return;
    if (button.classList.contains("armed")) {
      // Fires once: disarmed and held off until the call is done, so a
      // second click never acts on the next task or the commit before.
      disarm();
      await inFlight(button, action);
      return;
    }
    button.classList.add("armed");
    button.textContent = typeof armedLabel === "function" ? armedLabel() : armedLabel;
    timer = setTimeout(disarm, 4000);
  });
  return button;
}

/** Runs `action` with `button` disabled, so a double click acts once. */
async function inFlight(button, action) {
  const was = button.disabled;
  button.disabled = true;
  try {
    await action();
  } finally {
    button.disabled = was;
  }
}

async function commit(id, approve, includeIgnored, reviewed) {
  try {
    const result = await call("commit", { task: id, approve, includeIgnored, reviewed });
    if (result.outcome === "applied") state.includeIgnored.delete(id);
    if (result.outcome === "applied") toast(`Committed ${plural(result.changes, "change")} · checkpoint ${result.checkpoint}`);
    else if (result.outcome === "nothing") toast("Nothing to commit");
    else toast(`Not committed: ${VERDICT[result.ruling.verdict].title.toLowerCase()}`, "error");
  } catch (err) {
    fail(err);
  }
  await refresh();
}

async function rollback(id) {
  try {
    const n = await call("rollback", { task: id });
    toast(`Rolled back ${plural(n, "change")}`);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

// --- Timeline --------------------------------------------------------------

/** Undo of the newest commit: held off while an agent works or the copy
 * has uncommitted changes, which undo would refuse anyway. */
function undoButton(id, checkpoint, task) {
  const pending = task && task.changes > 0;
  const button = armed("Undo", `Click again to undo commit ${checkpoint}`, Boolean(task && (task.running || pending)), () => undo(id));
  button.dataset.key = "undo";
  if (pending) button.title = "Commit or roll back the uncommitted changes first";
  return button;
}

let timelineToken = 0;

function changeLine(change) {
  const mark = change.change ? CHANGE_MARK[change.change] : "↺";
  return `${mark} ${change.path}`;
}

async function loadTimeline(id) {
  const token = ++timelineToken;
  let timeline;
  try {
    timeline = await call("timeline", { task: id });
  } catch (err) {
    fail(err);
    return;
  }
  if (token !== timelineToken || id !== state.current) return;
  const task = currentTask();
  const entries = [];

  const newestApplied = [...timeline.checkpoints].reverse().find((c) => c.state === "applied");
  for (const cp of timeline.checkpoints) {
    const files = cp.effects.filter((e) => !inGit(e.path));
    const gitCount = cp.effects.length - files.length;
    const title = cp.state === "undone" ? `Commit ${cp.id} · undone` : cp.state === "applying" ? `Commit ${cp.id} · interrupted` : `Commit ${cp.id}`;
    const body = files.slice(0, 12).map((e) => `${CHANGE_MARK[e.change]} ${e.path}`);
    if (files.length > 12) body.push(`… and ${files.length - 12} more`);
    if (gitCount) body.push(`${plural(gitCount, "change")} under .git`);
    entries.push(
      h(
        "div",
        { class: `entry ${cp.state}` },
        h("span", { class: "entry-dot" }),
        h("span", { class: "entry-title" }, title),
        cp === newestApplied
          ? undoButton(id, cp.id, task)
          : h("span"),
        h("div", { class: "entry-body" }, body.map((line) => h("div", {}, line)))
      )
    );
  }
  for (const step of timeline.steps) {
    entries.push(
      h(
        "div",
        { class: "entry step" },
        h("span", { class: "entry-dot" }),
        h("span", { class: "entry-title" }, `Step ${step.id} · ${step.label}`),
        h("span", { class: "chip" }, "uncommitted"),
        h(
          "div",
          { class: "entry-body" },
          step.changes.length ? step.changes.map((c) => h("div", {}, changeLine(c))) : h("div", {}, "no file changes")
        )
      )
    );
  }
  if (entries.length === 0) {
    entries.push(h("p", { class: "muted" }, "No commits or steps yet."));
  }

  const network = summarizeConnections(timeline.connections);
  if (network.length) {
    entries.push(h("h2", { class: "group-title" }, "Network"));
    entries.push(h("ul", { class: "files" }, network.map((line) => h("li", {}, line))));
  }
  if (timeline.denials.length) {
    entries.push(h("h2", { class: "group-title" }, "Refused by the sandbox"));
    entries.push(
      h(
        "ul",
        { class: "files" },
        timeline.denials
          .slice(-50)
          .map((d) =>
            h(
              "li",
              {},
              h("span", { class: "change denied" }, "×"),
              `${d.operation} ${d.path} (${d.process})`,
              d.saved ? ` · what was there is in ${d.saved}` : null
            )
          )
      )
    );
  }
  replace($("timeline"), ...entries);
}

function summarizeConnections(log) {
  const seen = new Map();
  for (const c of log) {
    const key = `${c.host}:${c.port} ${c.verdict} ${c.outcome} ${c.answer || ""}`;
    const entry = seen.get(key) || { c, n: 0 };
    entry.n += 1;
    seen.set(key, entry);
  }
  return [...seen.values()].map(({ c, n }) => {
    const asked = {
      approved: `approved by you · ${c.outcome}`,
      denied: "denied by you",
      timed_out: "refused, nobody answered in time",
    };
    const what =
      c.verdict === "allow"
        ? c.outcome
        : c.verdict === "ask"
          ? asked[c.answer] || "refused, waits for approval"
          : "refused, denied";
    return `${c.host}:${c.port}${n > 1 ? ` ×${n}` : ""} · ${what}`;
  });
}

async function undo(id) {
  try {
    const undone = await call("undo", { task: id });
    toast(undone ? `Undid commit ${undone}` : "Nothing to undo");
  } catch (err) {
    fail(err);
  }
  await refresh();
}

// --- Tasks -----------------------------------------------------------------

let refreshToken = 0;

async function refresh() {
  const token = ++refreshToken;
  try {
    const tasks = await call("tasks");
    // A refresh that started later has already drawn a newer list.
    if (token !== refreshToken) return;
    state.tasks = tasks;
  } catch (err) {
    fail(err);
  }
  if (state.current && !currentTask()) state.current = null;
  if (!state.current && state.tasks.length) state.current = state.tasks[0].id;
  renderSidebar();
  renderTask();
}

function select(id) {
  if (id !== state.current) {
    $("remove-task").disarm?.();
    clearPanels();
  }
  state.current = id;
  renderSidebar();
  renderTask();
}

/** Empties Review and Timeline until the new task's load returns, so no
 * button of the task before stays live meanwhile. */
function clearPanels() {
  const loading = () => h("p", { class: "muted" }, "Loading…");
  $("ruling").className = "ruling";
  replace($("ruling"), loading());
  replace($("review-actions"));
  replace($("effects"));
  replace($("timeline"), loading());
}

async function newTask() {
  let workspace;
  try {
    workspace = await call("pick_workspace");
  } catch (err) {
    fail(err);
    return;
  }
  if (!workspace) return;
  toast(`Copying ${baseName(workspace)}…`);
  try {
    const task = await call("create_task", { workspace });
    state.current = task.id;
    state.tab = "live";
    await refresh();
    $("prompt").focus();
  } catch (err) {
    fail(err);
  }
}

async function openTerminal() {
  const task = currentTask();
  if (!task) return;
  try {
    await call("open_terminal", { task: task.id });
  } catch (err) {
    fail(err);
  }
}

async function removeTask() {
  const task = currentTask();
  if (!task) return;
  try {
    await call("remove_task", { task: task.id });
    state.streams.delete(task.id);
    state.loaded.delete(task.id);
    state.current = null;
    toast(`Deleted task ${task.id}; the folder was not touched`);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

// --- Start -----------------------------------------------------------------

async function start() {
  $("new-task").addEventListener("click", newTask);
  $("open-terminal").addEventListener("click", openTerminal);
  $("empty-new-task").addEventListener("click", newTask);
  $("composer").addEventListener("submit", submitPrompt);
  $("prompt").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) $("composer").requestSubmit();
  });
  const tabs = [...document.querySelectorAll(".tab")];
  const showTab = (tab) => {
    state.tab = tab.dataset.tab;
    renderTask();
  };
  for (const tab of tabs) {
    tab.addEventListener("click", () => showTab(tab));
  }
  $("tabs").addEventListener("keydown", (e) => {
    const at = tabs.indexOf(document.activeElement);
    if (at === -1) return;
    const to = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const tab = tabs[(to + tabs.length) % tabs.length];
    showTab(tab);
    tab.focus();
  });
  const remove = armed(
    "Delete task",
    () => {
      const task = currentTask();
      return task ? `Click again to delete ${baseName(task.workspace)} (${task.id})` : "Click again to delete";
    },
    false,
    removeTask
  );
  remove.id = "remove-task";
  $("remove-task").replaceWith(remove);

  try {
    state.agents = await call("agents");
  } catch (err) {
    fail(err);
  }
  replace($("agent"), ...state.agents.map((name) => h("option", { value: name }, name)));

  await backend.onRun(onRun);
  await backend.onQuestions(onQuestions);
  if (backend.onDaemon) await backend.onDaemon(onDaemon);
  // A task's numbers came back from a scan in the background.
  if (backend.onTasks) await backend.onTasks(() => refresh());
  // Changes made outside the window (a terminal in the copy, the CLI) show
  // when the person comes back to it.
  window.addEventListener("focus", () => refresh());
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") refresh();
  });
  try {
    state.questions = await call("questions");
  } catch (err) {
    fail(err);
  }
  $("timeout").addEventListener("change", setTimeoutFor);
  // Counts down the deadlines without re-rendering the cards.
  setInterval(() => {
    for (const el of document.querySelectorAll(".countdown")) {
      el.textContent = left(Number(el.dataset.deadline));
    }
  }, 1000);
  await refresh();
}

start();
