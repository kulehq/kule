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
    }
  : window.kuleMock;

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
};

const currentTask = () => state.tasks.find((t) => t.id === state.current);

function streamOf(id) {
  if (!state.streams.has(id)) state.streams.set(id, []);
  return state.streams.get(id);
}

// --- Sidebar ---------------------------------------------------------------

function taskChip(task) {
  if (task.running) return h("span", { class: "chip running" }, "working");
  if (task.verdict) return h("span", { class: `chip ${task.verdict}` }, VERDICT[task.verdict].chip);
  return null;
}

function taskItem(task) {
  const sub = task.changes
    ? `${plural(task.changes, "change")} · ${task.id}`
    : `no changes · ${task.id}`;
  return h(
    "li",
    {},
    h(
      "button",
      {
        class: "task-item",
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
  const waiting = state.tasks.filter((t) => !t.running && (t.verdict === "ask" || t.verdict === "deny"));
  $("inbox-section").hidden = waiting.length === 0;
  $("inbox").replaceChildren(...waiting.map(taskItem));
  $("tasks").replaceChildren(...state.tasks.map(taskItem));
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
  } else if (task.verdict) {
    chip.classList.add(task.verdict);
    chip.textContent = `${plural(task.changes, "change")} · ${VERDICT[task.verdict].chip}`;
  } else {
    chip.textContent = "in sync with the workspace";
  }

  const count = $("review-count");
  count.hidden = !task.changes;
  count.textContent = String(task.changes);

  for (const tab of document.querySelectorAll(".tab")) {
    tab.setAttribute("aria-selected", String(tab.dataset.tab === state.tab));
  }
  for (const panel of document.querySelectorAll(".panel")) {
    panel.hidden = panel.dataset.panel !== state.tab;
  }

  renderComposer(task);
  renderStream(task.id);
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
    stream.replaceChildren(
      h(
        "p",
        { class: "stream-empty" },
        "Give the agent a task below. Every tool call it makes becomes a step you can review."
      )
    );
  } else {
    stream.replaceChildren(...items.map(streamItem));
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
      const files = event.step.changes.map((c) => c.path);
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

async function onRun({ task, event }) {
  if (event.type === "agent") {
    absorb(task, event.event);
  } else if (event.type === "finished") {
    const refused = event.denials.length ? ` · the sandbox refused ${plural(event.denials.length, "operation")}` : "";
    streamOf(task).push({
      kind: "finished",
      error: Boolean(event.error),
      text: event.error ? `The run failed: ${event.error}` : `Done. Review the changes before they reach the workspace${refused}.`,
    });
    await refresh();
  }
  if (task === state.current) renderStream(task);
}

async function submitPrompt(e) {
  e.preventDefault();
  const task = currentTask();
  const prompt = $("prompt").value.trim();
  if (!task || !prompt) return;
  try {
    await call("start_run", { task: task.id, agent: $("agent").value, prompt });
    streamOf(task.id).push({ kind: "prompt", text: prompt });
    $("prompt").value = "";
    await refresh();
  } catch (err) {
    fail(err);
  }
}

// --- Review ----------------------------------------------------------------

function filesGroup(title, effects, holding) {
  if (effects.length === 0) return [];
  return [
    h("h2", { class: "group-title" }, title),
    h(
      "ul",
      { class: "files" },
      effects.map((e) => {
        const held = holding.get(e.path);
        return h(
          "li",
          {},
          h("span", { class: `change ${e.change}` }, CHANGE_MARK[e.change]),
          h("span", {}, e.kind === "dir" ? `${e.path}/` : e.path),
          held ? h("span", { class: "held-mark" }, VERDICT[held].chip) : null
        );
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
  let review;
  try {
    review = await call("review", { task: id });
  } catch (err) {
    fail(err);
    return;
  }
  if (token !== reviewToken || id !== state.current) return;
  const task = currentTask();
  const files = review.effects.effects.filter((e) => !inGit(e.path));
  const empty = files.length === 0 && !review.effects.git;

  const ruling = $("ruling");
  const holding = review.ruling.judgements.filter((j) => j.verdict !== "allow");
  ruling.className = `ruling ${empty ? "" : review.ruling.verdict}`;
  if (empty) {
    ruling.replaceChildren(
      h("p", { class: "ruling-title" }, "Nothing to review"),
      h("p", { class: "muted" }, "The copy holds what the workspace holds.")
    );
  } else {
    ruling.replaceChildren(
      h("p", { class: "ruling-title" }, VERDICT[review.ruling.verdict].title),
      holding.length
        ? h(
            "ul",
            { class: "holding" },
            holding.map((j) =>
              h("li", {}, `${j.subject} · ${VERDICT[j.verdict].chip} `, h("span", { class: "rules" }, `(${j.rules.join(", ")})`))
            )
          )
        : h("p", { class: "muted" }, `${plural(files.length, "file")} changed; the policy lets all of it through.`)
    );
  }

  const busy = task && task.running;
  const actions = [];
  if (!empty) {
    if (review.ruling.verdict === "allow") {
      actions.push(h("button", { class: "button primary", type: "button", disabled: busy, onclick: () => commit(id, false) }, "Commit to workspace"));
    } else if (review.ruling.verdict === "ask") {
      actions.push(
        h("button", { class: "button ask", type: "button", disabled: busy, onclick: () => commit(id, true) }, "Approve and commit")
      );
    }
    actions.push(armed("Roll back", "Click again to throw the changes away", busy, () => rollback(id)));
  }
  $("review-actions").replaceChildren(...actions);

  const byChange = (c) => files.filter((e) => e.change === c);
  const held = new Map(review.ruling.judgements.filter((j) => j.verdict !== "allow").map((j) => [j.subject, j.verdict]));
  $("effects").replaceChildren(
    ...filesGroup("Created", byChange("created"), held),
    ...filesGroup("Modified", byChange("modified"), held),
    ...filesGroup("Deleted", byChange("deleted"), held),
    ...gitGroup(review.effects.git)
  );
}

/** A button for an action that cannot be taken back: the first click arms it. */
function armed(label, armedLabel, disabled, action) {
  const button = h("button", { class: "button ghost danger", type: "button", disabled }, label);
  let timer = null;
  button.addEventListener("click", () => {
    if (button.classList.contains("armed")) {
      clearTimeout(timer);
      action();
      return;
    }
    button.classList.add("armed");
    button.textContent = armedLabel;
    timer = setTimeout(() => {
      button.classList.remove("armed");
      button.textContent = label;
    }, 4000);
  });
  return button;
}

async function commit(id, approve) {
  try {
    const result = await call("commit", { task: id, approve });
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
          ? armed("Undo", "Click again to undo this commit", task && task.running, () => undo(id))
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
        timeline.denials.slice(-50).map((d) => h("li", {}, h("span", { class: "change denied" }, "×"), `${d.operation} ${d.path} (${d.process})`))
      )
    );
  }
  $("timeline").replaceChildren(...entries);
}

function summarizeConnections(log) {
  const seen = new Map();
  for (const c of log) {
    const key = `${c.host}:${c.port} ${c.verdict} ${c.outcome}`;
    const entry = seen.get(key) || { c, n: 0 };
    entry.n += 1;
    seen.set(key, entry);
  }
  return [...seen.values()].map(({ c, n }) => {
    const what = c.verdict === "allow" ? c.outcome : c.verdict === "ask" ? "refused, waits for approval" : "refused, denied";
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

async function refresh() {
  try {
    state.tasks = await call("tasks");
  } catch (err) {
    fail(err);
  }
  if (state.current && !currentTask()) state.current = null;
  if (!state.current && state.tasks.length) state.current = state.tasks[0].id;
  renderSidebar();
  renderTask();
}

function select(id) {
  state.current = id;
  renderSidebar();
  renderTask();
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

async function removeTask() {
  const task = currentTask();
  if (!task) return;
  try {
    await call("remove_task", { task: task.id });
    state.streams.delete(task.id);
    state.current = null;
    toast(`Deleted task ${task.id}; the workspace was not touched`);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

// --- Start -----------------------------------------------------------------

async function start() {
  $("new-task").addEventListener("click", newTask);
  $("empty-new-task").addEventListener("click", newTask);
  $("composer").addEventListener("submit", submitPrompt);
  $("prompt").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) $("composer").requestSubmit();
  });
  for (const tab of document.querySelectorAll(".tab")) {
    tab.addEventListener("click", () => {
      state.tab = tab.dataset.tab;
      renderTask();
    });
  }
  const remove = armed("Delete task", "Click again to delete the task", false, removeTask);
  remove.id = "remove-task";
  $("remove-task").replaceWith(remove);

  try {
    state.agents = await call("agents");
  } catch (err) {
    fail(err);
  }
  $("agent").replaceChildren(...state.agents.map((name) => h("option", { value: name }, name)));

  await backend.onRun(onRun);
  await refresh();
}

start();
