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
      onMenu: (handler) => window.__TAURI__.event.listen("menu", (e) => handler(e.payload)),
      onTerminal: (handler) => window.__TAURI__.event.listen("terminal", (e) => handler(e.payload)),
      onTaskChanged: (handler) => window.__TAURI__.event.listen("task-changed", (e) => handler(e.payload)),
      onExtensions: (handler) => window.__TAURI__.event.listen("extensions", () => handler()),
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
    // Through the style object: the window's content security policy
    // refuses a style attribute, not styles set from script.
    else if (key === "style") el.style.cssText = value;
    else if (key === "title" || key.startsWith("aria-")) el.setAttribute(key, shown(String(value)));
    else el.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children.flat()) {
    if (child === undefined || child === null || child === false) continue;
    // Shown with what it hides written out (see `shown`, highlight.js).
    el.append(child instanceof Node ? child : document.createTextNode(shown(String(child))));
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
  // Left-out parts (`cond ? h(…) : null`) are skipped, as `h` skips them.
  el.replaceChildren(...children.flat().filter((c) => c !== null && c !== undefined && c !== false));
  if (!key) return;
  const again = el.querySelector(`[data-key="${CSS.escape(key)}"]`);
  if (again) again.focus();
}

function toast(message, kind) {
  const el = h("div", { class: `toast ${kind || ""}`, role: kind === "error" ? "alert" : "status" }, message);
  $("toasts").append(el);
  setTimeout(() => leave(el), kind === "error" ? 9000 : 4000);
}

const fail = (e) => toast(String(e), "error");

// --- Motion ----------------------------------------------------------------

// Small movements where something changes: what opens, arrives, ends or
// moves. They all go when the person turns motion off, or leaves it to the
// computer and the computer asks for less (its "reduce motion").

const REDUCE_MOTION = matchMedia("(prefers-reduced-motion: reduce)");
/** How long an arrival or a move takes: the stylesheet's --settle. */
const SETTLE_MS = 290;
const EASE_OUT = "cubic-bezier(0.2, 0.7, 0.3, 1)";

/** Whether the window moves, as the person chose: `system`, `on` or `off`. */
function applyMotion(chosen) {
  const on = chosen === "on" || (chosen !== "off" && !REDUCE_MOTION.matches);
  document.documentElement.dataset.motion = on ? "on" : "off";
}
applyMotion("system");
REDUCE_MOTION.addEventListener("change", () => applyMotion(appearance.kept.motion));

const moving = () => document.documentElement.dataset.motion === "on";

/** What is arriving on screen, by key: when it came and for how long it
 * moves. A redraw meanwhile carries its arrival on instead of starting
 * it again. */
const arrivals = new Map();

/** Says that what `key` names has just come, to arrive on its next draw. */
function arrive(key, lasts = SETTLE_MS) {
  if (!moving()) return;
  const now = performance.now();
  if (arrivals.size > 256) {
    for (const [k, a] of arrivals) if (now - a.at > a.lasts) arrivals.delete(k);
  }
  arrivals.set(key, { at: now, lasts });
}

/** Marks `el` as arriving while what `key` names still is. */
function arriving(el, key) {
  const a = arrivals.get(key);
  if (!a || !el) return el;
  const since = performance.now() - a.at;
  if (since > a.lasts) {
    arrivals.delete(key);
    return el;
  }
  el.classList.add("arrived");
  el.style.setProperty("--since", `${-Math.round(since)}ms`);
  return el;
}

/** Takes `el` away, after its leaving movement when the window moves. */
function leave(el) {
  if (!moving()) {
    el.remove();
    return;
  }
  el.inert = true;
  el.classList.add("leaving");
  setTimeout(() => el.remove(), SETTLE_MS);
}

/** Redraws with `change` and slides each element of `scope` matching
 * `selector` from where it was to where it is now, by `keyOf`; one that
 * was not there arrives when `arrive` says new ones do. */
function glide(scope, selector, keyOf, change, arrive) {
  if (!moving()) {
    change();
    return;
  }
  const was = new Map();
  for (const el of scope.querySelectorAll(selector)) {
    const at = el.getBoundingClientRect();
    if (at.width) was.set(keyOf(el), at);
  }
  change();
  const now = performance.now();
  for (const el of scope.querySelectorAll(selector)) {
    const key = keyOf(el);
    const to = el.getBoundingClientRect();
    if (!to.width) continue;
    const from = was.get(key);
    if (!from) {
      if (arrive && !arrivals.has(key)) arrivals.set(key, { at: now, lasts: SETTLE_MS });
      arriving(el, key);
      continue;
    }
    arriving(el, key);
    const dx = from.left - to.left;
    const dy = from.top - to.top;
    if (Math.abs(dx) < 1 && Math.abs(dy) < 1) continue;
    el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: "none" }], { duration: SETTLE_MS, easing: EASE_OUT });
  }
}

/** Plays the animation `name` sets on `el` again from its start. */
function replay(el, name) {
  el.classList.remove(name);
  void el.offsetWidth;
  el.classList.add(name);
}

/** Shows the person where Kule took them: scrolled to, and lit a moment. */
function spotlight(el) {
  if (!el) return;
  el.scrollIntoView({ block: "nearest", behavior: moving() ? "smooth" : "auto" });
  if (!moving()) return;
  replay(el, "spotlit");
  setTimeout(() => el.classList.remove("spotlit"), 1600);
}

/** The selected tab's line, slid from the tab it was under. */
function placeTabLines() {
  for (const list of document.querySelectorAll(".tabs, .region-tabs")) {
    const tab = list.querySelector(':scope > .tab[aria-selected="true"]');
    if (!tab || !tab.offsetWidth) {
      list.classList.remove("lined");
      continue;
    }
    const first = !list.classList.contains("lined");
    list.classList.toggle("line-still", first);
    list.style.setProperty("--line-x", `${tab.offsetLeft}px`);
    list.style.setProperty("--line-w", `${tab.offsetWidth}px`);
    list.classList.add("lined");
    if (first) {
      void list.offsetWidth;
      list.classList.remove("line-still");
    }
  }
}

let tabLineFrame = 0;
function lineTabs() {
  cancelAnimationFrame(tabLineFrame);
  tabLineFrame = requestAnimationFrame(placeTabLines);
}
new MutationObserver((changes) => {
  if (changes.some((c) => c.attributeName === "hidden" || c.target.closest?.(".tabs, .region-tabs"))) lineTabs();
}).observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["aria-selected", "hidden"] });
addEventListener("resize", lineTabs);

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

/** Task id → what its run at work reported it cost so far. */
const liveCost = new Map();

const state = {
  tasks: [],
  current: null,
  /** "tasks", or the section of Settings shown: one of SETTINGS. */
  view: "tasks",
  /** Per task, the held files the person ticked not to be asked about again. */
  remember: new Map(),
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
  /** The groups tasks are put in, in the person's order. */
  groups: [],
  /** The policy for every task: "balanced", "careful" or "own". */
  policy: "balanced",
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

/** The colours a task can be marked with; the daemon takes no others. */
const COLORS = ["red", "orange", "yellow", "green", "teal", "blue", "purple", "pink"];

/** The folder's name, and the one the person gave the task after it. */
function taskName(task) {
  const folder = baseName(task.workspace);
  return task.label ? `${folder} – ${task.label}` : folder;
}

function taskItem(task, list) {
  const parts = [task.scanning ? "checking…" : task.changes ? plural(task.changes, "change") : "no changes"];
  if (task.ignored) parts.push(`${task.ignored} ignored`);
  const others = new Set((task.overlaps || []).map((o) => o.task)).size;
  if (others) parts.push(`meets ${plural(others, "task")}`);
  const sub = `${parts.join(", ")} · ${task.id}`;
  const ordered = list === "tasks";
  const item = h(
    "button",
    {
      class: "task-item",
      "data-key": `${list}:${task.id}`,
      type: "button",
      "aria-current": task.id === state.current ? "true" : "false",
      "aria-keyshortcuts": ordered ? "Alt+ArrowUp Alt+ArrowDown" : undefined,
      title: ordered ? `${task.workspace}\nDrag, or Alt+↑/↓, to move it in the list.` : task.workspace,
      onclick: (e) => {
        // The click a drag ends with does not open the task.
        if (dragged) {
          e.preventDefault();
          return;
        }
        select(task.id);
      },
      oncontextmenu: (e) => {
        e.preventDefault();
        taskMenu(task, list, e);
      },
      onkeydown: (e) => {
        if (!ordered || !e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
        e.preventDefault();
        moveTask(task.id, e.key === "ArrowUp" ? -1 : 1);
      },
      onpointerdown: ordered ? (e) => dragStart(e, task.id) : undefined,
    },
    h(
      "span",
      { class: "task-item-name" },
      h("span", { class: "task-folder" }, baseName(task.workspace)),
      task.label ? h("span", { class: "task-label" }, ` – ${task.label}`) : null
    ),
    taskChip(task),
    h("span", { class: "task-item-sub" }, sub)
  );
  const more = h(
    "button",
    {
      class: "task-more",
      type: "button",
      "data-key": `${list}:${task.id}:more`,
      "aria-label": `Actions for ${taskName(task)}`,
      "aria-haspopup": "menu",
      "aria-expanded": "false",
      onclick: (e) => taskMenu(task, list, e),
    },
    h("span", { "aria-hidden": "true" }, "⋮")
  );
  return h(
    "li",
    { class: `task-row${colorOf(task) ? ` color-${colorOf(task)}` : ""}`, "data-task": task.id },
    item,
    more
  );
}

/** A task's actions: from its ⋮ button, a right click or the menu key.
 * Delete is armed as the button in the task view is. */
function taskMenu(task, list, e) {
  const opener = e.currentTarget.closest(".task-row").querySelector(".task-more");
  const wasOpen = $("task-menu")?.opener === opener;
  closeMenu();
  // A second click on the ⋮ that opened the menu closes it.
  if (wasOpen && e.type === "click") return;
  const name = taskName(task);
  const item = (label, action, attrs) =>
    h("button", { class: "menu-item", type: "button", role: "menuitem", onclick: action, ...attrs }, label);

  const busy = task.running;
  const remove = item(busy ? "Delete task (agent at work)" : "Delete task", null, { class: "menu-item danger", disabled: busy });
  remove.addEventListener("click", async () => {
    if (!remove.classList.contains("armed")) {
      remove.classList.add("armed");
      remove.textContent = shown(`Click again to delete ${name}`);
      setTimeout(() => {
        remove.classList.remove("armed");
        remove.textContent = "Delete task";
      }, 4000);
      return;
    }
    closeMenu();
    await removeTask(task);
  });

  const siblings = groupMates(task);
  const at = siblings.findIndex((t) => t.id === task.id);
  // Without a colour of its own, a task in a coloured group shows the group's.
  const swatches = colorSwatches(task.color, (color) => (closeMenu(true), setColor(task, color)), groupOf(task)?.color);
  const group = groupOf(task);
  const items = [
    item("Rename…", () => renameTask(task, menu)),
    ...swatches,
    item(group ? `Group: ${group.name}…` : "Group…", () => pickGroup(task, menu)),
    item("Move up", () => (closeMenu(true), moveTask(task.id, -1)), { disabled: list !== "tasks" || at <= 0, "aria-keyshortcuts": "Alt+ArrowUp" }),
    item("Move down", () => (closeMenu(true), moveTask(task.id, 1)), {
      disabled: list !== "tasks" || at === siblings.length - 1,
      "aria-keyshortcuts": "Alt+ArrowDown",
    }),
    item("Open terminal", () => (closeMenu(), openTerminal(task))),
    item(busy ? "Fork task (agent at work)" : "Fork task", () => (closeMenu(), forkTask(task.id, null)), {
      disabled: busy,
      title: "A new task on the same folder, starting from this one's copy as it is now",
    }),
    item(task.budget ? `Budget: ${money(task.budget)}…` : "Budget…", () => (closeMenu(), openTaskBudget(task.id))),
    item("Limits…", () => (closeMenu(), openTaskLimits(task.id)), { title: limitsSummary(task.limits || {}) }),
    remove,
  ];
  const menu = h(
    "div",
    { class: "menu", id: "task-menu", role: "menu", "aria-label": `Task ${name}` },
    items[0],
    h("div", { class: "menu-swatches", role: "group", "aria-label": "Colour" }, swatches),
    h("hr", { class: "menu-rule" }),
    items.slice(swatches.length + 1, swatches.length + 4),
    h("hr", { class: "menu-rule" }),
    items.slice(swatches.length + 4)
  );
  showMenu(menu, items, opener, e);
}

/** The colour swatches of a task's or a group's menu; `chosen` hears the
 * one picked, `null` for none. */
function colorSwatches(current, chosen, inherited) {
  const none = inherited ? `The group's colour (${inherited})` : "No colour";
  return [null, ...COLORS].map((color) =>
    h("button", {
      class: `swatch ${color ? `color-${color}` : inherited ? `swatch-group color-${inherited}` : "swatch-none"}`,
      type: "button",
      role: "menuitemradio",
      "aria-checked": String((current || null) === color),
      "aria-label": color ? `Colour ${color}` : none,
      title: color || none,
      onclick: () => chosen(color),
    })
  );
}

/** Opens `menu` for `opener`: placed, its first item focused, arrows
 * moving between its items, Escape and Tab closing it. */
function showMenu(menu, items, opener, e) {
  menu.items = items;
  menu.opener = opener;
  opener.setAttribute("aria-expanded", "true");
  document.body.append(menu);
  placeMenu(menu, e);
  items[0].focus();
  menu.addEventListener("keydown", (k) => {
    if (k.target.tagName === "INPUT") return;
    const live = menu.items.filter((i) => !i.disabled && i.isConnected);
    const here = live.indexOf(document.activeElement);
    if (k.key === "Escape") {
      k.preventDefault();
      closeMenu(true);
    } else if (k.key === "ArrowDown" || k.key === "ArrowUp" || k.key === "ArrowRight" || k.key === "ArrowLeft") {
      k.preventDefault();
      const step = k.key === "ArrowDown" || k.key === "ArrowRight" ? 1 : -1;
      live[(here + step + live.length) % live.length].focus();
    } else if (k.key === "Tab") {
      closeMenu(true);
    }
  });
}

/** Under the ⋮ that opened it, at the pointer for a right click, under the
 * task for the menu key; kept on screen. */
function placeMenu(menu, e) {
  const target = e.currentTarget.getBoundingClientRect();
  let x;
  let y;
  if (e.type === "click") {
    x = target.right - menu.offsetWidth;
    y = target.bottom + 2;
  } else if (e.clientX === 0 && e.clientY === 0) {
    x = target.left + 8;
    y = target.bottom;
  } else {
    x = e.clientX;
    y = e.clientY;
  }
  menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - menu.offsetWidth - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - menu.offsetHeight - 8))}px`;
  // It grows out of the corner nearest what opened it.
  menu.style.transformOrigin = e.type === "click" ? "top right" : "top left";
}

function closeMenu(refocus) {
  const menu = $("task-menu");
  if (!menu) return;
  menu.remove();
  menu.closed?.();
  menu.opener?.setAttribute("aria-expanded", "false");
  if (refocus && menu.opener?.isConnected) menu.opener.focus();
}

/** Turns the menu into a field for the task's name. */
function renameTask(task, menu) {
  const input = h("input", {
    id: "task-label",
    type: "text",
    maxlength: 60,
    autocomplete: "off",
    spellcheck: "false",
    value: task.label || "",
    placeholder: "A name for this task",
  });
  const save = async () => {
    closeMenu(true);
    await setLabel(task, input.value);
  };
  input.addEventListener("keydown", (k) => {
    if (k.key === "Enter") {
      k.preventDefault();
      save();
    } else if (k.key === "Escape") {
      k.preventDefault();
      closeMenu(true);
    }
  });
  const saveButton = h("button", { class: "button primary", type: "button", onclick: save }, "Save");
  const clear = h(
    "button",
    { class: "button ghost", type: "button", disabled: !task.label, onclick: () => ((input.value = ""), save()) },
    "Clear"
  );
  menu.items = [saveButton, clear];
  menu.replaceChildren(
    h(
      "label",
      { class: "menu-field", for: "task-label" },
      h("span", { class: "menu-field-label" }, `Name, shown after ${baseName(task.workspace)}`),
      input
    ),
    h("div", { class: "menu-field-actions" }, clear, saveButton)
  );
  input.focus();
  input.select();
}

async function setLabel(task, label) {
  try {
    await call("set_label", { task: task.id, label: label.trim() || null });
  } catch (err) {
    fail(err);
  }
  await refresh();
}

async function setColor(task, color) {
  try {
    await call("set_color", { task: task.id, color });
  } catch (err) {
    fail(err);
  }
  await refresh();
}

/** Moves a task `by` places among the tasks of its group, or of no group,
 * and keeps the order. */
async function moveTask(id, by) {
  const task = state.tasks.find((t) => t.id === id);
  if (!task) return;
  const siblings = groupMates(task);
  const next = siblings[siblings.findIndex((t) => t.id === id) + by];
  if (!next) return;
  await putInOrder(id, state.tasks.indexOf(next));
  // The keyboard stays on the task it moved.
  document.querySelector(`#tasks [data-key="${CSS.escape(`tasks:${id}`)}"]`)?.focus();
}

/** Puts task `id` at index `to` of the list: drawn at once, then kept. */
async function putInOrder(id, to) {
  const tasks = state.tasks.slice();
  const from = tasks.findIndex((t) => t.id === id);
  if (from === -1 || from === to) return;
  tasks.splice(to, 0, ...tasks.splice(from, 1));
  state.tasks = tasks;
  // A refresh already on its way would draw the order before this one.
  refreshToken++;
  renderSidebar();
  try {
    await call("set_order", { tasks: tasks.map((t) => t.id) });
  } catch (err) {
    fail(err);
    await refresh();
  }
}

// --- Groups ------------------------------------------------------------------

/** What a group's policy is called in the list and in Settings. */
const GROUP_POLICIES = {
  default: "the policy for every task",
  balanced: "balanced",
  careful: "careful",
  own: "its own policy",
};

/** Which policy judges `task`'s commits and runs, in words. */
function judgedBy(task) {
  return policyWords(groupOf(task));
}

/** The colour a task shows: its own, else its group's. */
const colorOf = (task) => task.color || groupOf(task)?.color || null;

/** Which policy judges the tasks of `group` (`null` for none), in words. */
function policyWords(group) {
  if (group && group.policy === "own") return "the group's own policy";
  const policy = group && group.policy !== "default" ? group.policy : state.policy;
  return policy === "own" ? "your own policy" : `the ${policy} policy`;
}

/** The group a task is in, when that group is there. */
const groupOf = (task) => (task.group == null ? null : state.groups.find((g) => g.id === task.group) || null);

/** The tasks listed with `task`: its group's, or those in none. */
function groupMates(task) {
  const group = groupOf(task);
  return state.tasks.filter((t) => groupOf(t) === group);
}

/** A folder, open or shut, in the group's colour. */
function folderIcon(shut) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("class", "group-icon");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(ns, "path");
  path.setAttribute(
    "d",
    shut
      ? "M1.5 4.5a1 1 0 0 1 1-1h3.6l1.4 1.5h6a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z"
      : "M1.5 4.5a1 1 0 0 1 1-1h3.6l1.4 1.5h5.5a1 1 0 0 1 1 1v1H4.6a1 1 0 0 0-.95.7L1.5 13.2zM3.6 8.2a1 1 0 0 1 .95-.7h10.2a.6.6 0 0 1 .57.8l-1.6 4.8a1 1 0 0 1-.95.7H1.8z"
  );
  svg.append(path);
  return svg;
}

/** Where tasks in no group are listed once there are groups: dropping a
 * task here takes it out of its group. */
function looseHead(count) {
  return h(
    "li",
    { class: "loose-row" },
    h(
      "div",
      { class: "group-head loose-head", "data-group": "" },
      h("span", { class: "loose-name" }, "Not in a group"),
      h("span", { class: "group-count", "aria-label": plural(count, "task") }, String(count))
    ),
    count ? null : h("p", { class: "group-empty muted" }, "Drag a task here to take it out of its group.")
  );
}

function groupRow(group, members) {
  const policy = group.policy !== "default" ? GROUP_POLICIES[group.policy] : null;
  const head = h(
    "div",
    { class: "group-head", "data-group": String(group.id) },
    h(
      "button",
      {
        class: "group-toggle",
        type: "button",
        "data-key": `group:${group.id}`,
        "aria-expanded": String(!group.collapsed),
        title: policy ? `Its tasks are judged by ${policy}` : "Its tasks are judged by the policy for every task",
        onclick: () => {
          if (!dragged) foldGroup(group);
        },
        oncontextmenu: (e) => {
          e.preventDefault();
          groupMenu(group, e);
        },
      },
      h("span", { class: "group-chevron", "aria-hidden": "true" }, group.collapsed ? "▸" : "▾"),
      folderIcon(group.collapsed),
      h("span", { class: "group-name" }, group.name),
      policy ? h("span", { class: "group-policy" }, group.policy === "own" ? "own policy" : policy) : null,
      h("span", { class: "group-count", "aria-label": plural(members.length, "task") }, String(members.length))
    ),
    h(
      "button",
      {
        class: "task-more",
        type: "button",
        "data-key": `group:${group.id}:more`,
        "aria-label": `Actions for the group ${group.name}`,
        "aria-haspopup": "menu",
        "aria-expanded": "false",
        onclick: (e) => groupMenu(group, e),
      },
      h("span", { "aria-hidden": "true" }, "⋮")
    )
  );
  return h(
    "li",
    { class: `group-row${group.color ? ` color-${group.color}` : ""}` },
    head,
    group.collapsed
      ? null
      : h(
          "ul",
          { class: "task-list group-tasks", "aria-label": group.name },
          members.length
            ? members.map((t) => taskItem(t, "tasks"))
            : h("li", { class: "group-empty muted" }, "Drag a task here, or choose Group… in its menu.")
        )
  );
}

/** Fold or unfold a group's tasks; kept with the group. */
async function foldGroup(group) {
  await saveGroup(group, { collapsed: !group.collapsed });
  document.querySelector(`#tasks [data-key="group:${group.id}"]`)?.focus();
}

/** Changes a group's name, colour or fold, drawn at once, then kept. */
async function saveGroup(group, change) {
  const next = { ...group, ...change };
  state.groups = state.groups.map((g) => (g.id === group.id ? next : g));
  renderSidebar();
  try {
    await call("update_group", { group: group.id, name: next.name, color: next.color || null, collapsed: next.collapsed });
  } catch (err) {
    fail(err);
  }
  await refresh();
}

/** A group's actions: rename, colour, policy, order, delete. */
function groupMenu(group, e) {
  const opener = e.currentTarget.closest(".group-head").querySelector(".task-more");
  const wasOpen = $("task-menu")?.opener === opener;
  closeMenu();
  if (wasOpen && e.type === "click") return;
  const item = (label, action, attrs) =>
    h("button", { class: "menu-item", type: "button", role: "menuitem", onclick: action, ...attrs }, label);
  const at = state.groups.findIndex((g) => g.id === group.id);
  const remove = item("Delete group", null, { class: "menu-item danger" });
  remove.addEventListener("click", async () => {
    if (!remove.classList.contains("armed")) {
      remove.classList.add("armed");
      remove.textContent = "Click again: its tasks stay, in no group";
      setTimeout(() => {
        remove.classList.remove("armed");
        remove.textContent = "Delete group";
      }, 4000);
      return;
    }
    closeMenu();
    await deleteGroup(group);
  });
  const swatches = colorSwatches(group.color, (color) => (closeMenu(true), saveGroup(group, { color })));
  const items = [
    item("Rename…", () =>
      nameField(menu, {
        label: "The group's name",
        value: group.name,
        save: (name) => saveGroup(group, { name }),
      })
    ),
    ...swatches,
    item(`Policy: ${GROUP_POLICIES[group.policy]}…`, () => (closeMenu(), openGroupPolicy(group.id))),
    item("Move up", () => (closeMenu(true), moveGroup(group.id, -1)), { disabled: at <= 0 }),
    item("Move down", () => (closeMenu(true), moveGroup(group.id, 1)), { disabled: at === state.groups.length - 1 }),
    remove,
  ];
  const menu = h(
    "div",
    { class: "menu", id: "task-menu", role: "menu", "aria-label": `Group ${group.name}` },
    items[0],
    h("div", { class: "menu-swatches", role: "group", "aria-label": "Colour" }, swatches),
    h("hr", { class: "menu-rule" }),
    items.slice(swatches.length + 1, swatches.length + 4),
    h("hr", { class: "menu-rule" }),
    remove
  );
  showMenu(menu, items, opener, e);
}

/** Turns `menu` into one field for a name: `save` hears it, trimmed. */
function nameField(menu, { label, value, placeholder, save }) {
  const input = h("input", {
    id: "group-name",
    type: "text",
    maxlength: 60,
    autocomplete: "off",
    spellcheck: "false",
    value: value || "",
    placeholder: placeholder || "",
  });
  const done = async () => {
    const name = input.value.trim();
    if (!name) {
      input.focus();
      return;
    }
    closeMenu(true);
    await save(name);
  };
  input.addEventListener("keydown", (k) => {
    if (k.key === "Enter") {
      k.preventDefault();
      done();
    } else if (k.key === "Escape") {
      k.preventDefault();
      closeMenu(true);
    }
  });
  const saveButton = h("button", { class: "button primary", type: "button", onclick: done }, "Save");
  menu.items = [saveButton];
  menu.replaceChildren(
    h("label", { class: "menu-field", for: "group-name" }, h("span", { class: "menu-field-label" }, label), input),
    h("div", { class: "menu-field-actions" }, saveButton)
  );
  input.focus();
  input.select();
}

/** Turns a task's menu into the groups it can be put in. */
function pickGroup(task, menu) {
  const current = groupOf(task);
  const choice = (label, group) =>
    h(
      "button",
      {
        class: "menu-item",
        type: "button",
        role: "menuitemradio",
        "aria-checked": String((current?.id ?? null) === (group?.id ?? null)),
        onclick: () => (closeMenu(true), setTaskGroup(task, group)),
      },
      group ? h("span", { class: `menu-dot${group.color ? ` color-${group.color}` : ""}`, "aria-hidden": "true" }) : null,
      label
    );
  const items = [
    ...state.groups.map((g) => choice(g.name, g)),
    choice("No group", null),
    h(
      "button",
      {
        class: "menu-item",
        type: "button",
        role: "menuitem",
        onclick: () =>
          nameField(menu, {
            label: `A new group for ${taskName(task)}`,
            placeholder: "Clients, side projects…",
            save: async (name) => {
              const group = await createGroup(name);
              if (group) await setTaskGroup(task, group);
            },
          }),
      },
      "New group…"
    ),
  ];
  menu.items = items;
  menu.replaceChildren(
    h("p", { class: "menu-note" }, "Its commits and runs are judged by the group's policy."),
    ...items.slice(0, -1),
    h("hr", { class: "menu-rule" }),
    items[items.length - 1]
  );
  (items.find((i) => i.getAttribute("aria-checked") === "true") || items[0]).focus();
}

async function createGroup(name) {
  try {
    const group = await call("create_group", { name, color: null });
    toast(`Group ${group.name} made; its tasks are judged by the policy for every task until you choose another`);
    await refresh();
    return group;
  } catch (err) {
    fail(err);
    return null;
  }
}

/** Puts a task in `group`, or in none, once the person has seen what that
 * changes; `anchor` is where a question about it shows. */
async function setTaskGroup(task, group, anchor) {
  const impact = await regroupImpact(task, group, anchor);
  if (impact) await applyGroup(task, group, impact);
  else await refresh();
}

/** What putting `task` in `group` changes; asked of the person when the
 * verdict on any of its pending changes would change. `null` when they
 * keep it where it is. */
async function regroupImpact(task, group, anchor) {
  let impact;
  try {
    impact = await call("group_impact", { task: task.id, group: group ? group.id : null });
  } catch (err) {
    fail(err);
    return null;
  }
  if (!impact.changes.length) return impact;
  return (await confirmRegroup(task, group, impact, anchor)) ? impact : null;
}

async function applyGroup(task, group, impact) {
  const name = taskName(task);
  try {
    await call("set_task_group", { task: task.id, group: group ? group.id : null });
    const where = group ? `${name} is in ${group.name}` : `${name} is in no group`;
    let said = impact.same
      ? `${where}; it is judged as before`
      : `${where}: ${policyWords(group)} judges its next commit and run, not ${policyWords(groupOf(task))}`;
    if (!impact.same && impact.running) said += "; the agent at work keeps its connections' policy until its run ends";
    toast(said);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

/** How much a verdict lets through: a lower one, more. */
const STRICTNESS = { allow: 0, ask: 1, deny: 2 };

/** A small question by `anchor`: the move and what it changes for the
 * task's pending changes. Resolves true to move. */
function confirmRegroup(task, group, impact, anchor) {
  return new Promise((resolve) => {
    closeMenu();
    const names = (list) => {
      const shown = list.slice(0, 4).map((c) => c.subject).join(", ");
      return list.length > 4 ? `${shown} and ${list.length - 4} more` : shown;
    };
    const looser = impact.changes.filter((c) => STRICTNESS[c.then] < STRICTNESS[c.now]);
    const stricter = impact.changes.filter((c) => STRICTNESS[c.then] > STRICTNESS[c.now]);
    const passing = looser.filter((c) => c.then === "allow");
    const asked = looser.filter((c) => c.then !== "allow");
    const lines = [];
    if (passing.length)
      lines.push(h("p", { class: "regroup-warn" }, `${plural(passing.length, "change")} waiting for you would pass on its own: ${names(passing)}`));
    if (asked.length)
      lines.push(h("p", { class: "regroup-warn" }, `${plural(asked.length, "change")} refused now would be put to you: ${names(asked)}`));
    if (stricter.length)
      lines.push(h("p", { class: "regroup-note" }, `${plural(stricter.length, "change")} that ${stricter.length === 1 ? "passes" : "pass"} now would wait for you: ${names(stricter)}`));
    if (impact.running) lines.push(h("p", { class: "muted" }, "The agent at work keeps its connections' policy until its run ends."));
    let answered = false;
    const answer = (yes) => {
      if (answered) return;
      answered = true;
      closeMenu(true);
      resolve(yes);
    };
    const move = h("button", { class: `button ${looser.length ? "ask" : "primary"}`, type: "button", onclick: () => answer(true) }, "Move");
    const keep = h("button", { class: "button ghost", type: "button", onclick: () => answer(false) }, "Keep it here");
    const menu = h(
      "div",
      { class: "menu regroup", id: "task-menu", role: "alertdialog", "aria-label": `Move ${taskName(task)}` },
      h("p", { class: "regroup-title" }, `Move ${taskName(task)} ${group ? `to ${group.name}` : "out of its group"}?`),
      h("p", { class: "muted" }, `It would be judged by ${policyWords(group)}, not ${policyWords(groupOf(task))}.`),
      lines,
      h("div", { class: "menu-field-actions" }, keep, move)
    );
    const opener = anchor || document.querySelector(`#tasks .task-row[data-task="${CSS.escape(task.id)}"] .task-more`) || $("new-group");
    showMenu(menu, [keep, move], opener, { type: "click", currentTarget: opener });
    // Closed any other way (Escape, a click elsewhere): it stays.
    menu.closed = () => answer(false);
    (looser.length ? keep : move).focus();
  });
}

/** Moves a group `by` places, and keeps the order. */
async function moveGroup(id, by) {
  const groups = state.groups.slice();
  const from = groups.findIndex((g) => g.id === id);
  const to = from + by;
  if (from === -1 || to < 0 || to >= groups.length) return;
  groups.splice(to, 0, ...groups.splice(from, 1));
  state.groups = groups;
  refreshToken++;
  renderSidebar();
  try {
    await call("set_group_order", { groups: groups.map((g) => g.id) });
  } catch (err) {
    fail(err);
  }
  await refresh();
  document.querySelector(`#tasks [data-key="group:${id}"]`)?.focus();
}

async function deleteGroup(group) {
  try {
    await call("delete_group", { group: group.id });
    toast(`Group ${group.name} deleted; its tasks are in no group now`);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

/** Asks for a new group's name under the button that makes one. */
function newGroup() {
  const button = $("new-group");
  closeMenu();
  const menu = h("div", { class: "menu", id: "task-menu", role: "dialog", "aria-label": "New group" });
  const placeholder = h("span");
  showMenu(menu, [placeholder], button, { type: "click", currentTarget: button });
  nameField(menu, { label: "A new group", placeholder: "Clients, side projects…", save: createGroup });
  placeMenu(menu, { type: "click", currentTarget: button });
}

// --- Dragging a task in the list ---------------------------------------------

// Pointer events rather than HTML drag and drop, which the window's own file
// drop handling can swallow.

/** The task being dragged: { id, startY, row, moved }. */
let drag = null;
/** True from a drag's end to the click it ends with. */
let dragged = false;

function dragStart(e, id) {
  if (e.button !== 0 || e.pointerType === "touch") return;
  drag = { id, startY: e.clientY, row: e.currentTarget.closest(".task-row"), moved: false, pointer: e.pointerId };
}

function dragMove(e) {
  if (!drag || e.pointerId !== drag.pointer) return;
  if (!drag.moved) {
    if (Math.abs(e.clientY - drag.startY) < 6) return;
    drag.moved = true;
    closeMenu();
    drag.row.classList.add("dragging");
    document.body.classList.add("dragging-task");
  }
  const targets = dropTargets();
  for (const el of targets) el.classList.remove("drop-before", "drop-after", "drop-into");
  const over = targets.find((el) => e.clientY < el.getBoundingClientRect().bottom);
  if (over) over.classList.add(over.classList.contains("group-head") ? "drop-into" : "drop-before");
  else if (targets.length) targets[targets.length - 1].classList.add("drop-after");
}

/** Where a dragged task can land, top to bottom: tasks, and the heads of
 * groups, which take it in. */
const dropTargets = () => [...$("tasks").querySelectorAll(".task-row, .group-head")];

async function dragEnd(e) {
  if (!drag || e.pointerId !== drag.pointer) return;
  const { id, moved, row } = drag;
  drag = null;
  if (!moved) return;
  dragged = true;
  setTimeout(() => (dragged = false), 0);
  document.body.classList.remove("dragging-task");
  row.classList.remove("dragging");
  const targets = dropTargets();
  const before = targets.find((el) => el.classList.contains("drop-before"));
  const into = targets.find((el) => el.classList.contains("drop-into"));
  for (const el of targets) el.classList.remove("drop-before", "drop-after", "drop-into");
  if (e.type === "pointercancel") {
    renderSidebar();
    return;
  }
  const task = state.tasks.find((t) => t.id === id);
  const from = state.tasks.indexOf(task);
  // Before a task: in its group, in its place. On a group's head: first in
  // that group. Past the end: last, in no group.
  let group = null;
  let to = state.tasks.length;
  if (before) {
    const other = state.tasks.find((t) => t.id === before.dataset.task);
    group = groupOf(other);
    to = state.tasks.indexOf(other);
  } else if (into) {
    group = state.groups.find((g) => String(g.id) === into.dataset.group) || null;
    const first = state.tasks.find((t) => t !== task && groupOf(t) === group);
    to = first ? state.tasks.indexOf(first) : from;
  }
  // The dragged task leaves its place first.
  if (to > from) to -= 1;
  if (group === groupOf(task)) {
    await putInOrder(id, to);
    return;
  }
  // Into another group only once the person has seen what that changes.
  renderSidebar();
  const anchor = $("tasks").querySelector(`.group-head[data-group="${group ? group.id : ""}"]`);
  const impact = await regroupImpact(task, group, anchor);
  if (!impact) return;
  await putInOrder(id, to);
  await applyGroup(task, group, impact);
}

function renderSidebar() {
  // A list redrawn under a drag would lose the rows it moves between.
  if (drag?.moved) return;
  const waiting = state.tasks.filter(
    (t) => questionsOf(t.id).length || (!t.running && (t.verdict === "ask" || t.verdict === "deny"))
  );
  // Rows slide to where the redraw puts them; a new one arrives, unless
  // the list is being filled for the first time.
  const filled = Boolean($("tasks").querySelector(".task-row"));
  glide($("panel-tasks"), ".task-row, .group-head, .nav-title-row", sidebarKey, () => {
    $("inbox-section").hidden = waiting.length === 0;
    replace($("inbox"), ...waiting.map((t) => taskItem(t, "inbox")));
    // The groups in their order, each with its tasks; then those in none,
    // under a head of their own once there are groups.
    const loose = state.tasks.filter((t) => !groupOf(t));
    replace(
      $("tasks"),
      ...state.groups.map((g) => groupRow(g, state.tasks.filter((t) => groupOf(t) === g))),
      state.groups.length ? looseHead(loose.length) : null,
      ...loose.map((t) => taskItem(t, "tasks"))
    );
  }, filled);
  $("tasks-empty").hidden = state.tasks.length > 0 || state.groups.length > 0;
}

/** What a sidebar row is, across redraws: a task in a list, a group, the title. */
function sidebarKey(el) {
  if (el.classList.contains("task-row")) return el.querySelector(".task-item")?.dataset.key;
  if (el.classList.contains("group-head")) return `group:${el.dataset.group}`;
  return "tasks-title";
}

// --- Task view -------------------------------------------------------------

function renderTask() {
  extensionsSeeTask();
  const settings = SETTINGS.includes(state.view);
  $("titlebar-title").textContent = settings ? "Settings — Kule" : "Kule";
  $("settings").hidden = !settings;
  $("open-settings").setAttribute("aria-current", settings ? "page" : "false");
  for (const tab of document.querySelectorAll("#settings-tabs .tab")) {
    const selected = tab.dataset.view === state.view;
    tab.setAttribute("aria-selected", String(selected));
    tab.tabIndex = selected ? 0 : -1;
    if (selected) $("agents-view").setAttribute("aria-labelledby", tab.id);
  }
  if (settings) {
    $("empty").hidden = true;
    $("task").hidden = true;
    renderLayout();
    return;
  }
  const task = currentTask();
  $("empty").hidden = Boolean(task);
  $("task").hidden = !task;
  renderLayout();
  if (!task) return;

  $("task-name").textContent = shown(taskName(task));
  $("titlebar-title").textContent = shown(`${taskName(task)} — Kule`);
  const mark = $("task-name").closest(".task-header");
  // A line runs along the header's foot while the agent is at work.
  mark.className = `task-header${colorOf(task) ? ` color-${colorOf(task)}` : ""}${task.running ? " running" : ""}`;
  const group = groupOf(task);
  $("task-path").textContent = shown(`${task.workspace} · task ${task.id} · ${when(task.created_unix)}${group ? ` · in ${group.name}` : ""} · ${judgedBy(task)}`);
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
  const waiting = task.changes + (task.home_changes || 0);
  if (count) {
    count.hidden = !waiting;
    count.textContent = String(waiting);
  }

  renderComposer(task);
  renderQuestions();
  renderStream(task.id);
  loadStream(task.id);
  if (isShown("review")) loadReview(task.id);
  if (isShown("timeline")) loadTimeline(task.id);
  if (isShown("files")) loadFiles(task.id);
  if (isShown("preview")) loadPreview(task);
  if (isShown("terminal")) showTerminal(task.id);
}

function renderComposer(task) {
  const busy = task.running;
  $("send").disabled = busy || state.agents.length === 0;
  $("prompt").disabled = busy;
  $("agent").disabled = busy;
  $("timeout").value = String(Math.round(task.approval_timeout_secs / 60));
  $("send").hidden = busy;
  renderStop(task);
  renderSpent(task);
  renderMeasure(task);
  renderPromptRefs(task);
}

function money(m) {
  return `${m.amount.toFixed(2)} ${m.currency}`;
}

/** What a task's runs spent, with the run at work now, and what is left. */
function renderSpent(task) {
  const spent = task.spent || { runs: 0, tokens: 0, cost: [] };
  const live = task.running ? liveCost.get(task.id) : null;
  const cost = spent.cost.map((m) => ({ ...m }));
  if (live) {
    const same = cost.find((m) => m.currency === live.currency);
    if (same) same.amount += live.amount;
    else cost.push({ ...live });
  }
  const chip = $("task-spent");
  chip.hidden = spent.runs === 0 && !live && !task.budget;
  chip.className = "chip";
  const parts = [];
  if (cost.length > 0) parts.push(`${cost.map(money).join(" + ")} spent`);
  else if (spent.tokens > 0) parts.push(`${spent.tokens.toLocaleString()} tokens`);
  if (task.budget) {
    const used = cost.filter((m) => m.currency === task.budget.currency).reduce((a, m) => a + m.amount, 0);
    const rest = Math.max(0, task.budget.amount - used);
    parts.push(`${money({ amount: rest, currency: task.budget.currency })} left`);
    if (rest <= 0) chip.classList.add("deny");
    else if (rest < task.budget.amount * 0.2) chip.classList.add("ask");
  }
  chip.textContent = parts.join(" · ") || "nothing spent yet";
}

/** Settings' sections, each a value of `state.view`. */
const SETTINGS = ["agents", "policy", "rules", "secrets", "budget", "limits", "appearance", "shortcuts", "extensions", "profile"];

async function openSettings(view = SETTINGS.includes(state.view) ? state.view : "agents") {
  closeMenu();
  const opening = { agents: openAgents, policy: openPolicy, rules: openRules, secrets: openSecrets, budget: openBudget, limits: openLimits, appearance: openAppearance, shortcuts: openShortcuts, extensions: openExtensions, profile: openProfile }[view];
  await opening();
}

// --- Policy ----------------------------------------------------------------

const POLICIES = [
  {
    choice: "balanced",
    name: "Balanced",
    text: "Changes to files pass on their own. What decides what gets installed, built or run (manifests, lockfiles, CI, editor settings), git history rewrites and mass deletes wait for you.",
  },
  {
    choice: "careful",
    name: "Careful",
    text: "Every change to a file or to git waits for you. Only package registries and the agents' own APIs are reached without asking.",
  },
];

/** Kule's two policies to choose from, or a note when the person wrote
 * their own; `idPrefix` keeps two pickers' ids apart. */
function policyPicker(choice, idPrefix, chosen) {
  if (choice === "own") {
    return h(
      "p",
      { class: "muted" },
      "Your own policy in ~/.kule/policy.cedar holds; Kule does not replace it. Move it away to choose one of Kule's."
    );
  }
  return h(
    "div",
    { class: "policy-pick", role: "radiogroup", "aria-label": "Policy" },
    POLICIES.map((p) =>
      h(
        "label",
        { class: "policy-option", for: `${idPrefix}-${p.choice}` },
        h("input", {
          type: "radio",
          name: `${idPrefix}-policy`,
          id: `${idPrefix}-${p.choice}`,
          "data-key": `${idPrefix}-${p.choice}`,
          value: p.choice,
          checked: p.choice === choice,
          onchange: () => choosePolicy(p.choice, chosen),
        }),
        h("span", { class: "policy-name" }, p.name),
        h("span", { class: "policy-text" }, p.text)
      )
    )
  );
}

async function choosePolicy(choice, chosen) {
  try {
    state.policy = await call("choose_policy", { choice });
    toast(`The ${choice} policy holds from the next commit`);
  } catch (err) {
    fail(err);
  }
  await chosen();
}

async function openPolicy() {
  state.view = "policy";
  renderSidebar();
  renderTask();
  await renderPolicy();
}

// --- Isolation ---------------------------------------------------------------

// What this computer cannot give an agent's commands. With a gap and no
// acceptance no agent runs; accepted, the title bar says so while it holds.

let isolation = { gaps: [], accept_weaker: false };

async function loadIsolation() {
  try {
    isolation = await call("isolation");
  } catch (err) {
    fail(err);
  }
  renderIsolation();
}

async function setIsolation(accept) {
  try {
    isolation = await call("set_isolation", { acceptWeaker: accept });
    toast(accept ? "Agents run with weaker isolation on this computer" : "Agents run only fully isolated");
  } catch (err) {
    fail(err);
  }
  renderIsolation();
  renderPolicy();
}

function renderIsolation() {
  const chip = $("isolation-warning");
  chip.hidden = !isolation.gaps.length;
  chip.textContent = isolation.accept_weaker ? "Weaker isolation" : "Agents held: isolation";
  chip.classList.toggle("deny", !isolation.accept_weaker);
  chip.title = `This computer cannot give an agent's commands ${isolation.gaps.join("; nor ")}.`;
}

/** Policy's word on isolation, where this computer leaves a gap. */
function isolationSection() {
  if (!isolation.gaps.length) return null;
  return h(
    "section",
    { class: "isolation", "aria-label": "Isolation" },
    h("h3", {}, "Isolation on this computer"),
    h("p", {}, "It cannot give an agent's commands:"),
    h("ul", {}, isolation.gaps.map((gap) => h("li", {}, gap))),
    h(
      "p",
      { class: "muted" },
      "Kule runs no agent less isolated than it means to without your say. On Ubuntu, allowing unprivileged user namespaces closes most of it: sudo sysctl kernel.apparmor_restrict_unprivileged_userns=0."
    ),
    h(
      "label",
      { class: "isolation-accept", for: "isolation-accept" },
      h("input", {
        type: "checkbox",
        id: "isolation-accept",
        "data-key": "isolation-accept",
        checked: isolation.accept_weaker,
        onchange: (e) => setIsolation(e.currentTarget.checked),
      }),
      " Run agents with what this computer gives"
    )
  );
}

async function renderPolicy() {
  if (state.view !== "policy") return;
  let choice = "balanced";
  try {
    choice = await call("policy");
  } catch (err) {
    fail(err);
  }
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Policy"),
      h(
        "p",
        { class: "muted" },
        "What passes on its own, what waits for you and what never passes, for every task. A repository can add its own refusals in .kule/policy.cedar; what you told Kule not to ask about again is under Rules."
      )
    ),
    isolationSection(),
    policyPicker(choice, "settings", renderPolicy),
    groupPolicies(choice)
  );
  if (focusGroup != null) {
    const select = document.querySelector(`#agents-view [data-key="group-policy:${focusGroup}"]`);
    select?.focus();
    spotlight(select?.closest("li"));
    focusGroup = null;
  }
}

/** The group whose policy Settings opens on. */
let focusGroup = null;

/** Opens Settings' Policy on group `id`'s choice. */
async function openGroupPolicy(id) {
  focusGroup = id;
  await openPolicy();
}

/** Each group's policy: the one above, one of Kule's, or its own. */
function groupPolicies(choice) {
  const head = h(
    "header",
    { class: "agents-head group-policies-head" },
    h("h2", {}, "Groups"),
    h(
      "p",
      { class: "muted" },
      "A group's tasks can be judged by a policy of their own: Kule's balanced or careful one whatever holds above, or one you write. A repository's refusals and what you told Kule not to ask about again still hold. A change counts from the task's next commit and run."
    )
  );
  if (!state.groups.length) {
    return h("section", { class: "group-policies" }, head, h("p", { class: "muted" }, "No groups yet. Make one with the + next to Tasks, or from a task's menu: Group…"));
  }
  const above = choice === "own" ? "your own" : choice;
  return h(
    "section",
    { class: "group-policies" },
    head,
    h(
      "ul",
      { class: "group-policy-list" },
      state.groups.map((group) => {
        const editor = h("div", { class: "group-policy-editor" });
        const select = h(
          "select",
          {
            id: `group-policy-${group.id}`,
            "data-key": `group-policy:${group.id}`,
            onchange: () => {
              if (select.value === "own") openOwnPolicy(group, editor);
              else setGroupPolicy(group, select.value, null);
            },
          },
          [
            ["default", `The one above (${above})`],
            ["balanced", "Balanced"],
            ["careful", "Careful"],
            ["own", "Its own…"],
          ].map(([value, label]) => h("option", { value, selected: group.policy === value }, label))
        );
        const row = h(
          "li",
          { class: `group-policy-row${group.color ? ` color-${group.color}` : ""}` },
          h(
            "div",
            { class: "group-policy-line" },
            h("label", { class: "group-policy-name", for: `group-policy-${group.id}` }, group.name),
            h("span", { class: "muted" }, plural(state.tasks.filter((t) => groupOf(t) === group).length, "task")),
            select,
            group.policy === "own"
              ? h("button", { class: "button ghost", type: "button", onclick: () => openOwnPolicy(group, editor) }, "Edit")
              : null
          ),
          editor
        );
        return row;
      })
    )
  );
}

/** Opens the editor for a group's own policy: what was written, or Kule's
 * balanced policy to start from. */
async function openOwnPolicy(group, editor) {
  let draft;
  try {
    draft = await call("group_policy_text", { group: group.id });
  } catch (err) {
    fail(err);
    return;
  }
  const area = h("textarea", {
    class: "group-policy-text",
    "data-key": `group-policy-text:${group.id}`,
    rows: 14,
    spellcheck: "false",
    autocomplete: "off",
    "aria-label": `${group.name}'s own policy, in Cedar`,
  });
  area.value = draft.written ?? draft.balanced;
  const error = h("p", { class: "group-policy-error", role: "alert", hidden: true });
  const use = h("button", { class: "button primary", type: "button" }, "Check and use");
  use.addEventListener("click", async () => {
    error.hidden = true;
    try {
      await call("set_group_policy", { group: group.id, policy: "own", text: area.value });
      toast(`${group.name}: its own policy judges its tasks' next commits and runs`);
      await refresh();
      await renderPolicy();
    } catch (err) {
      // Cedar's message, by the text it is about.
      error.textContent = shown(String(err));
      error.hidden = false;
    }
  });
  const cancel = h("button", { class: "button ghost", type: "button", onclick: () => renderPolicy() }, "Cancel");
  editor.replaceChildren(
    h(
      "p",
      { class: "muted" },
      draft.written
        ? "The policy written for this group, in Cedar. It replaces the one above for its tasks."
        : "Starts from Kule's balanced policy, in Cedar. It replaces the one above for this group's tasks; a forbid can never be overridden."
    ),
    area,
    error,
    h("div", { class: "group-policy-actions" }, cancel, use)
  );
  area.focus();
}

async function setGroupPolicy(group, policy, text) {
  try {
    await call("set_group_policy", { group: group.id, policy, text });
    toast(`${group.name}: ${GROUP_POLICIES[policy]} judges its tasks' next commits and runs`);
  } catch (err) {
    fail(err);
  }
  await refresh();
  await renderPolicy();
}

// --- First run ---------------------------------------------------------------

/** How each built-in agent is signed in, outside Kule. */
const SIGN_IN = {
  "claude-code": "run claude, then /login",
  codex: "run codex login",
  gemini: "run gemini and sign in",
};

/** What to install when an agent's program is not on PATH. */
function installHint(program) {
  if (program === "npx") return "needs Node.js (npx)";
  if (program === "gemini") return "needs the Gemini CLI: npm install -g @google/gemini-cli";
  return `needs ${program}, which is not on your PATH`;
}

/** The empty window's setup: the agents and whether they can start, the
 * policy, and a folder. */
async function loadWelcome() {
  let listed = [];
  let choice = "balanced";
  try {
    [listed, choice] = await Promise.all([call("agent_list"), call("policy")]);
  } catch (err) {
    fail(err);
  }
  const ours = listed.filter((l) => l.origin === "built_in" || SIGN_IN[l.profile.name]);
  replace(
    $("setup-agents"),
    ...ours.map((l) => {
      const name = l.profile.name;
      const [mark, text] = l.missing
        ? ["deny", installHint(l.missing)]
        : l.held
          ? ["ask", l.held]
          : ["allow", SIGN_IN[name] ? `ready to start · to sign in, ${SIGN_IN[name]}` : "ready to start"];
      return h(
        "li",
        { class: "setup-agent" },
        h("span", { class: `chip ${mark}` }, mark === "allow" ? "found" : mark === "ask" ? "held" : "missing"),
        h("strong", {}, name),
        h("span", { class: "muted" }, text)
      );
    })
  );
  replace($("setup-policy"), policyPicker(choice, "setup", loadWelcome));
  // Step by step: a step done is ticked, the one to take now stands out,
  // those after it wait. A policy is always chosen, Balanced at first.
  const done = [ours.some((l) => !l.missing && !l.held), true, false];
  const now = done.indexOf(false);
  document.querySelectorAll("#empty .setup-step").forEach((step, i) => {
    step.classList.toggle("done", done[i]);
    step.classList.toggle("now", i === now);
    step.classList.toggle("later", i > now && !done[i]);
  });
}

async function openBudget() {
  state.view = "budget";
  renderSidebar();
  renderTask();
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Budget"),
      h(
        "p",
        { class: "muted" },
        "What every task may cost together in any 24 hours, as the agents report it. No run starts anywhere once it is spent, and a run is held to what is left. Each task can have its own budget too, below; the tighter of the two holds."
      )
    ),
    h(
      "div",
      { class: "agent-card daily" },
      h(
        "label",
        { class: "daily-pick", for: "daily-budget" },
        "Daily budget",
        h("input", {
          id: "daily-budget",
          type: "number",
          min: 0,
          step: 1,
          inputmode: "decimal",
          placeholder: "none",
          "aria-describedby": "daily-spent",
          onchange: setDailyBudget,
        }),
        h("span", { id: "daily-currency" }, "USD")
      ),
      h("span", { class: "daily-spent", id: "daily-spent", "aria-live": "polite" }),
      h("span", { class: "muted" }, "Empty for no limit.")
    ),
    taskBudgets()
  );
  await loadDaily();
  if (focusBudget) {
    const input = document.querySelector(`#agents-view [data-key="budget:${CSS.escape(focusBudget)}"]`);
    input?.focus();
    spotlight(input?.closest("li"));
    focusBudget = null;
  }
}

/** The task whose budget Settings opens on. */
let focusBudget = null;

/** Opens Settings' Budget on task `id`'s budget. */
async function openTaskBudget(id) {
  focusBudget = id;
  await openBudget();
}

/** Each task's own budget, and what its runs spent. */
function taskBudgets() {
  const head = h(
    "header",
    { class: "agents-head task-budgets-head" },
    h("h2", {}, "Each task"),
    h(
      "p",
      { class: "muted" },
      "What a task's runs may cost together, as its agents report it. None starts once it is spent. Claude Code reports the cost as each turn ends, so a turn may go over and the next run is refused; an agent that reports it as it works is stopped when it gets there. Agents that report no cost are not held by it. Empty for no limit."
    )
  );
  if (!state.tasks.length) return h("section", { class: "task-budgets" }, head, h("p", { class: "muted" }, "No tasks yet."));
  return h(
    "section",
    { class: "task-budgets" },
    head,
    h(
      "ul",
      { class: "task-budget-list" },
      state.tasks.map((task) => {
        const spent = task.spent || { runs: 0, tokens: 0, cost: [] };
        const used = spent.cost.length
          ? `${spent.cost.map(money).join(" + ")} spent`
          : spent.tokens
            ? `${spent.tokens.toLocaleString()} tokens`
            : "nothing spent yet";
        const currency = task.budget ? task.budget.currency : "USD";
        const input = h("input", {
          id: `budget-${task.id}`,
          "data-key": `budget:${task.id}`,
          type: "number",
          min: 0,
          step: 0.5,
          inputmode: "decimal",
          placeholder: "none",
          value: task.budget ? String(task.budget.amount) : "",
          disabled: task.running,
          onchange: () => setTaskBudget(task, input.value, currency),
        });
        return h(
          "li",
          { class: `task-budget-row${colorOf(task) ? ` color-${colorOf(task)}` : ""}` },
          h("label", { class: "task-budget-name", for: `budget-${task.id}` }, taskName(task), h("span", { class: "muted" }, ` · ${task.id}`)),
          h("span", { class: "muted task-budget-spent" }, used),
          h("span", { class: "task-budget-field" }, input, currency)
        );
      })
    )
  );
}

async function setTaskBudget(task, text, currency) {
  const value = text.trim();
  const budget = value === "" ? null : { amount: Number(value), currency };
  try {
    await call("set_budget", { task: task.id, budget });
    toast(budget ? `${taskName(task)} may spend ${money(budget)}` : `${taskName(task)} has no budget of its own`);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

/** What every task spent in the last 24 hours, against the daily budget;
 * shown while Settings has Budget open. */
function renderDaily(daily) {
  if (!$("daily-budget")) return;
  const { budget, spent } = daily;
  if (document.activeElement !== $("daily-budget")) {
    $("daily-budget").value = budget ? String(budget.amount) : "";
  }
  $("daily-currency").textContent = budget ? budget.currency : "USD";
  const el = $("daily-spent");
  el.className = "daily-spent";
  const cost = spent.cost.length ? spent.cost.map(money).join(" + ") : "nothing reported";
  if (!budget) {
    el.textContent = `Last 24 h: ${cost}`;
    return;
  }
  const used = spent.cost.filter((m) => m.currency === budget.currency).reduce((a, m) => a + m.amount, 0);
  const rest = Math.max(0, budget.amount - used);
  el.textContent = `Last 24 h: ${cost} · ${money({ amount: rest, currency: budget.currency })} left`;
  if (rest <= 0) el.classList.add("deny");
  else if (rest < budget.amount * 0.2) el.classList.add("ask");
}

async function loadDaily() {
  if (state.view !== "budget") return;
  try {
    renderDaily(await call("daily_budget"));
  } catch (err) {
    fail(err);
  }
}

async function setDailyBudget() {
  const text = $("daily-budget").value.trim();
  const currency = $("daily-currency").textContent || "USD";
  const budget = text === "" ? null : { amount: Number(text), currency };
  try {
    renderDaily(await call("set_daily_budget", { budget }));
  } catch (err) {
    fail(err);
    await loadDaily();
  }
}

// --- Limits ----------------------------------------------------------------

/** The limits on a run, each in the unit a person sets it in; the daemon
 * keeps them in `stored` units (`to` and `from` convert). */
const LIMIT_FIELDS = [
  {
    key: "cpu_percent",
    label: "Processor",
    unit: "cores",
    step: 0.5,
    to: (v) => Math.round(v * 100),
    from: (n) => n / 100,
    help: "Over it, the run is slowed down, not stopped: its processes are paused and resumed in turn.",
  },
  {
    key: "memory_mb",
    label: "Memory",
    unit: "GB",
    step: 0.5,
    to: (v) => Math.round(v * 1024),
    from: (n) => n / 1024,
    help: "What the run's processes hold together. Over it, they are killed at once.",
  },
  {
    key: "disk_mb",
    label: "Disk",
    unit: "GB",
    step: 1,
    to: (v) => Math.round(v * 1024),
    from: (n) => n / 1024,
    help: "What the task's copy, temporary files and home changes may grow by since the task was made.",
  },
  {
    key: "run_minutes",
    label: "One run",
    unit: "min",
    step: 5,
    to: (v) => Math.round(v),
    from: (n) => n,
    help: "How long one run may take, start to end.",
  },
  {
    key: "task_minutes",
    label: "All of a task's runs",
    unit: "h",
    step: 1,
    to: (v) => Math.round(v * 60),
    from: (n) => n / 60,
    help: "How long a task's runs may take together; none starts once it is used up.",
  },
  {
    key: "idle_minutes",
    label: "Doing nothing",
    unit: "min",
    step: 1,
    to: (v) => Math.round(v),
    from: (n) => n,
    help: "How long the agent may say, run and print nothing before the run stops. A question waiting on you does not count.",
  },
];

/** A limit as a person reads it: "2.5 GB", or "none". */
function limitText(field, stored) {
  if (stored === null || stored === undefined) return "none";
  const v = field.from(stored);
  return `${Number.isInteger(v) ? v : v.toFixed(v < 1 ? 2 : 1)} ${field.unit}`;
}

/** Bytes as a person reads them. */
function bytesText(n) {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  return `${Math.round(n / 1024 ** 2)} MB`;
}

/** The limits in force for a task, said in a line. */
function limitsSummary(limits) {
  const set = LIMIT_FIELDS.filter((f) => limits[f.key] != null).map((f) => `${f.label.toLowerCase()} ${limitText(f, limits[f.key])}`);
  if (limits.gpu === false) set.push("no GPU");
  return set.length ? set.join(" · ") : "no limits";
}

/** The task whose limits Settings opens on. */
let focusLimits = null;

async function openTaskLimits(id) {
  focusLimits = id;
  await openLimits();
}

async function openLimits() {
  state.view = "limits";
  renderSidebar();
  renderTask();
  let every = {};
  try {
    every = await call("limits");
  } catch (err) {
    fail(err);
  }
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Limits"),
      h(
        "p",
        { class: "muted" },
        "What each run may take of this computer. Over the processor limit a run is slowed down; over memory, disk or time it is stopped, and Live says why. These hold for every task that sets none of its own, below. A run going on keeps the limits it started with."
      )
    ),
    h(
      "div",
      { class: "agent-card limits-card" },
      h(
        "div",
        { class: "limits-grid" },
        LIMIT_FIELDS.map((f) =>
          h(
            "label",
            { class: "limit-field", title: f.help },
            h("span", { class: "limit-label" }, f.label),
            h(
              "span",
              { class: "limit-value" },
              h("input", {
                id: `limit-${f.key}`,
                "data-key": `limit:${f.key}`,
                type: "number",
                min: 0,
                step: f.step,
                inputmode: "decimal",
                placeholder: "none",
                value: every[f.key] != null ? String(f.from(every[f.key])) : "",
                onchange: setEveryLimits,
              }),
              f.unit
            )
          )
        ),
        h(
          "label",
          { class: "limit-field", title: "Whether the run's programs may use the GPU (Metal on macOS; /dev/dri and NVIDIA's devices on Linux). Only on or off: no system lets a share of it be set." },
          h("span", { class: "limit-label" }, "GPU"),
          h(
            "span",
            { class: "limit-value" },
            h("input", { id: "limit-gpu", "data-key": "limit:gpu", type: "checkbox", checked: every.gpu !== false, onchange: setEveryLimits }),
            "may be used"
          )
        )
      ),
      h("span", { class: "muted" }, "Empty for no limit.")
    ),
    taskLimits(every)
  );
  if (focusLimits) {
    const row = document.querySelector(`#agents-view [data-task="${CSS.escape(focusLimits)}"]`);
    if (row) {
      row.open = true;
      row.querySelector("summary")?.focus();
      spotlight(row);
    }
    focusLimits = null;
  }
}

async function setEveryLimits() {
  const limits = { gpu: $("limit-gpu").checked };
  for (const f of LIMIT_FIELDS) {
    const text = $(`limit-${f.key}`).value.trim();
    limits[f.key] = text === "" ? null : f.to(Number(text));
  }
  try {
    await call("set_limits", { limits });
    toast(`Every task: ${limitsSummary(limits)}`);
  } catch (err) {
    fail(err);
  }
  await refresh();
  if (state.view === "limits") await openLimits();
}

/** Each task's own limits, field by field: every task's, none, or its own. */
function taskLimits(every) {
  const head = h(
    "header",
    { class: "agents-head task-budgets-head" },
    h("h2", {}, "Each task"),
    h("p", { class: "muted" }, "A task can set any limit for itself, or have none; the rest are every task's.")
  );
  if (!state.tasks.length) return h("section", { class: "task-budgets" }, head, h("p", { class: "muted" }, "No tasks yet."));
  return h(
    "section",
    { class: "task-budgets" },
    head,
    h("ul", { class: "task-budget-list" }, state.tasks.map((task) => taskLimitsRow(task, every)))
  );
}

function taskLimitsRow(task, every) {
  const own = task.own_limits || {};
  const used = task.time_used_secs ? `runs took ${durationText(task.time_used_secs)}` : "no runs yet";
  const fields = LIMIT_FIELDS.map((f) => {
    const set = Object.prototype.hasOwnProperty.call(own, f.key);
    const choice = !set ? "every" : own[f.key] === null ? "none" : "own";
    const input = h("input", {
      "data-key": `limit:${task.id}:${f.key}:value`,
      "aria-label": `${f.label}, ${f.unit}`,
      type: "number",
      min: 0,
      step: f.step,
      inputmode: "decimal",
      value: choice === "own" ? String(f.from(own[f.key])) : "",
      disabled: choice !== "own",
      onchange: () => setTaskLimits(task),
    });
    const select = h(
      "select",
      {
        "data-key": `limit:${task.id}:${f.key}`,
        "data-field": f.key,
        "aria-label": f.label,
        onchange: () => {
          input.disabled = select.value !== "own";
          if (select.value === "own") input.focus();
          else setTaskLimits(task);
        },
      },
      h("option", { value: "every", selected: choice === "every" }, `Every task's (${limitText(f, every[f.key])})`),
      h("option", { value: "none", selected: choice === "none" }, "No limit"),
      h("option", { value: "own", selected: choice === "own" }, "Its own")
    );
    select.input = input;
    return h(
      "label",
      { class: "limit-field", title: f.help },
      h("span", { class: "limit-label" }, f.label),
      h("span", { class: "limit-value" }, select, input, f.unit)
    );
  });
  const gpuChoice = own.gpu === undefined || own.gpu === null ? "every" : own.gpu ? "yes" : "no";
  const gpu = h(
    "select",
    { "data-key": `limit:${task.id}:gpu`, "data-field": "gpu", "aria-label": "GPU", onchange: () => setTaskLimits(task) },
    h("option", { value: "every", selected: gpuChoice === "every" }, `Every task's (${every.gpu === false ? "off" : "may be used"})`),
    h("option", { value: "yes", selected: gpuChoice === "yes" }, "May be used"),
    h("option", { value: "no", selected: gpuChoice === "no" }, "Off")
  );
  return h(
    "li",
    { class: `task-budget-row task-limits-row${colorOf(task) ? ` color-${colorOf(task)}` : ""}` },
    h(
      "details",
      { "data-task": task.id },
      h(
        "summary",
        { "data-key": `limits:${task.id}` },
        h("span", { class: "task-budget-name" }, taskName(task), h("span", { class: "muted" }, ` · ${task.id}`)),
        h("span", { class: "muted task-budget-spent" }, `${limitsSummary(task.limits || {})} · ${used}`)
      ),
      h("div", { class: "limits-grid" }, fields, h("label", { class: "limit-field" }, h("span", { class: "limit-label" }, "GPU"), h("span", { class: "limit-value" }, gpu)))
    )
  );
}

/** Seconds as "1 h 20 min". */
function durationText(secs) {
  const minutes = Math.round(secs / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h${minutes % 60 ? ` ${minutes % 60} min` : ""}`;
}

async function setTaskLimits(task) {
  const row = document.querySelector(`#agents-view [data-task="${CSS.escape(task.id)}"]`);
  if (!row) return;
  const limits = {};
  for (const select of row.querySelectorAll("select[data-field]")) {
    const key = select.dataset.field;
    if (key === "gpu") {
      if (select.value !== "every") limits.gpu = select.value === "yes";
      continue;
    }
    const f = LIMIT_FIELDS.find((x) => x.key === key);
    if (select.value === "none") limits[key] = null;
    else if (select.value === "own") {
      const text = select.input.value.trim();
      if (text === "") return; // Waits for a value.
      limits[key] = f.to(Number(text));
    }
  }
  try {
    const view = await call("set_task_limits", { task: task.id, limits });
    toast(`${taskName(task)}: ${limitsSummary(view.limits)}`);
  } catch (err) {
    fail(err);
  }
  await refresh();
  if (state.view === "limits") {
    focusLimits = task.id;
    await openLimits();
  }
}

/** Task id → what its run at work takes now. */
const liveMeasure = new Map();

/** What the run at work takes, against the task's limits. */
function renderMeasure(task) {
  const el = $("run-measure");
  const m = task.running ? liveMeasure.get(task.id) : null;
  // A redraw while the numbers run to this measure lets them run on.
  if (m && measureFrame && measureFor === task.id && shownMeasure.get(task.id) === m) return;
  cancelAnimationFrame(measureFrame);
  measureFrame = 0;
  el.hidden = !m;
  if (!m) {
    shownMeasure.delete(task.id);
    return;
  }
  const l = task.limits || {};
  const of = (key, used) => (l[key] != null ? used / l[key] : 0);
  const near = Math.max(of("memory_mb", m.memory / 1024 ** 2), of("disk_mb", (m.disk || 0) / 1024 ** 2));
  el.classList.toggle("ask", near >= 0.8);
  el.classList.toggle("slowed", l.cpu_percent != null && m.cpu_percent >= l.cpu_percent * 0.9);
  // The numbers run from what was shown to the new measure, so a trend
  // reads rather than a flicker.
  const from = shownMeasure.get(task.id);
  shownMeasure.set(task.id, m);
  if (!from || !moving()) {
    el.textContent = measureText(m, l);
    return;
  }
  const start = performance.now();
  measureFor = task.id;
  const step = (now) => {
    const k = Math.min(1, (now - start) / 500);
    const e = 1 - (1 - k) ** 3;
    const mix = (a, b) => (a == null || b == null ? b : a + (b - a) * e);
    el.textContent = measureText(
      { cpu_percent: Math.round(mix(from.cpu_percent, m.cpu_percent)), memory: mix(from.memory, m.memory), disk: mix(from.disk, m.disk), processes: m.processes },
      l
    );
    measureFrame = k < 1 ? requestAnimationFrame(step) : 0;
  };
  measureFrame = requestAnimationFrame(step);
}

/** The last measure shown for each task at work, to run on from. */
const shownMeasure = new Map();
let measureFrame = 0;
let measureFor = null;

function measureText(m, l) {
  const parts = [
    `CPU ${m.cpu_percent}%${l.cpu_percent != null ? ` / ${l.cpu_percent}%` : ""}`,
    `${bytesText(m.memory)}${l.memory_mb != null ? ` / ${limitText(LIMIT_FIELDS[1], l.memory_mb)}` : ""}`,
  ];
  if (m.disk != null) parts.push(`disk +${bytesText(m.disk)}${l.disk_mb != null ? ` / ${limitText(LIMIT_FIELDS[2], l.disk_mb)}` : ""}`);
  parts.push(plural(m.processes, "process", "processes"));
  return parts.join(" · ");
}

/** Tasks whose run was told to stop and has not ended yet. */
const stopping = new Set();

/** The Stop button: "Stopping…" and held while the agent ends its turn,
 * which can take up to ten seconds. */
function renderStop(task) {
  const held = task.running && stopping.has(task.id);
  $("stop").hidden = !task.running;
  $("stop").disabled = held;
  $("stop").textContent = held ? "Stopping…" : "Stop";
}

async function stopRun(task) {
  if (stopping.has(task.id)) return;
  stopping.add(task.id);
  renderStop(task);
  try {
    await call("stop_run", { task: task.id });
    toast(`Stopping the agent in ${taskName(task)}; it has ten seconds to end its turn`);
  } catch (err) {
    stopping.delete(task.id);
    renderStop(task);
    fail(err);
  }
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
  const box = $("questions");
  const asked = task ? questionsOf(task.id) : [];
  // An answered question folds away, on the task it was asked on; one
  // going already goes on.
  const going = [];
  if (task && task.id === questionsFor) {
    for (const el of box.querySelectorAll(".question")) {
      if (el.classList.contains("leaving") || !asked.some((q) => String(q.id) === el.dataset.q)) going.push(el);
    }
  }
  questionsFor = task?.id;
  const cards = asked.map((q) => {
    // Seen for the first time: it opens, and its edge pulses twice.
    if (!questionsSeen.has(q.id)) arrive(`question:${q.id}`, QUESTION_ARRIVES_MS);
    questionsSeen.add(q.id);
    return arriving(
        h(
          "div",
          { class: "question", "data-q": q.id },
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
              {
                class: "button ghost",
                type: "button",
                "data-key": `remember:${q.id}`,
                title: `Let ${q.host}:${q.port} through on this folder from now on, without asking`,
                onclick: (e) => inFlight(e.currentTarget, () => answer(q.id, true, true)),
              },
              "Approve and remember"
            ),
            h(
              "button",
              { class: "button ghost danger", type: "button", "data-key": `deny:${q.id}`, "aria-label": `Deny ${q.host}:${q.port}`, onclick: (e) => inFlight(e.currentTarget, () => answer(q.id, false)) },
              "Deny"
            )
          )
        ),
      `question:${q.id}`
    );
  });
  replace(box, ...cards);
  if (!moving()) return;
  // It opens to its own height, so the opening keeps one pace.
  for (const el of cards) {
    if (!el.classList.contains("arrived")) continue;
    el.classList.remove("arrived");
    el.style.setProperty("--open-h", `${el.offsetHeight}px`);
    el.classList.add("arrived");
  }
  for (const el of going) {
    if (!el.classList.contains("leaving")) {
      el.classList.remove("arrived");
      leave(el);
    }
    box.append(el);
  }
}

/** The questions shown at least once, and the task they were shown for. */
const questionsSeen = new Set();
let questionsFor;
/** It opens, then its edge pulses twice. */
const QUESTION_ARRIVES_MS = 2400;

async function answer(id, approve, remember) {
  try {
    await call("answer", { id, approve, remember: Boolean(remember) });
    if (remember) toast("Approved, and not asked again on this folder");
  } catch (err) {
    fail(err);
  }
}

async function onQuestions(waiting) {
  tellExtensions("questions", waiting, "questions.read");
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

// --- Markdown --------------------------------------------------------------

// An agent answers in Markdown. It is laid out here with DOM calls alone:
// the text comes from the agent, so nothing of it is ever parsed as HTML,
// and links are shown, not followed (the window has no business navigating).

/** Inline Markdown: code spans, bold, italics and links, as nodes. */
function inline(text) {
  const nodes = [];
  const pattern = /(`+)([\s\S]*?[^`])\1(?!`)|\*\*([^*]+)\*\*|(?<![\w])__([^_]+)__(?![\w])|\*([^*\s][^*]*)\*|(?<![\w])_([^_\s][^_]*)_(?![\w])|\[([^\]]+)\]\(([^)\s]+)\)/g;
  let at = 0;
  for (const m of text.matchAll(pattern)) {
    if (m.index > at) nodes.push(text.slice(at, m.index));
    if (m[2] !== undefined) nodes.push(h("code", {}, m[2].trim()));
    else if (m[3] !== undefined || m[4] !== undefined) nodes.push(h("strong", {}, ...inline(m[3] ?? m[4])));
    else if (m[5] !== undefined || m[6] !== undefined) nodes.push(h("em", {}, ...inline(m[5] ?? m[6])));
    else nodes.push(h("span", { class: "md-link", title: m[8] }, ...inline(m[7])), h("span", { class: "md-url" }, ` (${m[8]})`));
    at = m.index + m[0].length;
  }
  if (at < text.length) nodes.push(text.slice(at));
  return nodes;
}

/** A table row's cells: split on pipes outside code spans, `\|` kept. */
function cells(line) {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  const out = [];
  let cell = "";
  let code = false;
  for (let i = 0; i < row.length; i++) {
    const c = row[i];
    if (c === "\\" && row[i + 1] === "|") {
      cell += "|";
      i++;
    } else if (c === "`") {
      code = !code;
      cell += c;
    } else if (c === "|" && !code) {
      out.push(cell.trim());
      cell = "";
    } else {
      cell += c;
    }
  }
  out.push(cell.trim());
  return out;
}

const TABLE_RULE = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;
const LIST_ITEM = /^(\s*)([-*+]|\d{1,9}[.)])\s+(.*)$/;

/** Block Markdown: paragraphs, headings, lists, quotes, code and tables. */
/**
 * Blocks laid out from Markdown `text`. With `base` (the line before the
 * text's first, 0 for a whole file), each block and list item carries the
 * source lines it came from as data-line and data-line-end, counted from 1,
 * so a place picked in the Preview names its line in the file.
 */
function markdown(text, base) {
  const lines = text.split("\n");
  const blocks = [];
  const lined = (el, start, end = start) => {
    if (base !== undefined) {
      el.dataset.line = String(base + start + 1);
      el.dataset.lineEnd = String(base + end + 1);
    }
    return el;
  };
  const put = (el, start, end) => blocks.push(lined(el, start, end));
  let para = [];
  let paraStart = 0;
  const flush = () => {
    if (para.length) put(h("p", {}, ...inline(para.join("\n"))), paraStart, paraStart + para.length - 1);
    para = [];
  };
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const start = i;
    const fence = line.match(/^\s*(`{3,}|~{3,})\s*([\w+-]*)/);
    if (fence) {
      flush();
      const code = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith(fence[1])) code.push(lines[i++]);
      put(h("pre", {}, h("code", {}, ...highlighted(code.join("\n"), fence[2]))), start, Math.min(i, lines.length - 1));
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]) && lines[i + 1].includes("-")) {
      flush();
      const align = cells(lines[i + 1]).map((c) => (c.endsWith(":") ? (c.startsWith(":") ? "center" : "right") : "left"));
      const head = cells(line);
      const rows = [];
      i += 2;
      while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") rows.push(cells(lines[i++]));
      i--;
      // Classes, not style attributes: the window's CSP allows no inline style.
      const cell = (tag, text, n) => h(tag, { class: `align-${align[n] || "left"}` }, ...inline(text));
      put(
        h(
          "div",
          { class: "md-table" },
          h(
            "table",
            {},
            h("thead", {}, h("tr", {}, ...head.map((c, n) => cell("th", c, n)))),
            h("tbody", {}, ...rows.map((r) => h("tr", {}, ...head.map((_, n) => cell("td", r[n] || "", n)))))
          )
        ),
        start,
        i
      );
      continue;
    }
    const heading = line.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (heading) {
      flush();
      put(h(`h${Math.min(heading[1].length + 2, 6)}`, { class: "md-h", "data-level": heading[1].length }, ...inline(heading[2])), start);
      continue;
    }
    if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flush();
      put(h("hr", {}), start);
      continue;
    }
    const item = line.match(LIST_ITEM);
    if (item) {
      flush();
      const ordered = /\d/.test(item[2]);
      const items = [];
      while (i < lines.length) {
        const m = lines[i].match(LIST_ITEM);
        if (m && /\d/.test(m[2]) === ordered) {
          const depth = Math.min(Math.floor(m[1].replace(/\t/g, "  ").length / 2), 4);
          items.push(lined(h("li", { class: `depth-${depth}` }, ...inline(m[3])), i));
        } else if (lines[i].trim() !== "" && /^\s+/.test(lines[i]) && items.length) {
          // A continued item.
          const last = items[items.length - 1];
          last.append("\n", ...inline(lines[i].trim()));
          if (base !== undefined) last.dataset.lineEnd = String(base + i + 1);
        } else {
          break;
        }
        i++;
      }
      i--;
      put(h(ordered ? "ol" : "ul", {}, ...items), start, i);
      continue;
    }
    if (/^\s{0,3}>/.test(line)) {
      flush();
      const quoted = [];
      while (i < lines.length && /^\s{0,3}>/.test(lines[i])) quoted.push(lines[i++].replace(/^\s{0,3}>\s?/, ""));
      i--;
      put(h("blockquote", {}, ...markdown(quoted.join("\n"), base === undefined ? undefined : base + start)), start, i);
      continue;
    }
    if (line.trim() === "") flush();
    else {
      if (!para.length) paraStart = i;
      para.push(line);
    }
  }
  flush();
  return blocks;
}

// --- Live ------------------------------------------------------------------

function streamItem(item) {
  switch (item.kind) {
    case "prompt":
      return h(
        "div",
        { class: "msg prompt" },
        item.refs && item.refs.length ? h("div", { class: "prompt-refs" }, ...item.refs.map((r) => placeChip(r))) : null,
        item.text
      );
    case "message":
      return h("div", { class: "msg md" }, ...markdown(item.text));
    case "thought":
      return h("div", { class: "msg thought" }, item.text);
    case "tool": {
      const line = h(
        "div",
        { class: `event ${item.done ? (item.failed ? "failed" : "done") : ""}` },
        h("span", { class: "event-mark" }, item.done ? (item.failed ? "✗" : "✓") : "·"),
        h("span", {}, item.title),
        item.files && item.files.length
          ? h("span", { class: "event-files" }, `→ ${item.files.join(", ")}`)
          : null
      );
      if (!item.output) return line;
      // What the call showed: open while it runs or when it failed, closed
      // once it went well, unless the person opened or closed it.
      const open = item.expanded ?? (!item.done || item.failed);
      const output = h(
        "details",
        {
          class: "tool-output",
          open,
          ontoggle: (e) => {
            item.expanded = e.currentTarget.open;
          },
        },
        h("summary", { "data-key": `output:${item.id}` }, line),
        h("pre", {}, stripAnsi(item.output))
      );
      return output;
    }
    case "note":
      return h("div", { class: "event" }, h("span", { class: "event-mark" }, "·"), h("span", {}, item.text));
    case "finished":
      return h(
        "div",
        { class: `event finished ${item.error ? "error" : item.stopped ? "" : "done"}` },
        h("span", { class: "event-mark" }, item.error ? "!" : item.stopped ? "■" : checkMark()),
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
        "The agent works in the task's copy; nothing reaches your folder until you review and commit it. Each tool call it makes is a step you can look at."
      )
    );
  } else {
    // What came in while the person watched arrives; the rest stands.
    replace(stream, ...items.map((item) => arriving(streamItem(item), item)));
  }
  if (stick) stream.scrollTop = stream.scrollHeight;
}

/** A tick that draws itself as a run ends well. */
function checkMark() {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 14 14");
  svg.setAttribute("class", "check-mark");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(ns, "path");
  path.setAttribute("d", "M2.5 7.4 L5.7 10.4 L11.5 3.8");
  path.setAttribute("pathLength", "1");
  svg.append(path);
  return svg;
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
    case "note":
      items.push({ kind: "note", text: event.text });
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
    case "tool_output": {
      const call = items.findLast((i) => i.kind === "tool" && i.id === event.id);
      if (call) call.output = event.text;
      break;
    }
    case "permission":
      items.push({ kind: "note", text: `Permission “${event.title}”: ${event.chosen}${event.granted === false ? " (not granted)" : ""}` });
      break;
    case "plan":
      items.push({ kind: "note", text: `Plan: ${event.entries.join(" / ")}` });
      break;
    default:
      break;
  }
}

/** A command's output without terminal escapes (colours, cursor moves);
 * a carriage return keeps what was written after it, as a terminal does. */
function stripAnsi(text) {
  return text
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "")
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
    .replace(/\x1b[@-_]/g, "")
    .split("\n")
    .map((line) => line.slice(line.lastIndexOf("\r", line.length - 2) + 1).replace(/\r$/, ""))
    .join("\n");
}

/** Folds one run event into the task's stream. */
function fold(task, event) {
  if (event.type === "prompt") {
    streamOf(task).push({ kind: "prompt", text: event.text, refs: event.refs || [] });
  } else if (event.type === "agent") {
    absorb(task, event.event);
  } else if (event.type === "preparing") {
    streamOf(task).push({ kind: "note", text: event.text });
  } else if (event.type === "resources") {
    liveMeasure.set(task, event.measured);
  } else if (event.type === "finished") {
    const refused = event.denials.length ? ` · the sandbox refused ${plural(event.denials.length, "operation")}` : "";
    // Stopped by a person or a limit: said as it is, not as a failure.
    const stopped = event.error && event.error.startsWith("stopped: ");
    streamOf(task).push({
      kind: "finished",
      // A person's own stop is no failure; a limit's is.
      error: Boolean(event.error) && event.error !== "stopped: you stopped the run",
      stopped,
      text: stopped
        ? `Stopped: ${event.error.slice("stopped: ".length)}. Review what it changed before it reaches the folder${refused}.`
        : event.error
          ? `The run failed: ${event.error}`
          : `Done. Review the changes before they reach the folder${refused}.`,
    });
  }
}

async function onRun({ task, seq, event }) {
  tellExtensions("run", { task, seq, event }, "events.read");
  // What the run at work has cost so far; in the task's spent once it ends.
  if (event.type === "prompt" || event.type === "finished") {
    liveCost.delete(task);
    liveMeasure.delete(task);
    stopping.delete(task);
  }
  // A limit stopped a run out of sight: said where it is seen.
  if (event.type === "finished" && event.error?.startsWith("stopped: ") && task !== state.current && !event.error.includes("you stopped")) {
    const t = state.tasks.find((x) => x.id === task);
    toast(`${t ? taskName(t) : task}: ${event.error}`, "error");
  }
  if (event.type === "agent" && event.event.type === "usage" && event.event.cost) {
    liveCost.set(task, event.event.cost);
    const current = currentTask();
    if (current && current.id === task) renderSpent(current);
  }
  if (state.loaded.has(task)) {
    // Already in the loaded stream: the daemon sent it before the load.
    if (seq > (state.lastSeq.get(task) || 0)) {
      const items = streamOf(task);
      const had = items.length;
      fold(task, event);
      // Folded as it happened, not loaded: it arrives in the stream.
      if (task === state.current) for (const item of items.slice(had)) arrive(item, item.kind === "finished" ? 800 : SETTLE_MS);
      state.lastSeq.set(task, seq);
    }
  } else if (state.loading.has(task)) {
    // Held until the load returns, then folded if it was not in it.
    if (!state.buffered.has(task)) state.buffered.set(task, []);
    state.buffered.get(task).push({ seq, event });
  }
  if (event.type === "prompt" || event.type === "finished") await refresh();
  if (event.type === "resources") {
    const current = currentTask();
    if (current && current.id === task) renderMeasure(current);
    return;
  }
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
  // The places picked in the Preview go with it, and leave the prompt with it.
  const refs = (promptRefs.get(task.id) || []).map((c) => c.ref);
  try {
    await call("start_run", { task: task.id, agent: $("agent").value, prompt, refs });
    $("prompt").value = "";
    promptRefs.delete(task.id);
    renderPromptRefs(task);
    fitPrompt();
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

/** Files the daemon previews; it reads the bytes to decide in the end. */
const PICTURE = /\.(png|jpe?g|gif|webp)$/i;

/** Opens or closes the picture's before and after under its line. */
async function togglePreview(button, path) {
  const line = button.closest("li");
  const open = line.nextElementSibling;
  if (open && open.classList.contains("preview")) {
    open.remove();
    button.textContent = "Show";
    button.setAttribute("aria-expanded", "false");
    return;
  }
  let preview;
  try {
    preview = await call("preview", { task: state.current, path });
  } catch (err) {
    fail(err);
    return;
  }
  const side = (label, p) =>
    h(
      "figure",
      { class: "picture" },
      p.side === "image"
        ? h("img", { src: `data:${p.mime};base64,${p.data}`, alt: `${path}, ${label}` })
        : h("div", { class: "picture-none" }, p.side === "absent" ? "none" : p.reason),
      h("figcaption", {}, p.side === "image" ? `${label} · ${p.width}×${p.height}` : label)
    );
  line.after(h("li", { class: "preview" }, side("before", preview.before), side("after", preview.after)));
  button.textContent = "Hide";
  button.setAttribute("aria-expanded", "true");
}

/** The section open in each tabbed container, by the section's name. */
const openSection = new Map();

/**
 * Turns a container's sections, each opened by an h2.group-title (or the
 * ignored files' details), into tabs side by side: the section's name and
 * how many lines it holds, one section showing at a time. What comes
 * before the first heading stays above the tabs.
 */
function tabify(container) {
  const focused = container.contains(document.activeElement) ? document.activeElement : null;
  const before = [];
  const sections = [];
  for (const node of [...container.childNodes]) {
    const details = node.matches && node.matches("details.ignored-group");
    if (details || (node.matches && node.matches("h2.group-title"))) {
      const head = details ? node.querySelector("summary") : node;
      const name = head.dataset.tab || head.textContent.split(" · ")[0].trim();
      // In its own tab the ignored files show without a click.
      if (details) node.open = true;
      sections.push({ name, body: details ? [node] : [] });
    } else if (sections.length) sections[sections.length - 1].body.push(node);
    else before.push(node);
  }
  if (sections.length < 2) return;
  // Its lines: files, commits and steps.
  const count = (body) =>
    body.reduce((n, node) => {
      if (!node.matches) return n;
      if (node.matches("div.entry")) return n + 1;
      if (node.matches("ul.files")) return n + node.children.length;
      return n + node.querySelectorAll("ul.files > li").length;
    }, 0);
  const want = openSection.get(container.id);
  let active = sections.find((sec) => sec.name === want) || sections[0];
  const panes = sections.map((sec) => h("div", { class: "section-pane", role: "tabpanel", "aria-label": sec.name }, sec.body));
  const tabs = sections.map((sec, i) => {
    const n = count(sec.body);
    return h(
      "button",
      {
        class: "section-tab",
        type: "button",
        role: "tab",
        "data-key": `sec:${container.id}:${sec.name}`,
        onclick: () => show(i),
      },
      sec.name,
      n ? h("span", { class: "section-count" }, String(n)) : null
    );
  });
  const show = (i) => {
    active = sections[i];
    openSection.set(container.id, active.name);
    tabs.forEach((t, j) => {
      t.setAttribute("aria-selected", String(j === i));
      t.tabIndex = j === i ? 0 : -1;
    });
    panes.forEach((p, j) => (p.hidden = j !== i));
  };
  const bar = h("div", { class: "section-tabs", role: "tablist", "aria-label": "Sections" }, tabs);
  bar.addEventListener("keydown", (e) => {
    const at = tabs.indexOf(document.activeElement);
    const to = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (at === -1 || to === undefined) return;
    e.preventDefault();
    const i = (to + tabs.length) % tabs.length;
    show(i);
    tabs[i].focus();
  });
  container.replaceChildren(...before, bar, ...panes);
  show(sections.indexOf(active));
  if (focused && focused.isConnected && document.activeElement !== focused) {
    // Moved into a pane: the keyboard stays where it was, on its section.
    const pane = focused.closest(".section-pane");
    if (pane) show(panes.indexOf(pane));
    focused.focus();
  } else if (focused && !focused.isConnected && focused.dataset && focused.dataset.key) {
    container.querySelector(`[data-key="${CSS.escape(focused.dataset.key)}"]`)?.focus();
  }
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
            why ? h("span", { class: "entity-note" }, why) : null,
            e.kind === "file" && PICTURE.test(e.path)
              ? h(
                  "button",
                  {
                    class: "show-picture",
                    type: "button",
                    "data-key": `preview:${e.path}`,
                    "aria-expanded": "false",
                    "aria-label": `Show ${e.path}`,
                    onclick: (ev) => togglePreview(ev.currentTarget, e.path),
                  },
                  "Show"
                )
              : null
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
  for (const path of git.staged || []) lines.push(`${path} staged unlike the file: the next git commit records what you did not review`);
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
              h(
                "li",
                {},
                `${j.subject} · ${VERDICT[j.verdict].chip} `,
                h("span", { class: "rules" }, `(${j.rules.join(", ")})`),
                rememberable(j) ? rememberBox(id, task, j) : null
              )
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
    ...extensionNotesGroup(id),
    ...ignoredGroup(id, ignored, include, held),
    ...homeGroup(id, review.home || [], busy, review.home_applied)
  );
  tabify($("effects"));
  extensionsSeeReview(id, review.digest);
}

/** Per task, home paths whose diff the person opened (true) or closed (false). */
const homeShown = new Map();

/** What a home change does to the person's file, line by line. */
function homeDiff(id, path) {
  const box = h("pre", { class: "diff" }, "Loading…");
  call("home_diff", { task: id, path })
    .then((d) => {
      if (d.diff === "skipped") {
        box.textContent = d.reason === "binary" ? "Not text, so not shown." : "Too large to show.";
        return;
      }
      const sign = { same: " ", added: "+", removed: "−" };
      const rows = [];
      for (const hunk of d.hunks) {
        rows.push(h("span", { class: "diff-hunk" }, `line ${hunk.before}`));
        for (const line of hunk.lines) {
          rows.push(h("span", { class: `diff-${line.kind}` }, `${sign[line.kind]} ${line.text}`));
        }
      }
      if (d.cut) rows.push(h("span", { class: "diff-hunk" }, "cut here: the change is longer"));
      if (rows.length === 0) rows.push(h("span", { class: "diff-hunk" }, "same text; only its details changed"));
      box.replaceChildren(...rows);
    })
    .catch((err) => {
      box.textContent = shown(String(err));
    });
  return box;
}

/**
 * What the task's commands wrote in the home directory, caught in the
 * task's home layer. Never part of a commit: each is applied to the real
 * home, or thrown away, on its own.
 */
function homeGroup(id, changes, busy, applied) {
  const act = async (verb, args) => {
    const done = await call(verb, { task: id, ...args });
    if (verb === "apply_home" && done.kept) toast(`Applied; what it replaced is kept in ${done.kept}`);
    if (verb === "undo_home") toast(`Undid ${plural(done.paths.length, "change")} in ~; they wait for review again`);
    await refresh();
    await loadReview(id);
  };
  const undo = applied
    ? h(
        "div",
        { class: "home-applied" },
        h("span", { class: "muted" }, `Last applied: ${applied.paths.map((p) => `~/${p}`).join(", ")}`),
        Object.assign(
          armed("Undo", "Click again to put back what was there", busy, () => act("undo_home", {}).catch(fail)),
          { title: "Puts back what your home held there, if you have not changed it since" }
        )
      )
    : null;
  if (changes.length === 0) return undo ? [h("h2", { class: "group-title", "data-tab": "In your home" }, "In your home (~)"), undo] : [];
  const mark = { created: "+", modified: "~", deleted: "−" };
  if (!homeShown.has(id)) homeShown.set(id, new Map());
  const shown = homeShown.get(id);
  const all = changes.map((c) => c.path);
  return [
    h("h2", { class: "group-title", "data-tab": "In your home" }, `In your home (~) · ${plural(changes.length, "change")}, caught for you`),
    h(
      "p",
      { class: "muted home-note" },
      "Written in the task's layer, not in your home. Apply takes one there, after checking your file is still as it was; what it replaces is kept."
    ),
    h(
      "ul",
      { class: "files" },
      changes.flatMap((c) => {
        const apply = armed("Apply", "Click again to write it in ~", busy, () => act("apply_home", { paths: [c.path] }).catch(fail));
        apply.dataset.key = `home-apply:${c.path}`;
        const file = c.kind === "file";
        // A file that runs as shells start is open from the first look.
        const open = file && (shown.has(c.path) ? shown.get(c.path) : c.startup);
        const toggle = file
          ? h(
              "button",
              {
                class: "button ghost",
                type: "button",
                "data-key": `home-diff:${c.path}`,
                "aria-expanded": String(open),
                onclick: () => {
                  shown.set(c.path, !open);
                  loadReview(id);
                },
              },
              open ? "Hide" : "Show"
            )
          : null;
        return [
          h(
            "li",
            { class: "home-change" },
            h("span", { class: `change ${c.change}` }, mark[c.change] || "~"),
            h("span", {}, `~/${c.path}${c.kind === "dir" ? "/" : ""}`),
            // Where a link leads, before it is applied.
            c.target != null ? h("span", { class: "muted", title: "A link: what reads it reads what it points to" }, `→ ${c.target}`) : null,
            c.startup
              ? h("span", { class: "chip ask", title: "Shells or tools read and run this file as they start: what it says runs every time." }, "runs at every start")
              : null,
            h(
              "span",
              { class: "home-actions" },
              toggle,
              apply,
              h("button", { class: "button ghost", type: "button", "data-key": `home-discard:${c.path}`, disabled: busy, onclick: (e) => inFlight(e.currentTarget, () => act("discard_home", { paths: [c.path] }).catch(fail)) }, "Discard")
            )
          ),
          open ? h("li", { class: "home-diff" }, homeDiff(id, c.path)) : null,
        ];
      })
    ),
    changes.length > 1
      ? h(
          "div",
          { class: "review-actions" },
          h("button", { class: "button ghost", type: "button", "data-key": "home-discard-all", disabled: busy, onclick: (e) => inFlight(e.currentTarget, () => act("discard_home", { paths: all }).catch(fail)) }, "Discard all")
        )
      : null,
    undo,
  ];
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
/** " (its name)" for a task the person named; tasks on one folder share
 * the folder's. */
function otherLabel(id) {
  const label = state.tasks.find((t) => t.id === id)?.label;
  return label ? ` (${label})` : "";
}

function meetsGroup(task) {
  const overlaps = (task && task.overlaps) || [];
  if (overlaps.length === 0) return [];
  return [
    h("h2", { class: "group-title", "data-tab": "Other tasks" }, "Other tasks change these too"),
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
            h("span", { class: "entity-note" }, ` · task ${o.task}${otherLabel(o.task)}`)
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
      h("summary", { class: "group-title", "data-tab": "Ignored" }, `Ignored by the folder's .gitignore · ${ignored.length} ${include ? "included" : "stay in the copy"}`),
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

// --- Remembered approvals --------------------------------------------------

/** Rules that ask about a file by name: their question can be answered for good. */
const REMEMBERABLE = ["manifests-and-lockfiles", "project-files", "automation"];

const rememberable = (j) =>
  j.verdict === "ask" && j.rules.length > 0 && j.rules.every((r) => REMEMBERABLE.includes(r)) && !j.subject.startsWith(".kule/");

/** "Don't ask again here" for one held file, with what it would have let through. */
function rememberBox(id, task, j) {
  const picks = state.remember.get(id) || new Set();
  const note = h("span", { class: "remember-note" });
  const box = h("input", {
    type: "checkbox",
    checked: picks.has(j.subject),
    "data-key": `remember:${j.subject}`,
    onchange: async (e) => {
      const set = state.remember.get(id) || new Set();
      if (e.currentTarget.checked) set.add(j.subject);
      else set.delete(j.subject);
      state.remember.set(id, set);
      note.textContent = "";
      if (!e.currentTarget.checked || !task) return;
      try {
        const impact = await call("rule_impact", {
          allowance: { kind: "file", path: j.subject, rule: j.rules[0] },
          workspace: task.workspace,
        });
        note.textContent = ` · would have passed ${plural(impact.changes, "earlier change")} on its own`;
      } catch (err) {
        fail(err);
      }
    },
  });
  return h("label", { class: "check remember" }, box, "Don't ask again in this folder", note);
}

async function openRules() {
  state.view = "rules";
  renderSidebar();
  renderTask();
  await renderRules();
}

async function renderRules() {
  if (state.view !== "rules") return;
  let rules = [];
  try {
    rules = await call("rules");
  } catch (err) {
    fail(err);
  }
  const describe = (r) => (r.kind === "connect" ? `Connections to ${r.host}:${r.port}` : `Changes to ${r.path} (${r.rule})`);
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Rules"),
      h(
        "p",
        { class: "muted" },
        "What you told Kule not to ask about again. A denied change is never here, nor anything under .kule/, where a repository keeps its own rules."
      )
    ),
    rules.length === 0
      ? h("p", { class: "muted" }, "Nothing remembered: every question is asked.")
      : h(
          "ul",
          { class: "rule-list" },
          ...rules.map((r) => {
            const forget = armed("Forget", "Click again to be asked again", false, async () => {
              await call("forget_rule", { id: r.id }).catch(fail);
              await renderRules();
            });
            forget.dataset.key = `forget:${r.id}`;
            return h(
              "li",
              { class: "agent-card" },
              h("div", {}, h("strong", {}, describe(r))),
              h("div", { class: "muted" }, r.workspace ? `In ${r.workspace}` : "In every folder", ` · since ${when(r.added_unix)}`),
              forget
            );
          })
        )
  );
}

// --- Secrets ---------------------------------------------------------------

// API keys a task's commands use without seeing them. The window sends a
// value once, to keep, and never gets one back: the list has names and
// hosts only.

async function openSecrets() {
  state.view = "secrets";
  renderSidebar();
  renderTask();
  await renderSecrets();
}

async function renderSecrets() {
  if (state.view !== "secrets") return;
  let kept = [];
  let keyHome = null;
  try {
    [kept, keyHome] = await Promise.all([call("secrets"), call("secrets_key")]);
  } catch (err) {
    fail(err);
  }
  // Where the key that seals the values is: said plainly when it is a file.
  const sealed =
    keyHome === "system"
      ? h("p", { class: "muted" }, "The values are sealed with a key your system's keychain keeps.")
      : keyHome === "file"
        ? h(
            "p",
            { class: "secrets-key-file" },
            "The values are sealed with a key kept in a file in Kule's home: this computer offers no keychain, or Kule's home is not ~/.kule. Anyone who can read your files can open them."
          )
        : null;
  const folders = [...new Set(state.tasks.map((t) => t.workspace))].sort();
  const input = (id, label, hint, attrs) =>
    h(
      "label",
      { class: "agent-field", for: id },
      h("span", { class: "agent-field-label" }, label),
      h("input", { id, autocomplete: "off", spellcheck: "false", ...attrs }),
      h("span", { class: "agent-field-hint" }, hint)
    );
  const scope = h(
    "select",
    { id: "secret-scope" },
    h("option", { value: "" }, "Every folder"),
    ...folders.map((f) => h("option", { value: f }, f))
  );
  const add = async () => {
    const name = $("secret-name").value.trim();
    const hosts = $("secret-hosts").value.split(/[\s,]+/).filter(Boolean);
    const value = $("secret-value").value;
    const workspace = $("secret-scope").value || null;
    await call("add_secret", { name, value, hosts, workspace });
    // Not kept in the window a moment longer than needed.
    $("secret-value").value = "";
    toast(`${name} kept: commands see a placeholder`);
    await renderSecrets();
  };
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Secrets"),
      h(
        "p",
        { class: "muted" },
        "API keys a task's commands use without seeing them. A command finds a placeholder in the variable of the secret's name; the task's proxy puts the value into requests to the secret's hosts, over HTTPS, and nowhere else. Values in a folder's .env files that look like secrets are kept out of the copy the same way."
      ),
      sealed
    ),
    kept.length === 0
      ? h("p", { class: "muted" }, "No secrets kept.")
      : h(
          "ul",
          { class: "rule-list" },
          ...kept.map((s) => {
            const remove = armed("Forget", "Click again to forget it", false, async () => {
              await call("remove_secret", { name: s.name, workspace: s.workspace || null }).catch(fail);
              await renderSecrets();
            });
            remove.dataset.key = `forget-secret:${s.name}:${s.workspace || ""}`;
            return h(
              "li",
              { class: "agent-card" },
              h("div", {}, h("strong", {}, s.name), " → ", s.hosts.join(", ")),
              h("div", { class: "muted" }, s.workspace ? `In ${s.workspace}` : "In every folder", ` · since ${when(s.added_unix)}`),
              remove
            );
          })
        ),
    h(
      "div",
      { class: "agent-form" },
      input("secret-name", "Name", "The variable commands find it in, like STRIPE_KEY.", { placeholder: "STRIPE_KEY" }),
      input("secret-hosts", "Goes to", "Hosts, separated by spaces or commas.", { placeholder: "api.stripe.com" }),
      input("secret-value", "Value", "Kept in Kule's home, readable by you alone; never shown again.", { type: "password" }),
      h("label", { class: "agent-field", for: "secret-scope" }, h("span", { class: "agent-field-label" }, "For"), scope, h("span", { class: "agent-field-hint" }, "One folder, or every folder."))
    ),
    h(
      "div",
      { class: "agent-actions" },
      h("button", { class: "button primary", type: "button", "data-key": "secret-add", onclick: (e) => inFlight(e.currentTarget, () => add().catch(fail)) }, "Keep secret")
    )
  );
}

// --- Agents ----------------------------------------------------------------

// Kule's own agent profiles and the person's. A profile opens holes in the
// sandbox, so the view shows exactly what each one opens, and the person
// approves that: the daemon approves only the profile they looked at.

const agentsView = {
  listed: [],
  /** The ACP registry once asked for: { entries, fresh }. */
  registry: null,
  registryOpen: false,
  filter: "",
  /** The profile being written, and whether it is a new one. */
  editing: null,
  isNew: false,
  /** Where the agent came from, for a download the person installs. */
  note: null,
  trial: null,
  trying: false,
};

/** The agents the prompt can pick: the ones that can run. */
async function loadAgentChoices() {
  try {
    state.agents = await call("agents");
  } catch (err) {
    fail(err);
  }
  const picked = $("agent").value;
  replace($("agent"), ...state.agents.map((name) => h("option", { value: name, selected: name === picked }, name)));
}

async function openAgents() {
  state.view = "agents";
  renderSidebar();
  renderTask();
  await loadAgentList();
}

async function loadAgentList() {
  keepForm();
  try {
    agentsView.listed = await call("agent_list");
  } catch (err) {
    fail(err);
  }
  renderAgents();
}

/** After a change to the profiles: the list, and what the prompt offers. */
async function agentsChanged() {
  await loadAgentList();
  await loadAgentChoices();
}

const lines = (text) =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

/** What a profile opens, one row each, as the person approves it. */
function opens(profile, ownHome) {
  const row = (label, items) =>
    items && items.length ? [h("dt", {}, label), h("dd", {}, items.join(", "))] : [];
  const env = Object.entries(profile.env || {}).map(([k, v]) => `${k}=${v}`);
  return h(
    "dl",
    { class: "agent-opens" },
    ...row("Runs", [profile.command.join(" ")]),
    ...row("Writes in ~", profile.state),
    ...row("Its login", profile.login),
    ...row("Read-only", profile.protected),
    ...row("Reveals (macOS)", profile.reveal_on_macos),
    ...row("Sees your env", profile.pass_env),
    ...row("Keys it never sees", Object.entries(profile.keys || {}).map(([k, hosts]) => `${k} → ${hosts.join(", ")}`)),
    ...row("Sets env", env),
    ...row("Also connects to", (profile.hosts || []).map((host) => `https://${host}`)),
    ...row("Session mode", profile.mode ? [profile.mode] : []),
    ...row("Home of its own", profile.own_home ? [`$${profile.own_home} → ${ownHome || "Kule's folder for it"}`] : [])
  );
}

function agentCard(listed) {
  const { profile, origin, held, digest, warnings, own_home_dir } = listed;
  const yours = origin === "person";
  // An extension's profile is approved like the person's own, but edited
  // and removed with its extension.
  const theirs = origin === "extension";
  const actions = [];
  if (yours) {
    actions.push(
      h("button", { class: "button ghost", type: "button", "data-key": `edit:${profile.name}`, onclick: () => editAgent(profile, false) }, "Edit")
    );
  }
  if (yours || theirs) {
    if (held) {
      const approve = armed(
        "Approve",
        "Click again to let it open what is listed",
        false,
        async () => {
          try {
            await call("approve_agent", { name: profile.name, digest });
            toast(`${profile.name} can run now`);
          } catch (err) {
            fail(err);
          }
          await agentsChanged();
        }
      );
      approve.classList.remove("danger");
      approve.dataset.key = `approve:${profile.name}`;
      actions.push(approve);
    } else {
      actions.push(
        h(
          "button",
          {
            class: "button ghost",
            type: "button",
            "data-key": `revoke:${profile.name}`,
            onclick: (e) =>
              inFlight(e.currentTarget, async () => {
                await call("revoke_agent", { name: profile.name }).catch(fail);
                await agentsChanged();
              }),
          },
          "Revoke"
        )
      );
    }
  }
  if (yours) {
    const remove = armed("Remove", `Click again to remove ${profile.name}`, false, async () => {
      await call("remove_agent", { name: profile.name }).catch(fail);
      await agentsChanged();
    });
    remove.dataset.key = `remove:${profile.name}`;
    actions.push(remove);
  }
  return h(
    "article",
    { class: "agent-card" },
    h(
      "header",
      { class: "agent-card-head" },
      h("h3", {}, profile.name),
      h("span", { class: "chip" }, yours ? "yours" : theirs ? `from ${listed.extension}` : "built in"),
      listed.missing
        ? h("span", { class: "chip deny" }, "not installed")
        : h("span", { class: `chip ${held ? "ask" : "allow"}` }, held ? "held" : "can run")
    ),
    held ? h("p", { class: "agent-held" }, held) : null,
    listed.missing ? h("p", { class: "agent-held" }, `It ${installHint(listed.missing)}.`) : null,
    opens(profile, own_home_dir),
    (warnings || []).length ? h("ul", { class: "agent-warnings" }, ...warnings.map((w) => h("li", {}, w))) : null,
    actions.length ? h("div", { class: "agent-actions" }, ...actions) : null
  );
}

function editAgent(profile, isNew, note) {
  agentsView.editing = JSON.parse(JSON.stringify(profile));
  agentsView.isNew = isNew;
  agentsView.note = note || null;
  agentsView.trial = null;
  agentsView.registryOpen = false;
  renderAgents();
  $("agent-form-name")?.focus();
}

const BLANK = { name: "", command: [], state: [], login: [], protected: [], reveal_on_macos: [], env: {}, pass_env: [], keys: {}, hosts: [], mode: null, own_home: null };

/** The profile the form holds now. */
function formProfile() {
  const value = (id) => $(id).value;
  const env = {};
  for (const line of lines(value("agent-form-env"))) {
    const at = line.indexOf("=");
    if (at > 0) env[line.slice(0, at).trim()] = line.slice(at + 1).trim();
  }
  // NAME host host, one key per line.
  const keys = {};
  for (const line of lines(value("agent-form-keys"))) {
    const [name, ...hosts] = line.split(/[\s,]+/).filter(Boolean);
    if (name) keys[name] = hosts;
  }
  return {
    name: value("agent-form-name").trim(),
    command: lines(value("agent-form-command")),
    state: lines(value("agent-form-state")),
    login: lines(value("agent-form-login")),
    protected: lines(value("agent-form-protected")),
    reveal_on_macos: agentsView.editing.reveal_on_macos || [],
    mode: value("agent-form-mode").trim() || null,
    env,
    pass_env: lines(value("agent-form-pass-env")),
    keys,
    hosts: lines(value("agent-form-hosts")),
    own_home: value("agent-form-own-home").trim() || null,
  };
}

function field(id, label, hint, value, rows) {
  const input = rows
    ? h("textarea", { id, rows, spellcheck: "false" }, value)
    : h("input", { id, type: "text", value, spellcheck: "false" });
  return h("label", { class: "agent-field", for: id }, h("span", { class: "agent-field-label" }, label), input, hint ? h("span", { class: "agent-field-hint" }, hint) : null);
}

function agentEditor() {
  const p = agentsView.editing;
  const env = Object.entries(p.env || {}).map(([k, v]) => `${k}=${v}`).join("\n");
  const keys = Object.entries(p.keys || {}).map(([k, hosts]) => `${k} ${hosts.join(" ")}`).join("\n");
  const trial = agentsView.trial;
  return h(
    "section",
    { class: "agent-editor", "aria-label": "Agent profile" },
    h("h2", {}, agentsView.isNew ? "New agent" : `Edit ${p.name}`),
    agentsView.note ? h("p", { class: "agent-held" }, agentsView.note) : null,
    h(
      "div",
      { class: "agent-form" },
      field("agent-form-name", "Name", "Lowercase letters, digits and dashes.", p.name),
      field("agent-form-command", "Command", "One argument per line; a package fetched by npx or uvx names one exact version.", p.command.join("\n"), 3),
      field("agent-form-state", "Writes in ~", "Its own state, one path per line under your home directory.", p.state.join("\n"), 3),
      field("agent-form-login", "Its login", "In that state; hidden from every other agent.", p.login.join("\n"), 2),
      field("agent-form-protected", "Read-only", "Its settings that run code outside the sandbox later (hooks, MCP servers).", p.protected.join("\n"), 2),
      field("agent-form-pass-env", "Sees your env", "Your variables it may read.", p.pass_env.join("\n"), 2),
      field("agent-form-keys", "Keys it never sees", "NAME host host, one per line: your variable with its API key, and the hosts it is for. The agent gets a placeholder; the task's proxy puts the key into requests to those hosts alone.", keys, 2),
      field("agent-form-env", "Sets env", "NAME=value, one per line.", env, 2),
      field("agent-form-hosts", "Also connects to", "HTTPS hosts beyond what the policy lets through.", p.hosts.join("\n"), 3),
      field("agent-form-mode", "Session mode", "Optional: the ACP mode to start in.", p.mode || ""),
      field(
        "agent-form-own-home",
        "Home of its own",
        "Optional: the variable the agent reads its config directory from (CODEX_HOME, CLAUDE_CONFIG_DIR, or HOME). Kule points it at a folder of the agent's own, so what it writes in Kule never reaches it outside. Sign it in there once.",
        p.own_home || ""
      )
    ),
    h(
      "div",
      { class: "agent-actions" },
      h("button", { class: "button primary", type: "button", "data-key": "agent-save", onclick: (e) => inFlight(e.currentTarget, saveAgent) }, "Save"),
      h(
        "button",
        { class: "button", type: "button", "data-key": "agent-try", disabled: agentsView.trying, onclick: tryAgent },
        agentsView.trying ? "Trying… (up to two minutes)" : "Try it"
      ),
      h("button", { class: "button ghost", type: "button", "data-key": "agent-cancel", onclick: () => { agentsView.editing = null; renderAgents(); } }, "Cancel")
    ),
    h("p", { class: "agent-field-hint" }, "Try it runs the agent once in an empty folder, opened no wider than above, and suggests what it was refused. Saved, it runs only once you approve it."),
    trial ? trialResult(trial) : null
  );
}

function trialResult(trial) {
  const picks = [
    ...trial.state.map((path) => ({ kind: "state", value: path, label: `Writes in ~: ${path}` })),
    ...trial.hosts.map((host) => ({ kind: "hosts", value: host, label: `Connects to: https://${host}` })),
    ...(trial.protect || []).map((path) => ({ kind: "protected", value: path, label: `Read-only: ${path} (it runs code or gives instructions later)` })),
  ];
  const boxes = picks.map((pick) => {
    const box = h("input", { type: "checkbox", checked: true });
    box.pick = pick;
    return h("label", { class: "check" }, box, pick.label);
  });
  const add = () => {
    const profile = formProfile();
    for (const label of boxes) {
      const box = label.querySelector("input");
      if (box.checked && !profile[box.pick.kind].includes(box.pick.value)) profile[box.pick.kind].push(box.pick.value);
    }
    agentsView.editing = profile;
    agentsView.trial = null;
    renderAgents();
  };
  return h(
    "div",
    { class: "agent-trial" },
    h("p", { class: trial.error ? "agent-held" : "" }, trial.error ? `It did not answer: ${trial.error}` : `It answered: ${trial.reply}`),
    picks.length
      ? [h("p", {}, "It was refused these; add the ones it needs:"), ...boxes, h("button", { class: "button", type: "button", "data-key": "agent-add-picks", onclick: add }, "Add selected")]
      : h("p", { class: "muted" }, "Nothing to suggest."),
    trial.refused.length ? h("details", {}, h("summary", {}, `Everything refused · ${trial.refused.length}`), h("pre", {}, trial.refused.join("\n"))) : null,
    trial.log.length ? h("details", {}, h("summary", {}, "Its error output"), h("pre", {}, trial.log.join("\n"))) : null
  );
}

async function saveAgent() {
  const profile = formProfile();
  try {
    await call("save_agent", { profile });
    toast(`Saved ${profile.name}: approve it to let it run`);
    agentsView.editing = null;
  } catch (err) {
    fail(err);
  }
  await agentsChanged();
}

async function tryAgent() {
  const profile = formProfile();
  agentsView.editing = profile;
  agentsView.trying = true;
  agentsView.trial = null;
  renderAgents();
  try {
    agentsView.trial = await call("try_agent", { profile });
  } catch (err) {
    fail(err);
  }
  agentsView.trying = false;
  renderAgents();
}

/** What the person typed in the form, kept across a redraw. */
function keepForm() {
  if (agentsView.editing && $("agent-form-name")) agentsView.editing = formProfile();
}

async function toggleRegistry() {
  keepForm();
  agentsView.registryOpen = !agentsView.registryOpen;
  renderAgents();
  if (agentsView.registryOpen && !agentsView.registry) {
    try {
      agentsView.registry = await call("agent_registry");
    } catch (err) {
      fail(err);
      agentsView.registryOpen = false;
    }
    renderAgents();
  }
}

/** A registry entry as a profile to start from, opening nothing yet. */
function fromRegistry(entry) {
  const install = entry.install;
  if (install.kind === "package") {
    editAgent({ ...BLANK, name: entry.id, command: install.command, env: install.env }, true);
  } else if (install.kind === "download") {
    const program = install.cmd.split("/").pop();
    editAgent(
      { ...BLANK, name: entry.id, command: [program, ...install.args] },
      true,
      `${entry.name} is a download: get ${install.archive} (sha256 ${install.sha256}) and put ${program} on your PATH first.`
    );
  }
}

function registryPanel() {
  const registry = agentsView.registry;
  if (!registry) return h("section", { class: "agent-registry" }, h("p", { class: "muted" }, "Reading the ACP registry…"));
  const filter = h("input", {
    type: "search",
    placeholder: "Filter",
    "aria-label": "Filter the registry",
    value: agentsView.filter,
    "data-key": "registry-filter",
    // Only the list is drawn again, so typing keeps its place.
    oninput: (e) => {
      agentsView.filter = e.currentTarget.value;
      replace($("registry-list"), ...registryItems());
    },
  });
  return h(
    "section",
    { class: "agent-registry", "aria-label": "ACP registry" },
    h("h2", {}, "ACP registry"),
    registry.fresh ? null : h("p", { class: "muted" }, "The registry could not be reached; this is the list as last read."),
    filter,
    h("ul", { class: "registry-list", id: "registry-list" }, ...registryItems())
  );
}

function registryItems() {
  const words = agentsView.filter.toLowerCase();
  return agentsView.registry.entries
    .filter((e) => `${e.name} ${e.id} ${e.description}`.toLowerCase().includes(words))
    .map((entry) => {
        const how =
          entry.install.kind === "package" ? entry.install.command.join(" ") : entry.install.kind === "download" ? "download" : "not for this system";
        return h(
          "li",
          {},
          h("div", { class: "registry-name" }, h("strong", {}, entry.name), ` ${entry.version}`),
          h("div", { class: "muted" }, entry.description),
          h("div", { class: "agent-field-hint" }, how, entry.website ? ` · ${entry.website}` : ""),
          entry.install.kind === "unavailable"
            ? null
            : h("button", { class: "button", type: "button", "data-key": `registry:${entry.id}`, onclick: () => fromRegistry(entry) }, "Use")
        );
      });
}

function renderAgents() {
  if (state.view !== "agents") return;
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Agents"),
      h(
        "p",
        { class: "muted" },
        "Kule's own agents, and yours from ~/.kule/agents.toml. Each of yours opens only what is listed on it, and runs only once you approve that; a change to it needs approval again."
      ),
      h(
        "div",
        { class: "agent-actions" },
        h("button", { class: "button", type: "button", "data-key": "registry-toggle", "aria-expanded": String(agentsView.registryOpen), onclick: toggleRegistry }, "Add from the ACP registry"),
        h("button", { class: "button ghost", type: "button", "data-key": "agent-new", onclick: () => editAgent(BLANK, true) }, "Add by hand")
      )
    ),
    agentsView.editing ? agentEditor() : null,
    agentsView.registryOpen ? registryPanel() : null,
    ...agentsView.listed.map(agentCard)
  );
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
    const remember = approve ? [...(state.remember.get(id) || [])] : [];
    const result = await call("commit", { task: id, approve, includeIgnored, reviewed, remember });
    if (result.outcome === "applied") state.remember.delete(id);
    if (result.outcome === "applied") state.includeIgnored.delete(id);
    if (result.outcome === "applied") {
      const merged = (result.merged || []).length ? ` · merged with the folder's changes in ${result.merged.join(", ")}` : "";
      toast(`Committed ${plural(result.changes, "change")} · checkpoint ${result.checkpoint}${merged}`);
    }
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

  entries.push(h("h2", { class: "group-title" }, "History"));
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
  // Going back to a step is a step itself, so it needs no second click.
  const backTo = (n) => {
    const blocker = (timeline.blockers || {})[n];
    const why = task && task.running ? "The agent is at work" : blocker ? `${blocker} was not kept then (too large, or not a plain file)` : null;
    return h(
      "button",
      {
        class: "button ghost step-back",
        type: "button",
        disabled: Boolean(why),
        title: why || (n === 0 ? "Put the copy back as the folder holds it, keeping the steps" : `Put the copy back as it was after step ${n}; this is a step too, so it can be gone back on`),
        "data-key": `back:${n}`,
        onclick: () => goBack(id, n),
      },
      "Go back here"
    );
  };
  const forkAt = (n) =>
    h(
      "button",
      {
        class: "button ghost step-back",
        type: "button",
        disabled: Boolean((timeline.blockers || {})[n]) || (task && task.running),
        title: n === 0 ? "A new task on the same folder, from the copy as the folder holds it" : `A new task on the same folder, from the copy as it was after step ${n}`,
        "data-key": `fork:${n}`,
        onclick: () => forkTask(id, n),
      },
      "Fork from here"
    );
  if (timeline.steps.length) {
    entries.push(
      h(
        "div",
        { class: "entry step" },
        h("span", { class: "entry-dot" }),
        h("span", { class: "entry-title" }, "Before the first step"),
        h("span", { class: "entry-actions" }, backTo(0), forkAt(0)),
        h("div", { class: "entry-body" }, h("div", {}, "the copy as the folder holds it"))
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
        h("span", { class: "entry-actions" }, h("span", { class: "chip" }, "uncommitted"), backTo(step.id), forkAt(step.id)),
        h(
          "div",
          { class: "entry-body" },
          step.changes.length ? step.changes.map((c) => h("div", {}, changeLine(c))) : h("div", {}, "no file changes")
        )
      )
    );
  }
  if (entries.length === 1) {
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
              d.saved ? ` · what was there is in ${d.saved}` : null,
              d.hint ? h("div", { class: "muted" }, d.hint) : null
            )
          )
      )
    );
  }
  replace($("timeline"), ...entries);
  tabify($("timeline"));
}

/** A new task from this one's copy, now (`step` null) or at a step, and
 * the window on it. */
async function forkTask(id, step) {
  try {
    const fork = await call("fork_task", { task: id, step });
    await refresh();
    select(fork.id);
    toast(`${fork.label} is a new task on the same folder`);
  } catch (err) {
    fail(err);
  }
}

async function goBack(id, step) {
  try {
    const back = await call("back_to_step", { task: id, step });
    toast(back.label);
    await refresh();
    loadTimeline(id);
  } catch (err) {
    fail(err);
  }
}

function summarizeConnections(log) {
  const seen = new Map();
  for (const c of log) {
    const secrets = (c.secrets || []).join(", ");
    const key = `${c.host}:${c.port} ${c.verdict} ${c.outcome} ${c.answer || ""} ${secrets}`;
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
    const secrets = (c.secrets || []).length ? ` · with ${c.secrets.join(", ")}` : "";
    return `${c.host}:${c.port}${n > 1 ? ` ×${n}` : ""} · ${what}${secrets}`;
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
    const [tasks, groups, policy] = await Promise.all([call("tasks"), call("groups"), call("policy")]);
    // A refresh that started later has already drawn a newer list.
    if (token !== refreshToken) return;
    state.tasks = tasks;
    state.groups = groups;
    state.policy = policy;
  } catch (err) {
    fail(err);
  }
  if (state.current && !currentTask()) state.current = null;
  if (!state.current && state.tasks.length) state.current = state.tasks[0].id;
  renderSidebar();
  renderTask();
  loadDaily();
  if (!$("empty").hidden) loadWelcome();
}

function select(id) {
  if (id !== state.current) {
    disarmDelete();
    clearPanels();
  }
  state.view = "tasks";
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
    layout.regions[regionOf("live")].active = "live";
    await refresh();
    $("prompt").focus();
  } catch (err) {
    fail(err);
  }
}

async function openTerminal(task = currentTask()) {
  if (!task) return;
  try {
    await call("open_terminal", { task: task.id });
  } catch (err) {
    fail(err);
  }
}

/** The task the delete shortcut was pressed for once, and when that ends. */
let deleteArmed = null;

function disarmDelete() {
  clearTimeout(deleteArmed?.timer);
  deleteArmed = null;
}

/** The delete shortcut: the first press says what a second one does, the
 * second, within four seconds, deletes. */
function deleteByKey(task) {
  if (task.running) {
    toast("The task cannot be deleted while its agent works");
    return;
  }
  if (deleteArmed?.id === task.id) {
    disarmDelete();
    removeTask(task);
    return;
  }
  disarmDelete();
  deleteArmed = { id: task.id, timer: setTimeout(disarmDelete, 4000) };
  toast(`Press again to delete ${taskName(task)} (${task.id}); the folder is not touched`);
}

async function removeTask(task = currentTask()) {
  if (!task) return;
  try {
    await call("remove_task", { task: task.id });
    forgetTerminal(task.id);
    state.streams.delete(task.id);
    state.loaded.delete(task.id);
    if (state.current === task.id) {
      state.current = null;
      clearPanels();
    }
    toast(`Deleted task ${task.id}; the folder was not touched`);
  } catch (err) {
    fail(err);
  }
  await refresh();
}

// --- Layout ----------------------------------------------------------------

// The window is four regions, left, main, right and bottom, and each holds
// panels as tabs, one showing at a time. A panel moves to another region by
// dragging its tab, from its tab's menu or from the keyboard; the regions
// open, close and resize, and the daemon keeps it all in layout.json.

const PANELS = {
  tasks: { title: "Tasks", needsTask: false },
  live: { title: "Agent", needsTask: true },
  review: { title: "Review", needsTask: true },
  timeline: { title: "Timeline", needsTask: true },
  files: { title: "Files", needsTask: true },
  preview: { title: "Preview", needsTask: true },
  terminal: { title: "Terminal", needsTask: true },
};

const REGION_NAMES = { left: "the left side", main: "the main area", right: "the right side", bottom: "the bottom panel" };

const SIZE_LIMITS = { left: [180, 640], right: [220, 760], bottom: [120, 900] };

/** The layout's format; one kept in an older one gives way to Kule's own. */
const LAYOUT_VERSION = 2;

function defaultLayout() {
  return {
    version: LAYOUT_VERSION,
    regions: {
      left: { panels: ["tasks", "files"], active: "tasks", open: true, size: 272 },
      main: { panels: ["review", "timeline", "preview"], active: "review", open: true, size: 0 },
      right: { panels: [], active: null, open: false, size: 380 },
      // The agent's chat and the terminals share the bottom, side by side.
      bottom: { panels: ["live", "terminal"], active: "live", open: true, size: 420 },
    },
  };
}

let layout = defaultLayout();

/** A layout as kept, made whole: unknown panels out, a panel this version
 * added but the kept layout lacks put where it goes by default. */
function normalLayout(kept) {
  const base = defaultLayout();
  if (!kept || !kept.regions || kept.version !== LAYOUT_VERSION) return base;
  const seen = new Set();
  const regions = {};
  for (const name of Object.keys(base.regions)) {
    const r = kept.regions[name] || base.regions[name];
    const panels = (r.panels || []).filter((p) => PANELS[p] && !seen.has(p) && seen.add(p));
    regions[name] = {
      panels,
      active: panels.includes(r.active) ? r.active : panels[0] || null,
      open: name === "main" ? r.open !== false : Boolean(r.open),
      size: Number.isFinite(r.size) ? r.size : base.regions[name].size,
    };
  }
  for (const [name, r] of Object.entries(base.regions)) {
    for (const p of r.panels) {
      if (!seen.has(p)) {
        regions[name].panels.push(p);
        regions[name].active ||= p;
        seen.add(p);
      }
    }
  }
  // An extension's panel the layout has not placed yet goes to the right.
  for (const p of Object.keys(PANELS)) {
    if (!seen.has(p)) {
      regions.right.panels.push(p);
      regions.right.active ||= p;
      seen.add(p);
    }
  }
  return { version: LAYOUT_VERSION, regions };
}

const regionOf = (panel) => Object.keys(layout.regions).find((n) => layout.regions[n].panels.includes(panel));

/** Whether `panel` shows now: the one in front in an open region, and the
 * main area only while a task is open there. */
function isShown(panel) {
  const name = regionOf(panel);
  const r = name && layout.regions[name];
  if (!r || !r.open || r.active !== panel) return false;
  if (name === "main" && (SETTINGS.includes(state.view) || !currentTask())) return false;
  return !PANELS[panel].needsTask || Boolean(currentTask());
}

let layoutSave = null;

function keepLayout() {
  clearTimeout(layoutSave);
  layoutSave = setTimeout(() => call("set_layout", { layout }).catch(fail), 300);
}

async function loadLayout() {
  try {
    layout = normalLayout(await call("layout"));
  } catch (err) {
    fail(err);
  }
  renderTask();
}

function panelTab(name, panel) {
  const r = layout.regions[name];
  const command = commandById(`view.${panel}`);
  const combo = command ? bindingOf(command) : "";
  const label = [PANELS[panel].title];
  if (panel === "review") label.push(h("span", { class: "tab-count", id: "review-count", hidden: true }));
  return h(
    "button",
    {
      class: "tab",
      role: "tab",
      type: "button",
      "data-panel": panel,
      "data-key": `tab:${panel}`,
      "aria-selected": String(r.active === panel),
      "aria-controls": `panel-${panel}`,
      tabindex: r.active === panel ? 0 : -1,
      title: combo ? `${PANELS[panel].title} (${formatCombo(combo)}) · drag to move it` : `${PANELS[panel].title} · drag to move it`,
      onclick: () => {
        if (panelDrag.moved) return;
        activatePanel(panel);
      },
      oncontextmenu: (e) => {
        e.preventDefault();
        panelMenu(panel, e);
      },
      onpointerdown: (e) => panelDragStart(e, panel),
    },
    ...label
  );
}

/** The terminal panel's tabs in its region's bar: one per shell of the open
 * task, each closable, then "+" for another. */
function terminalTabs(name) {
  const r = layout.regions[name];
  const task = currentTask();
  const tabs = (task && terminals.get(task.id)) || { list: [], active: null };
  const command = commandById("terminal.new");
  const combo = command ? bindingOf(command) : "";
  const shown = r.active === "terminal";
  const items = tabs.list.map((e) => {
    const selected = shown && e.key === tabs.active;
    return h(
      "span",
      { class: `tab terminal-tab${selected ? " active" : ""}${e.ended ? " ended" : ""}` },
      h(
        "button",
        {
          type: "button",
          role: "tab",
          class: "terminal-tab-main",
          "data-panel": "terminal",
          "data-key": `term:${e.key}`,
          "aria-selected": String(selected),
          "aria-controls": "panel-terminal",
          tabindex: selected ? 0 : -1,
          title: "Your shell in the task's copy, outside the sandbox · drag to move the terminals",
          onclick: () => {
            if (panelDrag.moved) return;
            focusTerminal(task.id, e.key);
          },
          oncontextmenu: (ev) => {
            ev.preventDefault();
            panelMenu("terminal", ev);
          },
          onpointerdown: (ev) => panelDragStart(ev, "terminal"),
        },
        h("span", { class: "terminal-dot", "aria-hidden": "true" }),
        `${e.shell} ${e.n}`
      ),
      h("button", { type: "button", class: "terminal-close", "aria-label": `Close terminal ${e.n}`, title: "Close", onclick: () => closeTerminal(task.id, e.key) }, "×")
    );
  });
  items.push(
    h(
      "button",
      {
        type: "button",
        class: "tab-plus",
        "aria-label": "New terminal",
        title: combo ? `New terminal (${formatCombo(combo)})` : "New terminal",
        disabled: !task,
        onclick: () => newTerminal(),
      },
      "+"
    )
  );
  return items;
}

/** Puts every panel in its region, shows the one in front, sizes the
 * regions and says which are open. */
function renderLayout() {
  const workbench = $("workbench");
  const side = (n) => (layout.regions[n].open ? layout.regions[n].size : 0);
  workbench.style.gridTemplateColumns = `${side("left")}px ${layout.regions.left.open ? 5 : 0}px minmax(0, 1fr) ${layout.regions.right.open ? 5 : 0}px ${side("right")}px`;
  const task = currentTask();
  // The main area gives its height to the bottom panel while it is hidden;
  // Settings and the empty window still have it.
  const mainHeld = layout.regions.main.open || SETTINGS.includes(state.view) || !task;
  workbench.querySelector(".center").style.gridTemplateRows = mainHeld
    ? `minmax(0, 1fr) ${layout.regions.bottom.open ? 5 : 0}px ${side("bottom")}px`
    : "auto 0px minmax(0, 1fr)";
  $("main-area").classList.toggle("collapsed", !mainHeld);
  for (const [name, r] of Object.entries(layout.regions)) {
    const el = $(`region-${name}`);
    const hideMain = name === "main" && (SETTINGS.includes(state.view) || !task);
    el.hidden = !r.open || hideMain;
    const split = document.querySelector(`[data-split="${name}"]`);
    if (split) split.hidden = !r.open;
    replace(el.querySelector(".region-tabs"), ...r.panels.flatMap((p) => (p === "terminal" ? terminalTabs(name) : [panelTab(name, p)])));
    replace(el.querySelector(".region-actions"), ...regionActions(name));
    const body = el.querySelector(".region-body");
    for (const p of r.panels) {
      const node = $(`panel-${p}`);
      if (node.parentElement !== body) body.append(node);
      node.hidden = p !== r.active || (PANELS[p].needsTask && !task);
      node.setAttribute("aria-labelledby", "");
    }
    let note = body.querySelector(".region-note");
    const message = !r.panels.length
      ? "Drag a panel's tab here, or move one with its menu."
      : PANELS[r.active] && PANELS[r.active].needsTask && !task
        ? `Choose a task to see its ${PANELS[r.active].title.toLowerCase()}.`
        : null;
    if (message) {
      if (!note) body.append((note = h("p", { class: "region-note" })));
      note.textContent = message;
    } else if (note) note.remove();
  }
  for (const n of ["left", "main", "bottom", "right"]) {
    $(`toggle-${n}`).setAttribute("aria-pressed", String(layout.regions[n].open));
  }
}

/** The buttons at the end of a region's tab bar: hide it, and for the
 * bottom panel, fill the window with it or give the space back. */
function regionActions(name) {
  const button = (label, glyph, key, action) =>
    h("button", { class: "region-action", type: "button", "aria-label": label, title: label, "data-key": key, onclick: action }, glyph);
  const words = REGION_NAMES[name];
  const actions = [];
  if (name === "bottom") {
    const filling = !layout.regions.main.open;
    actions.push(
      button(filling ? "Give the main area its space back" : "Fill the window with the bottom panel", filling ? "⤡" : "⤢", "bottom-fill", () => toggleRegion("main"))
    );
  }
  if (name === "main") {
    actions.push(button("Fill the window with the main area", "⤢", "main-fill", () => closeRegion("bottom")));
  }
  actions.push(button(`Hide ${words}`, { left: "‹", right: "›", bottom: "⌄", main: "⌃" }[name], `hide-${name}`, () => closeRegion(name)));
  return actions;
}

function activatePanel(panel) {
  const name = regionOf(panel);
  if (!name) return;
  const r = layout.regions[name];
  r.active = panel;
  r.open = true;
  if (name === "main" || PANELS[panel].needsTask) state.view = "tasks";
  keepLayout();
  renderSidebar();
  renderTask();
}

/** Shows `panel` and puts the keyboard on its tab. */
function revealPanel(panel) {
  activatePanel(panel);
  document.querySelector(`.region-tabs [data-panel="${panel}"]`)?.focus();
}

/** Moves `panel` into region `to`, before the tab at `at` (the end without). */
function movePanel(panel, to, at) {
  const from = regionOf(panel);
  if (!from) return;
  const source = layout.regions[from];
  const target = layout.regions[to];
  source.panels = source.panels.filter((p) => p !== panel);
  if (source.active === panel) source.active = source.panels[0] || null;
  if (!source.panels.length && from !== "main") source.open = false;
  const index = at === undefined ? target.panels.length : Math.min(at, target.panels.length);
  target.panels.splice(index, 0, panel);
  target.active = panel;
  target.open = true;
  keepLayout();
  renderTask();
  toast(`${PANELS[panel].title} moved to ${REGION_NAMES[to]}`);
}

function toggleRegion(name) {
  if (layout.regions[name].open) closeRegion(name);
  else openRegion(name);
}

function openRegion(name) {
  const r = layout.regions[name];
  r.open = true;
  if (!r.panels.length) toast(`${REGION_NAMES[name][0].toUpperCase()}${REGION_NAMES[name].slice(1)} is empty; drag a panel's tab here`);
  keepLayout();
  renderTask();
}

/** Hides a region. The main area and the bottom panel are never hidden
 * together: one of them holds the window's middle. */
function closeRegion(name) {
  const { main, bottom } = layout.regions;
  if (name === "main") {
    if (!bottom.panels.length) {
      toast("The bottom panel is empty, so the main area stays");
      return;
    }
    main.open = false;
    bottom.open = true;
  } else {
    layout.regions[name].open = false;
    if (name === "bottom") main.open = true;
  }
  keepLayout();
  renderTask();
}

/** Shows `panel`, or hides its region when it is the one showing there. */
function togglePanel(panel) {
  const name = regionOf(panel);
  if (!name) return;
  const r = layout.regions[name];
  if (r.open && r.active === panel && (name !== "main" || isShown(panel))) closeRegion(name);
  else revealPanel(panel);
}

async function resetLayout() {
  layout = defaultLayout();
  try {
    await call("set_layout", { layout: null });
  } catch (err) {
    fail(err);
  }
  renderTask();
  toast("The layout is back to Kule's own");
}

/** The panel the keyboard is in, else the main area's. */
function panelInView() {
  const el = document.activeElement && document.activeElement.closest && document.activeElement.closest("[data-panel]");
  return (el && el.dataset.panel) || layout.regions.main.active || "live";
}

function panelMenu(panel, e) {
  closeMenu();
  const here = regionOf(panel);
  const items = Object.keys(REGION_NAMES)
    .filter((n) => n !== here)
    .map((n) =>
      h("button", { class: "menu-item", type: "button", role: "menuitem", onclick: () => (closeMenu(), movePanel(panel, n)) }, `Move to ${REGION_NAMES[n]}`)
    );
  const menu = h("div", { class: "menu", id: "task-menu", role: "menu", "aria-label": `${PANELS[panel].title} panel` }, items);
  menu.items = items;
  menu.opener = e.currentTarget;
  document.body.append(menu);
  placeMenu(menu, e);
  items[0].focus();
  menu.addEventListener("keydown", (k) => {
    const here2 = items.indexOf(document.activeElement);
    if (k.key === "Escape") {
      k.preventDefault();
      closeMenu(true);
    } else if (k.key === "ArrowDown" || k.key === "ArrowUp") {
      k.preventDefault();
      items[(here2 + (k.key === "ArrowDown" ? 1 : -1) + items.length) % items.length].focus();
    } else if (k.key === "Tab") closeMenu(true);
  });
}

// --- Dragging a panel by its tab.

const panelDrag = { panel: null, startX: 0, startY: 0, moved: false, pointer: null, target: null, ghost: null };

function panelDragStart(e, panel) {
  if (e.button !== 0 || e.pointerType === "touch") return;
  Object.assign(panelDrag, { panel, startX: e.clientX, startY: e.clientY, moved: false, pointer: e.pointerId, target: null });
}

/** Where a drop at (x, y) puts the panel: a tab bar it is over, else the
 * part of the window it is in. */
function dropTarget(x, y) {
  for (const name of Object.keys(layout.regions)) {
    const el = $(`region-${name}`);
    if (el.hidden) continue;
    const bar = el.querySelector(".region-tabs").getBoundingClientRect();
    if (x >= bar.left && x <= bar.right && y >= bar.top && y <= bar.bottom) {
      const tabs = [...el.querySelectorAll(".region-tabs .tab")].filter((t) => t.dataset.panel !== panelDrag.panel);
      const at = tabs.findIndex((t) => x < t.getBoundingClientRect().left + t.offsetWidth / 2);
      return { region: name, at: at === -1 ? tabs.length : at };
    }
  }
  const box = $("workbench").getBoundingClientRect();
  const fx = (x - box.left) / box.width;
  const fy = (y - box.top) / box.height;
  if (fx < 0.2) return { region: "left" };
  if (fx > 0.8) return { region: "right" };
  if (fy > 0.72) return { region: "bottom" };
  return { region: "main" };
}

function panelDragMove(e) {
  if (!panelDrag.panel || e.pointerId !== panelDrag.pointer) return;
  if (!panelDrag.moved) {
    if (Math.hypot(e.clientX - panelDrag.startX, e.clientY - panelDrag.startY) < 6) return;
    panelDrag.moved = true;
    closeMenu();
    document.body.classList.add("dragging-panel");
    panelDrag.ghost = h("div", { class: "panel-ghost" }, PANELS[panelDrag.panel].title);
    panelDrag.zone = h("div", { class: "drop-zone" });
    document.body.append(panelDrag.ghost, panelDrag.zone);
  }
  panelDrag.ghost.style.transform = `translate(${e.clientX + 12}px, ${e.clientY + 8}px)`;
  const target = dropTarget(e.clientX, e.clientY);
  panelDrag.target = target;
  // Shows where it lands: the region's box, or the strip of the window a
  // closed region would open in.
  const box = $("workbench").getBoundingClientRect();
  const el = $(`region-${target.region}`);
  let r = el.hidden ? null : el.getBoundingClientRect();
  if (!r) {
    const w = box.width * 0.22;
    const hgt = box.height * 0.3;
    r = {
      left: { left: box.left, top: box.top, width: w, height: box.height },
      right: { left: box.right - w, top: box.top, width: w, height: box.height },
      bottom: { left: box.left, top: box.bottom - hgt, width: box.width, height: hgt },
      main: { left: box.left, top: box.top, width: box.width, height: box.height },
    }[target.region];
  }
  Object.assign(panelDrag.zone.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
  panelDrag.zone.textContent = `Move to ${REGION_NAMES[target.region]}`;
}

function panelDragEnd(e) {
  if (!panelDrag.panel || e.pointerId !== panelDrag.pointer) return;
  const { panel, moved, target } = panelDrag;
  panelDrag.panel = null;
  if (!moved) return;
  document.body.classList.remove("dragging-panel");
  panelDrag.ghost?.remove();
  panelDrag.zone?.remove();
  // The click the drag ends with does not also choose the tab.
  setTimeout(() => (panelDrag.moved = false), 0);
  if (e.type === "pointercancel" || !target) return;
  const from = regionOf(panel);
  if (from === target.region && target.at === undefined) return;
  movePanel(panel, target.region, target.at);
}

// --- Resizing the regions.

const splitDrag = { name: null, start: 0, size: 0 };

function splitStart(e) {
  const name = e.currentTarget.dataset.split;
  if (e.button !== 0) return;
  e.preventDefault();
  Object.assign(splitDrag, { name, start: name === "bottom" ? e.clientY : e.clientX, size: layout.regions[name].size, pointer: e.pointerId });
  e.currentTarget.setPointerCapture(e.pointerId);
  document.body.classList.add(name === "bottom" ? "resizing-v" : "resizing-h");
}

function resize(name, size) {
  const [min, max] = SIZE_LIMITS[name];
  layout.regions[name].size = Math.round(Math.max(min, Math.min(max, size)));
  renderLayout();
}

function splitMove(e) {
  if (!splitDrag.name || e.pointerId !== splitDrag.pointer) return;
  const at = splitDrag.name === "bottom" ? e.clientY : e.clientX;
  // The left side grows to the right; the right side and the bottom grow
  // toward the middle.
  const delta = splitDrag.name === "left" ? at - splitDrag.start : splitDrag.start - at;
  resize(splitDrag.name, splitDrag.size + delta);
}

function splitEnd(e) {
  if (!splitDrag.name || e.pointerId !== splitDrag.pointer) return;
  splitDrag.name = null;
  document.body.classList.remove("resizing-v", "resizing-h");
  keepLayout();
}

/** Arrow keys on a split move it 16 pixels at a time. */
function splitKey(e) {
  const name = e.currentTarget.dataset.split;
  const grow = { left: "ArrowRight", right: "ArrowLeft", bottom: "ArrowUp" }[name];
  const shrink = { left: "ArrowLeft", right: "ArrowRight", bottom: "ArrowDown" }[name];
  if (e.key !== grow && e.key !== shrink) return;
  e.preventDefault();
  resize(name, layout.regions[name].size + (e.key === grow ? 16 : -16));
  keepLayout();
}

// --- Files: the task's copy, a directory at a time.

const files = {
  task: null,
  /** Directories open in the tree, by path ("" is the top). */
  open: new Set([""]),
  listings: new Map(),
  /** Path → created, modified or deleted, from the task's review. */
  changes: new Map(),
  selected: null,
  token: 0,
};

const joinPath = (dir, name) => (dir ? `${dir}/${name}` : name);

async function loadFiles(id) {
  if (files.task !== id) {
    Object.assign(files, { task: id, open: new Set([""]), listings: new Map(), changes: new Map(), selected: null });
  }
  const token = ++files.token;
  try {
    const review = await call("review", { task: id, includeIgnored: false });
    files.changes = new Map(review.effects.effects.filter((e) => !inGit(e.path)).map((e) => [e.path, e.change]));
    for (const dir of files.open) files.listings.set(dir, await call("list_dir", { task: id, path: dir }));
  } catch (err) {
    fail(err);
    return;
  }
  if (token !== files.token || id !== state.current) return;
  renderFiles();
}

/** What changed at or under `path`, for a directory's mark. */
function changedUnder(path) {
  for (const p of files.changes.keys()) if (p === path || p.startsWith(`${path}/`)) return true;
  return false;
}

function fileRows(dir, depth) {
  const listing = files.listings.get(dir);
  if (!listing) return [];
  const names = new Set(listing.entries.map((e) => e.name));
  // What the task deleted is not in the copy; it shows where it was.
  const deleted = [...files.changes]
    .filter(([p, c]) => c === "deleted" && p.slice(0, p.lastIndexOf("/") === -1 ? 0 : p.lastIndexOf("/")) === dir)
    .map(([p]) => p.slice(p.lastIndexOf("/") + 1))
    .filter((n) => !names.has(n))
    .map((name) => ({ name, kind: "file", gone: true }));
  const rows = [];
  for (const entry of [...listing.entries, ...deleted]) {
    const path = joinPath(dir, entry.name);
    const change = entry.gone ? "deleted" : files.changes.get(path);
    const isDir = entry.kind === "dir";
    const open = isDir && files.open.has(path);
    rows.push(
      h(
        "button",
        {
          class: `file-row${files.selected === path ? " selected" : ""}${change ? ` ${change}` : ""}${entry.gone ? " gone" : ""}`,
          type: "button",
          role: "treeitem",
          "data-key": `file:${path}`,
          "data-path": path,
          "data-dir": isDir ? "1" : undefined,
          "aria-expanded": isDir ? String(open) : undefined,
          "aria-selected": String(files.selected === path),
          "aria-level": depth + 1,
          style: `padding-left:${8 + depth * 14}px`,
          onclick: () => (isDir ? toggleDir(path) : entry.kind === "link" ? toast(`${path} is a link; Files does not follow links`) : openFile(path, change)),
        },
        h("span", { class: "file-icon", "aria-hidden": "true" }, isDir ? (open ? "▾" : "▸") : entry.kind === "link" ? "↪" : ""),
        fileGlyph(entry.name, isDir, open),
        h("span", { class: "file-name" }, entry.name),
        change
          ? h("span", { class: "file-mark", title: change }, CHANGE_MARK[change])
          : isDir && changedUnder(path)
            ? h("span", { class: "file-mark dot", title: "changes inside" }, "•")
            : null
      )
    );
    if (open) rows.push(...fileRows(path, depth + 1));
  }
  if (listing.cut) rows.push(h("p", { class: "muted file-cut", style: `padding-left:${8 + depth * 14}px` }, "More files than are shown"));
  return rows;
}

function renderFiles() {
  const tree = $("files-tree");
  const rows = fileRows("", 0);
  replace(tree, ...(rows.length ? rows : [h("p", { class: "muted region-note" }, "The copy is empty.")]));
}

async function toggleDir(path) {
  if (files.open.has(path)) files.open.delete(path);
  else {
    files.open.add(path);
    if (!files.listings.has(path)) {
      try {
        files.listings.set(path, await call("list_dir", { task: files.task, path }));
      } catch (err) {
        files.open.delete(path);
        fail(err);
      }
    }
  }
  renderFiles();
}

/** A file picked in the tree opens in the Preview. */
function openFile(path, change) {
  files.selected = path;
  renderFiles();
  previewFile(path, change);
}

/** Arrow keys in the tree move between rows; right opens a directory,
 * left closes it. */
function filesKey(e) {
  const rows = [...$("files-tree").querySelectorAll(".file-row")];
  const at = rows.indexOf(document.activeElement);
  if (at === -1) return;
  const row = rows[at];
  const move = (to) => {
    e.preventDefault();
    rows[Math.max(0, Math.min(rows.length - 1, to))].focus();
  };
  if (e.key === "ArrowDown") move(at + 1);
  else if (e.key === "ArrowUp") move(at - 1);
  else if (e.key === "Home") move(0);
  else if (e.key === "End") move(rows.length - 1);
  else if (e.key === "ArrowRight" && row.dataset.dir && row.getAttribute("aria-expanded") === "false") {
    e.preventDefault();
    toggleDir(row.dataset.path);
  } else if (e.key === "ArrowLeft" && row.dataset.dir && row.getAttribute("aria-expanded") === "true") {
    e.preventDefault();
    toggleDir(row.dataset.path);
  }
}

// --- Terminal --------------------------------------------------------------

// The person's shells in the open task's copy, drawn by xterm.js, as many
// as they open, each a tab. They run outside the sandbox, as Open terminal
// does, and last while the window is open.

/** Task id → { list: [terminal], active: key }; a terminal is
 * { key, n, term, fit, search, el, id (the shell's), shell, ended }. */
const terminals = new Map();
let terminalKeys = 0;

/** The sixteen ANSI colours for a dark or a light ground: readable on it,
 * with "bright black" (what shells use for hints and suggestions) clearly
 * fainter than the text but not lost. */
const ANSI = {
  dark: {
    black: "#3b4048", red: "#f87171", green: "#4ade80", yellow: "#fbbf24", blue: "#60a5fa", magenta: "#c084fc", cyan: "#2dd4bf", white: "#d4d8de",
    brightBlack: "#7d8696", brightRed: "#fca5a5", brightGreen: "#86efac", brightYellow: "#fde68a", brightBlue: "#93c5fd", brightMagenta: "#d8b4fe", brightCyan: "#5eead4", brightWhite: "#ffffff",
  },
  light: {
    black: "#1b1f27", red: "#b91c1c", green: "#15803d", yellow: "#a16207", blue: "#1d4ed8", magenta: "#9333ea", cyan: "#0e7490", white: "#6b7280",
    brightBlack: "#8a93a3", brightRed: "#dc2626", brightGreen: "#16a34a", brightYellow: "#ca8a04", brightBlue: "#2563eb", brightMagenta: "#a855f7", brightCyan: "#0891b2", brightWhite: "#374151",
  },
};

/** xterm's colours from the theme in use. */
function terminalTheme() {
  const css = getComputedStyle(document.documentElement);
  const v = (name) => css.getPropertyValue(`--${name}`).trim();
  const scheme = document.documentElement.dataset.scheme || (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  return {
    ...ANSI[scheme],
    background: v("sunken"),
    foreground: v("text"),
    cursor: v("accent"),
    cursorAccent: v("sunken"),
    selectionBackground: `${v("accent")}55`,
    selectionInactiveBackground: `${v("accent")}33`,
  };
}

// xterm.js colours its text and blinks its cursor with a <style> element
// it writes at run time, which the content security policy refuses. The
// same rules go in through a constructed style sheet, which it allows.
const terminalSheet = new CSSStyleSheet();
document.adoptedStyleSheets = [...document.adoptedStyleSheets, terminalSheet];

function mirrorTerminalStyles() {
  const rules = [...$("terminal-host").querySelectorAll("style")].map((el) => el.textContent).join("\n");
  try {
    terminalSheet.replaceSync(rules);
  } catch (err) {
    console.error("terminal styles", err);
  }
}

function retheme() {
  for (const t of terminals.values()) for (const e of t.list) e.term.options.theme = terminalTheme();
  // Some file icons are drawn for a light background.
  if (files.task) renderFiles();
  if (preview.view) renderPreview(true);
  tellExtensions("theme", themeForFrames());
}

const b64Bytes = (data) => Uint8Array.from(atob(data), (c) => c.charCodeAt(0));

function tabsOf(taskId) {
  if (!terminals.has(taskId)) terminals.set(taskId, { list: [], active: null });
  return terminals.get(taskId);
}

const activeTerminal = (taskId = state.current) => {
  const t = terminals.get(taskId);
  return t && t.list.find((e) => e.key === t.active);
};

/** A link a program named (OSC 8) waiting for its second click. */
let namedLink = { uri: null, until: 0 };

/** Opens a link from the terminal: with ⌘ (Ctrl elsewhere) held, as
 * editors do, so a plain click still selects. A link a program named
 * (`named`) can read as one address and go to another, so it opens on a
 * second click, once its real address has been shown. */
function terminalLink(event, uri, named = false) {
  const mod = PLATFORM === "mac" ? "⌘" : "Ctrl";
  if (!(PLATFORM === "mac" ? event.metaKey : event.ctrlKey)) {
    toast(`${mod}+click opens ${uri}`);
    return;
  }
  if (named && !(namedLink.uri === uri && Date.now() < namedLink.until)) {
    namedLink = { uri, until: Date.now() + 8000 };
    toast(`This link goes to ${uri}. ${mod}+click it again to open it.`);
    return;
  }
  namedLink = { uri: null, until: 0 };
  call("open_url", { url: uri }).catch(fail);
}

async function newTerminal(taskId = state.current) {
  const tabs = tabsOf(taskId);
  const el = h("div", { class: "terminal-view", "data-task": taskId });
  $("terminal-host").append(el);
  const term = new Terminal({
    fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--mono").trim() || "monospace",
    fontSize: 13,
    lineHeight: 1.25,
    fontWeightBold: "600",
    cursorBlink: true,
    cursorStyle: "block",
    cursorInactiveStyle: "outline",
    scrollback: 10000,
    allowProposedApi: true,
    theme: terminalTheme(),
    // Links a program writes (OSC 8) go the way plain URLs do: by
    // Kule's opener, http(s) alone, on Cmd or Ctrl+click, and only once
    // the address they really go to has been shown.
    linkHandler: { activate: (event, uri) => terminalLink(event, uri, true), allowNonHttpProtocols: false },
  });
  const fit = new FitAddon.FitAddon();
  const search = new SearchAddon.SearchAddon();
  term.loadAddon(fit);
  term.loadAddon(search);
  term.loadAddon(new WebLinksAddon.WebLinksAddon(terminalLink));
  const n = Math.max(0, ...tabs.list.map((e) => e.n)) + 1;
  const entry = { key: ++terminalKeys, n, term, fit, search, el, id: null, shell: "shell", ended: false };
  tabs.list.push(entry);
  tabs.active = entry.key;
  const name = regionOf("terminal");
  if (name) {
    layout.regions[name].active = "terminal";
    layout.regions[name].open = true;
    keepLayout();
  }
  term.open(el);
  renderTerminals(taskId);
  fit.fit();
  term.onData((data) => {
    if (entry.ended) {
      if (data === "\r") closeTerminal(taskId, entry.key).then(() => newTerminal(taskId));
      return;
    }
    if (entry.id) call("terminal_write", { id: entry.id, data }).catch(() => {});
  });
  term.onResize(({ cols, rows }) => {
    if (entry.id && !entry.ended) call("terminal_resize", { id: entry.id, cols, rows }).catch(() => {});
  });
  try {
    const opened = await call("terminal_open", { task: taskId, cols: term.cols, rows: term.rows });
    entry.id = opened.id;
    entry.shell = opened.shell;
  } catch (err) {
    entry.ended = true;
    term.write(`\x1b[31m${String(err)}\x1b[0m\r\n`);
  }
  renderTerminals(taskId);
  term.focus();
  return entry;
}

/** The open task's terminal in front, and the tab bars that show them. */
function renderTerminals(taskId = state.current) {
  const tabs = terminals.get(taskId) || { list: [], active: null };
  for (const t of terminals.values()) for (const e of t.list) e.el.hidden = true;
  const front = tabs.list.find((e) => e.key === tabs.active);
  if (front) front.el.hidden = false;
  $("terminal-empty").hidden = tabs.list.length > 0;
  renderLayout();
  if (front) requestAnimationFrame(() => front.fit.fit());
}

function focusTerminal(taskId, key) {
  const tabs = tabsOf(taskId);
  tabs.active = key;
  const name = regionOf("terminal");
  if (name) {
    layout.regions[name].active = "terminal";
    layout.regions[name].open = true;
    keepLayout();
  }
  renderTask();
  renderTerminals(taskId);
  activeTerminal(taskId)?.term.focus();
}

/** Shows the open task's terminals, starting one the first time. */
async function showTerminal(taskId) {
  if (!window.Terminal) return;
  const tabs = tabsOf(taskId);
  // Shown with none open: one starts, as it would in an editor.
  if (!tabs.list.length) {
    if (tabs.starting) return;
    tabs.starting = true;
    try {
      await newTerminal(taskId);
    } finally {
      tabs.starting = false;
    }
    return;
  }
  renderTerminals(taskId);
}

async function closeTerminal(taskId, key) {
  const tabs = terminals.get(taskId);
  const entry = tabs && tabs.list.find((e) => e.key === key);
  if (!entry) return;
  if (entry.id && !entry.ended) await call("terminal_close", { id: entry.id }).catch(() => {});
  entry.term.dispose();
  entry.el.remove();
  const at = tabs.list.indexOf(entry);
  tabs.list.splice(at, 1);
  if (tabs.active === key) tabs.active = (tabs.list[at] || tabs.list[at - 1] || {}).key ?? null;
  const name = regionOf("terminal");
  if (!tabs.list.length && name && layout.regions[name].active === "terminal") {
    const other = layout.regions[name].panels.find((p) => p !== "terminal");
    if (other) layout.regions[name].active = other;
    keepLayout();
  }
  renderTask();
  renderTerminals(taskId);
  activeTerminal(taskId)?.term.focus();
}

function stepTerminal(by) {
  const tabs = terminals.get(state.current);
  if (!tabs || tabs.list.length < 2) return;
  const at = tabs.list.findIndex((e) => e.key === tabs.active);
  focusTerminal(state.current, tabs.list[(at + by + tabs.list.length) % tabs.list.length].key);
}

function onTerminal(event) {
  for (const [taskId, tabs] of terminals) {
    const entry = tabs.list.find((e) => e.id === event.id);
    if (!entry) continue;
    if (event.kind === "output") entry.term.write(b64Bytes(event.data));
    else if (event.kind === "exit") {
      entry.ended = true;
      entry.term.write("\r\n\x1b[2m[the shell ended; press Enter for a new one]\x1b[0m\r\n");
      if (taskId === state.current) renderTerminals(taskId);
    }
    return;
  }
}

/** Ends a deleted task's shells. */
function forgetTerminal(taskId) {
  const tabs = terminals.get(taskId);
  if (!tabs) return;
  for (const e of tabs.list) {
    if (e.id && !e.ended) call("terminal_close", { id: e.id }).catch(() => {});
    e.term.dispose();
    e.el.remove();
  }
  terminals.delete(taskId);
}

// --- Finding text in the terminal.

const FIND_LOOK = {
  matchBackground: "#fbbf2455",
  matchBorder: "#fbbf24",
  matchOverviewRuler: "#fbbf24",
  activeMatchBackground: "#ff5a3c88",
  activeMatchBorder: "#ff5a3c",
  activeMatchColorOverviewRuler: "#ff5a3c",
};

function openFind() {
  const entry = activeTerminal();
  if (!entry) return;
  $("terminal-find").hidden = false;
  const input = $("terminal-find-input");
  input.focus();
  input.select();
}

function find(backward) {
  const entry = activeTerminal();
  const text = $("terminal-find-input").value;
  if (!entry || !text) return;
  const found = backward
    ? entry.search.findPrevious(text, { decorations: FIND_LOOK })
    : entry.search.findNext(text, { decorations: FIND_LOOK });
  $("terminal-find-input").classList.toggle("missing", !found);
}

function closeFind() {
  $("terminal-find").hidden = true;
  const entry = activeTerminal();
  if (entry) {
    entry.search.clearDecorations();
    entry.term.focus();
  }
}

// --- The prompt line: one line, growing with what is written in it, up
// to a third of the panel, as a terminal's command line wraps.

function fitPrompt() {
  const prompt = $("prompt");
  prompt.style.height = "auto";
  const max = Math.max(60, Math.round($("panel-live").clientHeight / 3));
  prompt.style.height = `${Math.min(prompt.scrollHeight, max)}px`;
  prompt.style.overflowY = prompt.scrollHeight > max ? "auto" : "hidden";
}

// --- Appearance ------------------------------------------------------------

// Kule's themes and the person's own, kept by the daemon with the one in
// use, so the window opens the way it was left.

const appearance = {
  /** { current, custom } as the daemon keeps it. */
  kept: { current: "system", custom: [] },
  /** The theme being written: a copy, not yet kept. */
  editing: null,
  isNew: false,
};

const allThemes = () => [...BUILT_IN_THEMES, ...appearance.kept.custom, ...EXT.themes];
const themeById = (id) => allThemes().find((t) => t.id === id) || BUILT_IN_THEMES[0];
const currentTheme = () => themeById(appearance.kept.current);

async function loadThemes() {
  try {
    appearance.kept = await call("themes");
  } catch (err) {
    fail(err);
  }
  if (!appearance.editing) applyTheme(currentTheme());
  applyMotion(appearance.kept.motion);
  retheme();
}

/** Keeps what `next` changes of the themes and the motion; the rest as it is. */
async function keepThemes(next) {
  try {
    appearance.kept = await call("set_themes", { themes: { ...appearance.kept, ...next } });
    return true;
  } catch (err) {
    fail(err);
    return false;
  } finally {
    if (!appearance.editing) applyTheme(currentTheme());
    applyMotion(appearance.kept.motion);
    retheme();
    renderAppearance();
  }
}

const MOTIONS = [
  ["system", "As the computer says", "Moves, unless the computer's “reduce motion” is on."],
  ["on", "On", "Menus, tabs, new events, questions and the task list move a little as they change."],
  ["off", "Off", "Nothing moves; every change shows at once."],
];

function setMotion(motion) {
  return keepThemes({ motion });
}

/** Turns motion off when the window moves, on when it does not. */
function toggleMotion() {
  const next = moving() ? "off" : "on";
  setMotion(next).then((kept) => kept && toast(`Motion ${next}`));
}

function useTheme(id) {
  return keepThemes({ ...appearance.kept, current: id });
}

/** Moves to the next theme in the list, for the keyboard. */
function nextTheme() {
  const themes = allThemes();
  const at = themes.findIndex((t) => t.id === appearance.kept.current);
  const next = themes[(at + 1) % themes.length];
  useTheme(next.id).then(() => toast(`Theme: ${next.name}`));
}

async function openAppearance() {
  state.view = "appearance";
  renderSidebar();
  renderTask();
  renderAppearance();
}

/** A small picture of a theme: a panel on its background, text, the
 * accent and the three verdicts. */
function themeSwatch(theme) {
  const c = (p) => themeColor(theme.scheme ? theme : { scheme: matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light" }, p);
  const box = (style) => h("span", { class: "sw", style });
  return h(
    "span",
    { class: "swatch-preview", style: `background:${c("bg")};border-color:${c("border")}`, "aria-hidden": "true" },
    h(
      "span",
      { class: "sw-panel", style: `background:${c("surface")};border-color:${c("border")}` },
      box(`background:${c("text")};width:56%`),
      box(`background:${c("muted")};width:38%`),
      h(
        "span",
        { class: "sw-row" },
        box(`background:${c("accent")};width:30%;height:9px;border-radius:5px`),
        box(`background:${c("ok")};width:9px;height:9px;border-radius:50%`),
        box(`background:${c("ask")};width:9px;height:9px;border-radius:50%`),
        box(`background:${c("deny")};width:9px;height:9px;border-radius:50%`)
      )
    )
  );
}

function themeCard(theme) {
  const current = theme.id === appearance.kept.current;
  return h(
    "button",
    {
      class: "theme-card",
      type: "button",
      role: "radio",
      "aria-checked": String(current),
      "data-key": `theme:${theme.id}`,
      onclick: () => useTheme(theme.id),
    },
    themeSwatch(theme),
    h("span", { class: "theme-name" }, theme.name),
    h("span", { class: "theme-kind" }, theme.scheme ? (theme.scheme === "dark" ? "dark" : "light") : "follows your computer", theme.from ? ` · from ${theme.from}` : "")
  );
}

/** A copy of `theme` to write, under a new id. */
function newThemeFrom(theme) {
  const base = theme.scheme ? theme : themeById(matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light");
  const id = `custom-${Array.from(crypto.getRandomValues(new Uint8Array(4)), (b) => b.toString(16).padStart(2, "0")).join("")}`;
  appearance.editing = {
    id,
    name: `${base.name} (mine)`.slice(0, 40),
    scheme: base.scheme,
    colors: Object.fromEntries(THEME_PARTS.map(([p]) => [p, themeColor(base, p)])),
  };
  appearance.isNew = true;
  if (state.view !== "appearance") openAppearance();
  else renderAppearance();
}

function editTheme(theme) {
  appearance.editing = JSON.parse(JSON.stringify(theme));
  appearance.editing.colors = Object.fromEntries(THEME_PARTS.map(([p]) => [p, themeColor(theme, p)]));
  appearance.isNew = false;
  renderAppearance();
}

function stopEditing() {
  appearance.editing = null;
  applyTheme(currentTheme());
  renderAppearance();
}

async function saveEditedTheme() {
  const theme = appearance.editing;
  const custom = appearance.kept.custom.filter((t) => t.id !== theme.id);
  custom.push(theme);
  appearance.editing = null;
  if (await keepThemes({ current: theme.id, custom })) toast(`Kept ${theme.name}`);
}

async function importTheme() {
  let file;
  try {
    file = await call("import_theme");
  } catch (err) {
    fail(err);
    return;
  }
  if (!file) return;
  const id = `custom-${Array.from(crypto.getRandomValues(new Uint8Array(4)), (b) => b.toString(16).padStart(2, "0")).join("")}`;
  const theme = { id, name: file.name, scheme: file.scheme, colors: file.colors };
  if (await keepThemes({ current: id, custom: [...appearance.kept.custom, theme] })) toast(`Took in ${file.name}`);
}

async function exportTheme(theme) {
  if (!theme.scheme) {
    toast("System follows your computer; export Light or Dark instead");
    return;
  }
  const colors = Object.fromEntries(THEME_PARTS.map(([p]) => [p, themeColor(theme, p)]));
  try {
    const path = await call("export_theme", { theme: { kule_theme: 1, name: theme.name, scheme: theme.scheme, colors } });
    if (path) toast(`Saved ${theme.name} to ${path}`);
  } catch (err) {
    fail(err);
  }
}

function themeEditor() {
  const theme = appearance.editing;
  const preview = () => applyTheme(theme);
  const checks = () =>
    readability(theme)
      .filter((r) => r.ratio < r.need)
      .map((r) => h("li", {}, `${r.label}: ${r.ratio.toFixed(1)} to 1, needs ${r.need}`));
  const warnings = h("ul", { class: "theme-warnings", "aria-live": "polite" }, checks());
  const refresh = () => {
    preview();
    replace(warnings, ...checks());
    if (!warnings.children.length) warnings.append(h("li", { class: "ok" }, "Every pair reads clearly."));
  };
  const name = h("input", {
    id: "theme-name",
    type: "text",
    maxlength: 40,
    value: theme.name,
    autocomplete: "off",
    oninput: (e) => (theme.name = e.target.value),
  });
  const scheme = h(
    "select",
    { id: "theme-scheme", onchange: (e) => ((theme.scheme = e.target.value), refresh()) },
    h("option", { value: "light", selected: theme.scheme === "light" }, "Light"),
    h("option", { value: "dark", selected: theme.scheme === "dark" }, "Dark")
  );
  const parts = THEME_PARTS.map(([part, label]) => {
    const id = `theme-part-${part}`;
    const text = h("input", {
      type: "text",
      class: "hex",
      value: theme.colors[part],
      maxlength: 7,
      spellcheck: "false",
      "aria-label": `${label}, as #rrggbb`,
    });
    const picker = h("input", { id, type: "color", value: theme.colors[part] });
    picker.addEventListener("input", () => {
      theme.colors[part] = picker.value;
      text.value = picker.value;
      refresh();
    });
    text.addEventListener("change", () => {
      const v = text.value.trim().toLowerCase();
      if (isHex(v)) {
        theme.colors[part] = v;
        picker.value = v;
        refresh();
      } else {
        text.value = theme.colors[part];
        toast("A colour is written like #1b1f27");
      }
    });
    return h("label", { class: "theme-part", for: id }, picker, h("span", {}, label), text);
  });
  setTimeout(refresh, 0);
  return h(
    "div",
    { class: "agent-editor theme-editor" },
    h("h3", {}, appearance.isNew ? "New theme" : `Edit ${theme.name}`),
    h("p", { class: "muted" }, "The window shows your changes as you make them; nothing is kept until you save."),
    h(
      "div",
      { class: "theme-meta" },
      h("label", { class: "agent-field", for: "theme-name" }, h("span", { class: "agent-field-label" }, "Name"), name),
      h("label", { class: "agent-field", for: "theme-scheme" }, h("span", { class: "agent-field-label" }, "Light or dark"), scheme)
    ),
    h("div", { class: "theme-parts" }, parts),
    warnings,
    h(
      "div",
      { class: "agent-actions" },
      h("button", { class: "button primary", type: "button", "data-key": "theme-save", onclick: saveEditedTheme }, "Save theme"),
      h("button", { class: "button ghost", type: "button", "data-key": "theme-cancel", onclick: stopEditing }, "Cancel")
    )
  );
}

function renderAppearance() {
  if (state.view !== "appearance") return;
  const current = currentTheme();
  const custom = current.id.startsWith("custom-");
  const actions = [
    h("button", { class: "button", type: "button", "data-key": "theme-new", onclick: () => newThemeFrom(current) }, "New theme from this one"),
    custom ? h("button", { class: "button ghost", type: "button", "data-key": "theme-edit", onclick: () => editTheme(current) }, "Edit") : null,
    current.scheme ? h("button", { class: "button ghost", type: "button", "data-key": "theme-export", onclick: () => exportTheme(current) }, "Export…") : null,
    h("button", { class: "button ghost", type: "button", "data-key": "theme-import", onclick: importTheme }, "Import…"),
  ];
  if (custom) {
    const remove = armed("Delete", `Click again to delete ${current.name}`, false, () =>
      keepThemes({ current: "system", custom: appearance.kept.custom.filter((t) => t.id !== current.id) })
    );
    remove.dataset.key = "theme-delete";
    actions.push(remove);
  }
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Appearance"),
      h(
        "p",
        { class: "muted" },
        "Kule's themes and your own. A theme is a set of colours for the window's parts; it can be shared as a .kule-theme.json file, which carries colours and nothing else."
      )
    ),
    appearance.editing ? themeEditor() : null,
    h("section", { class: "key-group", "aria-label": "Kule's themes" }, h("h3", {}, "Kule's themes"), h("div", { class: "theme-grid", role: "radiogroup", "aria-label": "Kule's themes" }, BUILT_IN_THEMES.map(themeCard))),
    appearance.kept.custom.length
      ? h("section", { class: "key-group", "aria-label": "Your themes" }, h("h3", {}, "Your themes"), h("div", { class: "theme-grid", role: "radiogroup", "aria-label": "Your themes" }, appearance.kept.custom.map(themeCard)))
      : null,
    EXT.themes.length
      ? h("section", { class: "key-group", "aria-label": "From extensions" }, h("h3", {}, "From extensions"), h("div", { class: "theme-grid", role: "radiogroup", "aria-label": "Themes from extensions" }, EXT.themes.map(themeCard)))
      : null,
    appearance.editing ? null : h("div", { class: "agent-actions" }, h("span", { class: "muted" }, `In use: ${current.name}`), ...actions),
    appearance.editing
      ? null
      : h(
          "section",
          { class: "key-group", "aria-label": "Motion" },
          h("h3", {}, "Motion"),
          h(
            "div",
            { class: "policy-pick", role: "radiogroup", "aria-label": "Motion" },
            MOTIONS.map(([value, name, text]) =>
              h(
                "label",
                { class: "policy-option", for: `motion-${value}` },
                h("input", {
                  type: "radio",
                  name: "motion",
                  id: `motion-${value}`,
                  "data-key": `motion-${value}`,
                  value,
                  checked: (appearance.kept.motion || "system") === value,
                  onchange: () => setMotion(value),
                }),
                h("span", { class: "policy-name" }, name),
                h("span", { class: "policy-text" }, text)
              )
            )
          )
        )
  );
}

// --- Profile ---------------------------------------------------------------

// The person's choices in one file: to take to another computer, or keep.
// Each part goes only when they tick it; secrets, remembered approvals and
// a policy of their own never go.

const PROFILE_PARTS = [
  ["appearance", "Appearance", "The theme in use, your own themes and the motion"],
  ["shortcuts", "Shortcuts", "The shortcuts you set yourself"],
  ["policy", "Policy", "Kule's balanced or careful policy"],
  ["budget", "Daily budget", "What every task may cost together in a day"],
  ["limits", "Limits", "What every task's runs may take: processor, memory, disk, GPU, time"],
  ["agents", "Your agents", "The agent profiles you added; they wait for approval on the other side"],
];

const profileView = {
  exportParts: new Set(PROFILE_PARTS.map(([p]) => p)),
  /** { text, file } of a profile picked to import. */
  picked: null,
  importParts: new Set(),
  /** What the person has now, to say what an export would carry. */
  now: null,
};

async function openProfile() {
  state.view = "profile";
  renderSidebar();
  renderTask();
  renderProfile();
  const [policy, daily, agents, limits] = await Promise.all([
    call("policy").catch(() => null),
    call("daily_budget").catch(() => null),
    call("agent_list").catch(() => []),
    call("limits").catch(() => null),
  ]);
  profileView.now = { policy, daily, limits, agents: agents.filter((l) => l.origin === "person").map((l) => l.profile.name) };
  renderProfile();
}

/** What a part holds, in a few words, from a profile file or from now. */
function partSummary(part, from) {
  switch (part) {
    case "appearance": {
      const a = from.appearance;
      if (!a) return null;
      const inUse = [...BUILT_IN_THEMES, ...a.custom].find((t) => t.id === a.current);
      return `${plural(a.custom.length, "theme")} of your own; ${inUse ? inUse.name : a.current} in use`;
    }
    case "shortcuts":
      return from.shortcuts ? `${plural(Object.keys(from.shortcuts).length, "shortcut")} of your own` : null;
    case "policy":
      return from.policy ? `the ${from.policy} policy` : null;
    case "budget":
      if (!from.budget) return null;
      return from.budget.daily ? `${money(from.budget.daily)} a day` : "no daily budget";
    case "limits":
      return from.limits ? limitsSummary(from.limits) : null;
    case "agents":
      return from.agents ? (from.agents.length ? from.agents.join(", ") : "none") : null;
    default:
      return null;
  }
}

function profileNow() {
  const now = profileView.now;
  return {
    appearance: appearance.kept,
    shortcuts: keys.chosen,
    policy: now && now.policy,
    budget: now && now.daily ? { daily: now.daily.budget } : null,
    limits: now && now.limits,
    agents: now ? now.agents : null,
  };
}

function partBox(prefix, part, label, summary, set, disabled, note) {
  const id = `${prefix}-${part}`;
  return h(
    "label",
    { class: "profile-part", for: id },
    h("input", {
      id,
      type: "checkbox",
      "data-key": id,
      checked: set.has(part) && !disabled,
      disabled,
      onchange: (e) => (e.target.checked ? set.add(part) : set.delete(part)),
    }),
    h("span", { class: "profile-part-label" }, label),
    h("span", { class: "muted" }, note || summary || "")
  );
}

async function exportProfile() {
  const parts = PROFILE_PARTS.map(([p]) => p).filter((p) => profileView.exportParts.has(p));
  if (!parts.length) {
    toast("Tick at least one part to export");
    return;
  }
  try {
    const path = await call("export_profile", { parts });
    if (path) toast(`Saved your profile to ${path}`);
  } catch (err) {
    fail(err);
  }
}

async function importProfile() {
  let picked;
  try {
    picked = await call("import_profile");
  } catch (err) {
    fail(err);
    return;
  }
  if (!picked) return;
  profileView.picked = picked;
  const f = picked.file;
  profileView.importParts = new Set(PROFILE_PARTS.map(([p]) => p).filter((p) => f[p] !== undefined && f[p] !== null));
  profileView.weaker = await weakerBy(f);
  profileView.weakerAccepted = false;
  if (state.view !== "profile") await openProfile();
  else renderProfile();
}

/** What applying profile file `f` would make less strict than it is now,
 * part by part: [part, what changes]. */
async function weakerBy(f) {
  const [policy, limits, daily] = await Promise.all([
    call("policy").catch(() => null),
    call("limits").catch(() => null),
    call("daily_budget").catch(() => null),
  ]);
  const out = [];
  // What cannot be read now cannot be compared; it counts as less strict.
  const unread = "what Kule has now could not be read to compare";
  if (f.policy && !policy) out.push(["policy", `the policy: ${unread}`]);
  if (f.limits && !limits) out.push(["limits", `the limits: ${unread}`]);
  if (f.budget && !daily) out.push(["budget", `the daily budget: ${unread}`]);
  if (f.policy === "balanced" && policy === "careful") out.push(["policy", "the policy goes from Careful to Balanced"]);
  if (f.policy && policy === "own") out.push(["policy", `the policy you wrote is replaced by Kule's ${f.policy} one`]);
  if (f.limits && limits) {
    for (const field of LIMIT_FIELDS) {
      const now = limits[field.key];
      const then = f.limits[field.key];
      if (now != null && (then == null || then > now)) {
        out.push(["limits", `${field.label.toLowerCase()}: ${limitText(field, now)} → ${then == null ? "no limit" : limitText(field, then)}`]);
      }
    }
    if (limits.gpu === false && f.limits.gpu === true) out.push(["limits", "the GPU becomes usable"]);
  }
  const was = daily && daily.budget;
  const comes = f.budget && f.budget.daily;
  if (was && (!comes || comes.currency !== was.currency || comes.amount > was.amount)) {
    out.push(["budget", `the daily budget: ${was.amount} ${was.currency} → ${comes ? `${comes.amount} ${comes.currency}` : "none"}`]);
  }
  return out;
}

async function applyProfile() {
  const { text } = profileView.picked;
  const parts = PROFILE_PARTS.map(([p]) => p).filter((p) => profileView.importParts.has(p));
  if (!parts.length) {
    toast("Tick at least one part to apply");
    return;
  }
  // Less strict than now only when the person said so.
  const weaker = (profileView.weaker || []).filter(([part]) => parts.includes(part));
  if (weaker.length && !profileView.weakerAccepted) {
    toast("Tick that you want Kule less strict, or leave those parts out", "error");
    return;
  }
  try {
    const said = await call("apply_profile", { text, parts });
    for (const line of said) toast(line);
  } catch (err) {
    fail(err);
    return;
  }
  profileView.picked = null;
  await Promise.all([loadThemes(), loadShortcuts(), loadAgentChoices(), refreshPolicyViews()]);
  await openProfile();
}

function renderProfile() {
  if (state.view !== "profile") return;
  const now = profileNow();
  const ownPolicy = now.policy === "own";
  const picked = profileView.picked;
  const exportBlock = h(
    "section",
    { class: "agent-card profile-block", "aria-label": "Export" },
    h("h3", {}, "Export"),
    h("p", { class: "muted" }, "Writes the parts you tick to a .kule-profile.json file. Secrets, remembered approvals and a policy you wrote yourself are never in it."),
    h(
      "div",
      { class: "profile-parts" },
      PROFILE_PARTS.map(([part, label, what]) =>
        partBox(
          "export",
          part,
          label,
          partSummary(part, now) || what,
          profileView.exportParts,
          part === "policy" && ownPolicy,
          part === "policy" && ownPolicy ? "Your policy is one you wrote; it stays here" : null
        )
      )
    ),
    h("div", { class: "agent-actions" }, h("button", { class: "button primary", type: "button", "data-key": "profile-export", onclick: exportProfile }, "Export…"))
  );
  const importBlock = picked
    ? h(
        "section",
        { class: "agent-card profile-block", "aria-label": "Import" },
        h("h3", {}, "Import"),
        h("p", { class: "muted" }, "This profile holds the parts below. Tick the ones to apply: themes join yours, shortcuts, the policy, the budget and the limits take the profile's, and its agents wait for your approval before they run."),
        h(
          "div",
          { class: "profile-parts" },
          PROFILE_PARTS.filter(([part]) => picked.file[part] !== undefined && picked.file[part] !== null).map(([part, label]) =>
            partBox("import", part, label, partSummary(part, { ...picked.file, agents: picked.file.agents && picked.file.agents.map((a) => a.name) }), profileView.importParts, false, null)
          )
        ),
        (profileView.weaker || []).length
          ? h(
              "section",
              { class: "isolation", "aria-label": "Less strict" },
              h("h3", {}, "This profile makes Kule less strict"),
              h("ul", {}, profileView.weaker.map(([, what]) => h("li", {}, what))),
              h(
                "label",
                { class: "isolation-accept", for: "profile-weaker" },
                h("input", {
                  type: "checkbox",
                  id: "profile-weaker",
                  "data-key": "profile-weaker",
                  checked: profileView.weakerAccepted,
                  onchange: (e) => (profileView.weakerAccepted = e.currentTarget.checked),
                }),
                " I want these, as the profile says"
              )
            )
          : null,
        h(
          "div",
          { class: "agent-actions" },
          h("button", { class: "button primary", type: "button", "data-key": "profile-apply", onclick: (e) => inFlight(e.currentTarget, applyProfile) }, "Apply"),
          h("button", { class: "button ghost", type: "button", "data-key": "profile-cancel", onclick: () => ((profileView.picked = null), renderProfile()) }, "Cancel")
        )
      )
    : h(
        "section",
        { class: "agent-card profile-block", "aria-label": "Import" },
        h("h3", {}, "Import"),
        h("p", { class: "muted" }, "Reads a .kule-profile.json file and shows what is in it before anything changes."),
        h("div", { class: "agent-actions" }, h("button", { class: "button", type: "button", "data-key": "profile-import", onclick: importProfile }, "Import…"))
      );
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Profile"),
      h("p", { class: "muted" }, "Your choices in one file, all of them or the parts you pick: to set up Kule on another computer the way you have it here, or to keep a copy.")
    ),
    importBlock,
    exportBlock
  );
}

// --- Keyboard --------------------------------------------------------------

// Everything the window does is a command here, with a default shortcut or
// none; the person can give each one their own in Settings → Shortcuts, and
// the daemon keeps those choices in Kule's home. Every new feature adds its
// command to this list.

function showTab(name) {
  state.view = "tasks";
  revealPanel(name);
}

/** Clicks the button `selector` names when it is there to click; says why
 * not otherwise. An armed button (Delete, Roll back, Undo) needs the
 * shortcut twice, as it needs two clicks. */
function press(selector, missing) {
  const button = document.querySelector(selector);
  if (!button || button.disabled || button.closest("[hidden]")) {
    toast(missing);
    return;
  }
  button.focus();
  button.click();
}

/** Runs `then` with the task shown, or says there is none. */
function withTask(then) {
  const task = currentTask();
  if (!task) {
    toast("No task is open");
    return;
  }
  if (state.view !== "tasks") {
    state.view = "tasks";
    renderSidebar();
    renderTask();
  }
  then(task);
}

/** The question to answer first: the open task's oldest, else any. */
function firstQuestion() {
  const task = currentTask();
  return (task && questionsOf(task.id)[0]) || state.questions[0];
}

async function answerFirst(approve, remember) {
  const q = firstQuestion();
  if (!q) {
    toast("No agent is waiting for an answer");
    return;
  }
  await answer(q.id, approve, remember);
  toast(`${approve ? "Approved" : "Denied"} ${q.host}:${q.port}`);
}

/** Moves to the task `by` places away in the list. */
function stepTask(by) {
  if (!state.tasks.length) return;
  const at = state.tasks.findIndex((t) => t.id === state.current);
  const next = state.tasks[(at + by + state.tasks.length) % state.tasks.length];
  select(next.id);
  document.querySelector(`#tasks [data-key="${CSS.escape(`tasks:${next.id}`)}"]`)?.focus();
}

/** Opens the open task's ⋮ menu, as a click on it would. */
function openTaskMenu() {
  const task = currentTask();
  const more = task && document.querySelector(`#tasks [data-key="${CSS.escape(`tasks:${task.id}:more`)}"]`);
  if (!more) {
    toast("No task is open");
    return null;
  }
  more.click();
  return $("task-menu");
}

/** Shows `tab` of the open task; the review's buttons are there once it is. */
function onTab(tab, then) {
  withTask(() => {
    if (!isShown(tab)) {
      showTab(tab);
      toast(`Showing ${tab === "review" ? "the review" : "the timeline"}; press the shortcut again to go on`);
      return;
    }
    then();
  });
}

const COLOR_NAMES = { red: "Red", orange: "Orange", yellow: "Yellow", green: "Green", teal: "Teal", blue: "Blue", purple: "Purple", pink: "Pink" };

/** Every command: id, what it does, its group, its default shortcut, and
 * whether it fires while the person types in a field. */
const COMMANDS = [
  { id: "app.palette", title: "Show all commands", group: "General", keys: "Mod+K", inFields: true, run: () => openPalette() },
  { id: "app.shortcuts", title: "Keyboard shortcuts", group: "General", keys: "Mod+/", inFields: true, run: () => openShortcuts(true) },
  { id: "app.settings", title: "Open Settings", group: "General", keys: "Mod+,", inFields: true, run: () => openSettings() },

  { id: "task.new", title: "New task…", group: "Tasks", keys: "Mod+N", inFields: true, run: () => newTask() },
  { id: "task.next", title: "Next task", group: "Tasks", keys: "Ctrl+Tab", inFields: true, run: () => stepTask(1) },
  { id: "task.prev", title: "Previous task", group: "Tasks", keys: "Ctrl+Shift+Tab", inFields: true, run: () => stepTask(-1) },
  { id: "task.up", title: "Move the task up the list", group: "Tasks", keys: "Alt+Shift+ArrowUp", run: () => withTask((t) => moveTask(t.id, -1)) },
  { id: "task.down", title: "Move the task down the list", group: "Tasks", keys: "Alt+Shift+ArrowDown", run: () => withTask((t) => moveTask(t.id, 1)) },
  { id: "task.menu", title: "Task actions…", group: "Tasks", keys: "Mod+.", run: () => openTaskMenu() },
  {
    id: "task.rename", title: "Rename the task…", group: "Tasks", keys: "F2",
    run: () => withTask((t) => { const menu = openTaskMenu(); if (menu) renameTask(t, menu); }),
  },
  {
    id: "task.group", title: "Put the task in a group…", group: "Tasks", keys: "",
    run: () => withTask((t) => { const menu = openTaskMenu(); if (menu) pickGroup(t, menu); }),
  },
  { id: "group.new", title: "New group…", group: "Tasks", keys: "", run: () => { revealPanel("tasks"); newGroup(); } },
  { id: "group.policy", title: "Group policies", group: "Settings", keys: "", run: () => openPolicy() },
  { id: "isolation.settings", title: "Isolation on this computer", group: "Settings", keys: "", run: () => openPolicy() },
  { id: "task.terminal", title: "Open a terminal in the task's copy", group: "Tasks", keys: "Mod+T", run: () => withTask((t) => openTerminal(t)) },
  { id: "task.fork", title: "Fork the task: a new one from its copy as it is now", group: "Tasks", keys: "Mod+Shift+D", run: () => withTask((t) => forkTask(t.id, null)) },
  { id: "task.delete", title: "Delete the task (press twice)", group: "Tasks", keys: "Mod+Backspace", run: () => withTask((t) => deleteByKey(t)) },
  ...[null, ...COLORS].map((color) => ({
    id: `task.color.${color || "none"}`,
    title: color ? `Colour the task ${COLOR_NAMES[color].toLowerCase()}` : "Take the task's colour away",
    group: "Tasks",
    keys: "",
    run: () => withTask((t) => setColor(t, color)),
  })),

  { id: "view.tasks", title: "Show Tasks", group: "Layout", keys: "Mod+0", inFields: true, run: () => revealPanel("tasks") },
  { id: "view.files", title: "Show Files", group: "Layout", keys: "Mod+Shift+E", inFields: true, run: () => revealPanel("files") },
  { id: "view.terminal", title: "Show or hide the terminal", group: "Terminal", keys: "Ctrl+`", inFields: true, run: () => withTask(() => togglePanel("terminal")) },
  { id: "chat.toggle", title: "Show or hide the agent chat", group: "Layout", keys: "Mod+Shift+J", inFields: true, run: () => withTask(() => togglePanel("live")) },
  { id: "layout.main", title: "Show or hide the main area", group: "Layout", keys: "", inFields: true, run: () => toggleRegion("main") },
  { id: "terminal.new", title: "New terminal", group: "Terminal", keys: "Ctrl+Shift+`", inFields: true, run: () => withTask((t) => { revealPanel("terminal"); newTerminal(t.id); }) },
  { id: "terminal.close", title: "Close the terminal", group: "Terminal", keys: "", run: () => withTask((t) => { const e = activeTerminal(t.id); if (e) closeTerminal(t.id, e.key); }) },
  { id: "terminal.next", title: "Next terminal", group: "Terminal", keys: "", inFields: true, run: () => stepTerminal(1) },
  { id: "terminal.prev", title: "Previous terminal", group: "Terminal", keys: "", inFields: true, run: () => stepTerminal(-1) },
  { id: "terminal.find", title: "Find in the terminal", group: "Terminal", keys: PLATFORM === "mac" ? "Mod+F" : "Mod+Shift+F", inFields: true, run: () => withTask(() => { revealPanel("terminal"); openFind(); }) },
  { id: "terminal.clear", title: "Clear the terminal", group: "Terminal", keys: "", inFields: false, run: () => activeTerminal()?.term.clear() },
  { id: "layout.left", title: "Show or hide the left side", group: "Layout", keys: "Mod+B", inFields: true, run: () => toggleRegion("left") },
  { id: "layout.bottom", title: "Show or hide the bottom panel", group: "Layout", keys: "Mod+J", inFields: true, run: () => toggleRegion("bottom") },
  { id: "layout.right", title: "Show or hide the right side", group: "Layout", keys: "Mod+Alt+B", inFields: true, run: () => toggleRegion("right") },
  ...Object.entries(REGION_NAMES).map(([name, words]) => ({
    id: `panel.to-${name}`,
    title: `Move the panel in view to ${words}`,
    group: "Layout",
    keys: "",
    run: () => movePanel(panelInView(), name),
  })),
  { id: "layout.reset", title: "Reset the layout", group: "Layout", keys: "", run: () => resetLayout() },

  { id: "view.live", title: "Show the agent chat", group: "Task", keys: "Mod+1", inFields: true, run: () => withTask(() => showTab("live")) },
  { id: "view.review", title: "Show Review", group: "Task", keys: "Mod+2", inFields: true, run: () => withTask(() => showTab("review")) },
  { id: "view.timeline", title: "Show Timeline", group: "Task", keys: "Mod+3", inFields: true, run: () => withTask(() => showTab("timeline")) },
  { id: "view.preview", title: "Show Preview", group: "Task", keys: "Mod+4", inFields: true, run: () => withTask(() => showTab("preview")) },
  { id: "preview.ask", title: "Ask the agent about what is picked in the Preview", group: "Task", keys: "Mod+Shift+A", inFields: false, run: () => withTask(() => askHere()) },
  { id: "prompt.focus", title: "Write to the agent", group: "Task", keys: "Mod+L", run: () => withTask(() => { showTab("live"); $("prompt").focus(); }) },
  {
    id: "prompt.run", title: "Run the prompt", group: "Task", keys: "Mod+Enter", inFields: true,
    run: () => withTask(() => { if (!$("send").disabled) $("composer").requestSubmit(); }),
  },
  { id: "task.budget", title: "Set the task's budget", group: "Task", keys: "", run: () => withTask((t) => openTaskBudget(t.id)) },
  { id: "task.limits", title: "Set the task's limits: processor, memory, disk, GPU, time", group: "Task", keys: "", run: () => withTask((t) => openTaskLimits(t.id)) },
  {
    id: "task.stop", title: "Stop the agent at work", group: "Task", keys: "Mod+Shift+K", inFields: true,
    run: () => withTask((t) => (t.running ? stopRun(t) : toast("No agent is at work here"))),
  },
  { id: "task.wait", title: "Set how long the agent waits for you", group: "Task", keys: "", run: () => withTask(() => { showTab("live"); $("timeout").focus(); }) },

  {
    id: "review.commit", title: "Commit, or approve and commit", group: "Review", keys: "Mod+Shift+Enter", inFields: true,
    run: () => onTab("review", () => press('#review-actions [data-key="commit"]', "Nothing to commit")),
  },
  {
    id: "review.rollback", title: "Roll back the changes (press twice)", group: "Review", keys: "Mod+Shift+Backspace",
    run: () => onTab("review", () => press('#review-actions [data-key="rollback"]', "Nothing to roll back")),
  },
  {
    id: "timeline.undo", title: "Undo the last commit (press twice)", group: "Review", keys: "Mod+Shift+U",
    run: () => onTab("timeline", () => press('#timeline [data-key="undo"]', "No commit to undo")),
  },

  { id: "question.approve", title: "Approve the waiting connection", group: "Questions", keys: "Mod+Shift+Y", inFields: true, run: () => answerFirst(true, false) },
  { id: "question.remember", title: "Approve and remember it", group: "Questions", keys: "", inFields: true, run: () => answerFirst(true, true) },
  { id: "question.deny", title: "Deny the waiting connection", group: "Questions", keys: "Mod+Shift+N", inFields: true, run: () => answerFirst(false, false) },

  ...SETTINGS.map((view) => ({
    id: `settings.${view}`,
    title: `Settings: ${view[0].toUpperCase()}${view.slice(1)}`,
    group: "Settings",
    keys: "",
    run: () => openSettings(view),
  })),
  { id: "policy.balanced", title: "Use the balanced policy", group: "Settings", keys: "", run: () => choosePolicy("balanced", refreshPolicyViews) },
  { id: "policy.careful", title: "Use the careful policy", group: "Settings", keys: "", run: () => choosePolicy("careful", refreshPolicyViews) },

  { id: "profile.export", title: "Export your profile…", group: "Settings", keys: "", run: () => openProfile() },
  { id: "profile.import", title: "Import a profile…", group: "Settings", keys: "", run: () => importProfile() },
  { id: "shortcuts.search", title: "Search the shortcuts", group: "General", keys: "", inFields: true, run: () => openShortcuts(true) },

  { id: "theme.next", title: "Next theme", group: "Appearance", keys: "", inFields: true, run: () => nextTheme() },
  { id: "motion.toggle", title: "Turn motion on or off", group: "Appearance", keys: "", inFields: true, run: () => toggleMotion() },
  { id: "theme.new", title: "New theme from the one in use…", group: "Appearance", keys: "", run: () => newThemeFrom(currentTheme()) },
  { id: "theme.import", title: "Import a theme…", group: "Appearance", keys: "", run: () => importTheme() },
  { id: "theme.export", title: "Export the theme in use…", group: "Appearance", keys: "", run: () => exportTheme(currentTheme()) },
  ...BUILT_IN_THEMES.map((t) => ({
    id: `theme.use.${t.id}`,
    title: `Theme: ${t.name}`,
    group: "Appearance",
    keys: "",
    inFields: true,
    run: () => useTheme(t.id).then(() => toast(`Theme: ${t.name}`)),
  })),
];

async function refreshPolicyViews() {
  if (state.view === "policy") await renderPolicy();
  if (!$("empty").hidden) await loadWelcome();
}

const keys = {
  /** Command id → the person's own combo; "" for none. */
  chosen: {},
  /** The command whose shortcut is being recorded. */
  recording: null,
  note: "",
  /** What the Shortcuts list is narrowed to. */
  filter: "",
};

const commandById = (id) => COMMANDS.find((c) => c.id === id);

/** The combo a command answers to: the person's, else its default. */
function bindingOf(command) {
  return normalCombo(command.id in keys.chosen ? keys.chosen[command.id] : command.keys);
}

/** Puts each command's shortcut on the controls that do the same. */
function labelControls() {
  const label = (el, id, what) => {
    const combo = bindingOf(commandById(id));
    el.title = combo ? `${what} (${formatCombo(combo)})` : what;
    if (combo) el.setAttribute("aria-keyshortcuts", ariaCombo(combo));
    else el.removeAttribute("aria-keyshortcuts");
  };
  label($("new-task"), "task.new", "New task");
  label($("new-group"), "group.new", "New group: tasks in it can have a policy of their own");
  label($("open-settings"), "app.settings", "Settings");
  label($("toggle-left"), "layout.left", "Show or hide the left side");
  label($("toggle-main"), "layout.main", "Show or hide the main area");
  label($("toggle-bottom"), "layout.bottom", "Show or hide the bottom panel");
  label($("toggle-right"), "layout.right", "Show or hide the right side");
  label($("send"), "prompt.run", "Run (Enter; Shift+Enter for a new line)");
  label($("stop"), "task.stop", "Stop: cancel the agent's turn; it is killed if it does not end it within seconds");
  $("send-keys").textContent = "↵";
  renderLayout();
}

async function loadShortcuts() {
  try {
    keys.chosen = await call("shortcuts");
  } catch (err) {
    fail(err);
  }
  labelControls();
  syncMenu();
}

async function saveShortcuts(chosen) {
  try {
    keys.chosen = await call("set_shortcuts", { shortcuts: chosen });
  } catch (err) {
    fail(err);
  }
  labelControls();
  syncMenu();
  if (state.view === "shortcuts") renderShortcuts();
}

/** Keys text editing owns as well; a menu's shortcut would fire in a text
 * field and take them from it, so the menu shows none for them. */
const EDITING_KEYS = new Set(["Backspace", "Delete", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown"]);

function menuSafe(combo) {
  const parts = normalCombo(combo).split("+");
  const key = parts.pop();
  if (EDITING_KEYS.has(key)) return false;
  // Outside macOS a menu's Ctrl or Alt shortcut would reach past the
  // terminal, whose shell owns those; the menu takes only Ctrl+Shift ones
  // and function keys there.
  if (PLATFORM !== "mac" && !(parts.includes("Mod") && parts.includes("Shift")) && !/^F([1-9]|1[0-2])$/.test(key)) return false;
  // A letter with Mod alone is often an editing command (⌘B, ⌘I in some
  // fields); the ones Kule uses are its own, the system's are reserved.
  return !isReserved(combo) && isUsable(combo);
}

/** The menu's accelerator for a combo: CmdOrCtrl+Shift+K. */
function accelerator(combo) {
  return normalCombo(combo)
    .split("+")
    .map((p) => ({ Mod: "CmdOrCtrl", Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift", Plus: "Plus" })[p] || p)
    .join("+");
}

/** Tells the menu bar which shortcut goes next to each command; none while
 * one is being recorded, so the keys reach the window. */
function syncMenu() {
  if (!window.__TAURI__) return;
  const keysForMenu = {};
  if (!keys.recording) {
    for (const c of COMMANDS) {
      const combo = bindingOf(c);
      if (combo && menuSafe(combo)) keysForMenu[c.id] = accelerator(combo);
    }
  }
  call("set_menu_keys", { keys: keysForMenu }).catch(() => {});
}

/** A command the menu bar chose. */
function onMenu(id) {
  const command = commandById(id);
  if (command) command.run();
}

function onKey(e) {
  if (e.defaultPrevented || e.isComposing) return;
  if (keys.recording) {
    recordKey(e);
    return;
  }
  // The palette, a task's menu and the recording keep their own keys.
  if ($("palette") || $("task-menu")) return;
  const combos = eventCombos(e).map(normalCombo);
  if (!combos.length) return;
  const typing = typingIn(document.activeElement);
  // In the terminal, Ctrl and Alt belong to the shell (Ctrl+L, Ctrl+B,
  // Alt+F): outside macOS only Ctrl+Shift shortcuts and function keys are
  // Kule's there.
  const inTerminal = Boolean(document.activeElement && document.activeElement.closest && document.activeElement.closest(".terminal-host"));
  const shellOwns = (combo) => {
    if (!inTerminal || PLATFORM === "mac") return false;
    const parts = combo.split("+");
    const key = parts[parts.length - 1];
    return !(parts.includes("Mod") && parts.includes("Shift")) && !/^F([1-9]|1[0-2])$/.test(key);
  };
  // A binding that is a key alone (kept from before, or brought in some
  // other way) never fires: it would answer as the person types.
  const command = COMMANDS.find((c) => {
    const combo = bindingOf(c);
    return combo && isUsable(combo) && combos.includes(combo) && (!typing || c.inFields) && !shellOwns(combo);
  });
  if (!command) return;
  e.preventDefault();
  command.run();
}

// --- The palette: every command, by name.

function openPalette() {
  if ($("palette")) return;
  const opener = document.activeElement;
  const input = h("input", {
    id: "palette-input",
    type: "text",
    autocomplete: "off",
    spellcheck: "false",
    placeholder: "Type a command",
    role: "combobox",
    "aria-expanded": "true",
    "aria-controls": "palette-list",
    "aria-autocomplete": "list",
  });
  const list = h("ul", { id: "palette-list", class: "palette-list", role: "listbox", "aria-label": "Commands" });
  let shown = [];
  let at = 0;
  const close = () => {
    $("palette")?.remove();
    if (opener && opener.isConnected) opener.focus();
  };
  const run = (command) => {
    close();
    command.run();
  };
  const draw = () => {
    const words = input.value.toLowerCase().split(/\s+/).filter(Boolean);
    shown = COMMANDS.filter((c) => words.every((w) => `${c.title} ${c.group}`.toLowerCase().includes(w)));
    at = Math.min(at, Math.max(0, shown.length - 1));
    list.replaceChildren(
      ...shown.map((c, i) =>
        h(
          "li",
          {
            id: `palette-${c.id}`,
            class: "palette-item",
            role: "option",
            "aria-selected": String(i === at),
            onclick: () => run(c),
            onpointermove: () => {
              if (at !== i) {
                at = i;
                draw();
              }
            },
          },
          h("span", { class: "palette-group" }, c.group),
          h("span", { class: "palette-title" }, c.title),
          bindingOf(c) ? h("kbd", { class: "palette-keys" }, formatCombo(bindingOf(c))) : null
        )
      )
    );
    if (!shown.length) list.append(h("li", { class: "palette-empty" }, "No command by that name"));
    const current = shown[at] && $(`palette-${shown[at].id}`);
    if (current) {
      input.setAttribute("aria-activedescendant", current.id);
      current.scrollIntoView({ block: "nearest" });
    } else input.removeAttribute("aria-activedescendant");
  };
  input.addEventListener("input", () => {
    at = 0;
    draw();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (shown.length) at = (at + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length;
      draw();
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (shown[at]) run(shown[at]);
    } else if (e.key === "Tab") {
      e.preventDefault();
    }
  });
  const dialog = h(
    "div",
    { id: "palette", class: "palette-backdrop", onpointerdown: (e) => e.target === e.currentTarget && close() },
    h("div", { class: "palette", role: "dialog", "aria-modal": "true", "aria-label": "Commands" }, input, list)
  );
  document.body.append(dialog);
  draw();
  input.focus();
}

// --- Settings → Shortcuts.

async function openShortcuts(focusSearch) {
  state.view = "shortcuts";
  renderSidebar();
  renderTask();
  renderShortcuts();
  if (focusSearch) $("shortcut-search")?.focus();
}

/** Shows the rows whose command, group or keys hold every word typed. */
function filterShortcuts() {
  const words = keys.filter.toLowerCase().split(/\s+/).filter(Boolean);
  let shown = 0;
  for (const group of document.querySelectorAll("#agents-view .key-group")) {
    let any = false;
    for (const row of group.querySelectorAll(".key-row")) {
      const hit = words.every((w) => row.dataset.search.includes(w));
      row.hidden = !hit;
      any ||= hit;
      if (hit) shown += 1;
    }
    group.hidden = !any;
  }
  const none = $("shortcut-none");
  if (none) none.hidden = shown > 0;
}

function renderShortcuts() {
  if (state.view !== "shortcuts") return;
  const groups = [...new Set(COMMANDS.map((c) => c.group))];
  const changed = Object.keys(keys.chosen).length > 0;
  const row = (c) => {
    const combo = bindingOf(c);
    const own = c.id in keys.chosen;
    const recording = keys.recording === c.id;
    const search = `${c.title} ${c.group} ${combo ? formatCombo(combo) : ""} ${combo}`.toLowerCase();
    return h(
      "li",
      { class: `key-row${recording ? " recording" : ""}`, "data-search": search },
      h("span", { class: "key-title" }, c.title),
      recording
        ? h("span", { class: "key-wait", role: "status" }, keys.note || "Press the keys · Esc cancels · Backspace leaves it without one")
        : h("kbd", { class: `key-combo${combo ? "" : " none"}` }, combo ? formatCombo(combo) : "none"),
      h(
        "span",
        { class: "key-actions" },
        h(
          "button",
          {
            class: "button ghost",
            type: "button",
            "data-key": `record:${c.id}`,
            "aria-label": `Change the shortcut for ${c.title}`,
            onclick: () => startRecording(c.id),
          },
          recording ? "Waiting…" : "Change"
        ),
        own
          ? h(
              "button",
              {
                class: "button ghost",
                type: "button",
                "data-key": `reset:${c.id}`,
                "aria-label": `Put back the default for ${c.title}`,
                onclick: () => {
                  const next = { ...keys.chosen };
                  delete next[c.id];
                  saveShortcuts(next);
                },
              },
              "Default"
            )
          : null
      )
    );
  };
  replace(
    $("agents-view"),
    h(
      "header",
      { class: "agents-head" },
      h("h2", {}, "Shortcuts"),
      h(
        "p",
        { class: "muted" },
        `Every command has a place here; ${formatCombo(bindingOf(commandById("app.palette")))} finds any of them by name. Your own shortcuts are kept in Kule's home and stay after a restart. ${PLATFORM === "mac" ? "⌘ is Command." : "Ctrl is the Control key."}`
      ),
      changed
        ? h("div", { class: "agent-actions" }, h("button", { class: "button ghost", type: "button", "data-key": "reset-all", onclick: () => saveShortcuts({}) }, "Put back every default"))
        : null
    ),
    h("input", {
      id: "shortcut-search",
      class: "key-search",
      type: "search",
      "data-key": "shortcut-search",
      placeholder: "Search by name or keys, like “theme” or “⌘⇧”",
      "aria-label": "Search the shortcuts",
      autocomplete: "off",
      spellcheck: "false",
      value: keys.filter,
      oninput: (e) => {
        keys.filter = e.target.value;
        filterShortcuts();
      },
      onkeydown: (e) => {
        if (e.key === "Escape" && e.target.value) {
          e.preventDefault();
          e.target.value = "";
          keys.filter = "";
          filterShortcuts();
        }
      },
    }),
    h("p", { id: "shortcut-none", class: "muted", hidden: true }, "No command matches. The search looks at names, groups and keys."),
    ...groups.map((g) =>
      h(
        "section",
        { class: "key-group", "aria-label": g },
        h("h3", {}, g),
        h("ul", { class: "key-list" }, ...COMMANDS.filter((c) => c.group === g).map(row))
      )
    )
  );
  filterShortcuts();
}

function startRecording(id) {
  keys.recording = id;
  keys.note = "";
  syncMenu();
  renderShortcuts();
}

/** Takes the next key press as the shortcut being recorded. */
function recordKey(e) {
  const combos = eventCombos(e);
  if (!combos.length) return;
  e.preventDefault();
  e.stopPropagation();
  const id = keys.recording;
  const stop = () => {
    keys.recording = null;
    keys.note = "";
    syncMenu();
    renderShortcuts();
    document.querySelector(`[data-key="record:${CSS.escape(id)}"]`)?.focus();
  };
  if (combos[0] === "Escape") return stop();
  if (combos[0] === "Backspace" || combos[0] === "Delete") {
    keys.recording = null;
    saveShortcuts({ ...keys.chosen, [id]: "" });
    return;
  }
  const combo = normalCombo(combos[0]);
  if (isReserved(combo)) {
    keys.note = `${formatCombo(combo)} belongs to the system or to editing text; try another`;
    renderShortcuts();
    return;
  }
  if (!isUsable(combo)) {
    keys.note = `${formatCombo(combo)} would be typed; add ${PLATFORM === "mac" ? "⌘, ⌃ or ⌥" : "Ctrl or Alt"}, or use a function key`;
    renderShortcuts();
    return;
  }
  // One command per combo: the one that had it gives it up.
  const next = { ...keys.chosen, [id]: combo };
  const other = COMMANDS.find((c) => c.id !== id && bindingOf(c) === combo);
  if (other) {
    next[other.id] = "";
    toast(`${formatCombo(combo)} moved here from “${other.title}”`);
  }
  // A choice equal to the default is no choice.
  if (combo === normalCombo(commandById(id).keys)) delete next[id];
  keys.recording = null;
  saveShortcuts(next);
}

// --- Start -----------------------------------------------------------------

async function start() {
  // The title bar leaves room for macOS's window buttons, which sit in it.
  document.documentElement.dataset.platform = PLATFORM;
  $("new-task").addEventListener("click", newTask);
  $("new-group").addEventListener("click", newGroup);
  $("empty-new-task").addEventListener("click", newTask);
  $("composer").addEventListener("submit", submitPrompt);
  $("stop").addEventListener("click", () => {
    const task = currentTask();
    if (task && task.running) stopRun(task);
  });
  $("run-measure").addEventListener("click", () => {
    const task = currentTask();
    if (task) openTaskLimits(task.id);
  });
  watchSelection();
  // Enter alone is a new line in the prompt; running it is a shortcut
  // (prompt.run, ⌘/Ctrl+Enter by default).
  $("send").append(h("kbd", { class: "shortcut", "aria-hidden": "true", id: "send-keys" }));
  // A region's tabs: the arrow keys move between them.
  for (const bar of document.querySelectorAll(".region-tabs")) {
    bar.addEventListener("keydown", (e) => {
      const tabs = [...bar.querySelectorAll(".tab")];
      const at = tabs.indexOf(document.activeElement);
      if (at === -1) return;
      const to = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: tabs.length - 1 }[e.key];
      if (to === undefined) return;
      e.preventDefault();
      revealPanel(tabs[(to + tabs.length) % tabs.length].dataset.panel);
    });
  }
  for (const split of document.querySelectorAll(".split")) {
    split.addEventListener("pointerdown", splitStart);
    split.addEventListener("pointermove", splitMove);
    split.addEventListener("pointerup", splitEnd);
    split.addEventListener("pointercancel", splitEnd);
    split.addEventListener("keydown", splitKey);
  }
  document.addEventListener("pointermove", panelDragMove);
  document.addEventListener("pointerup", panelDragEnd);
  document.addEventListener("pointercancel", panelDragEnd);
  for (const n of ["left", "main", "bottom", "right"]) $(`toggle-${n}`).addEventListener("click", () => toggleRegion(n));
  $("files-tree").addEventListener("keydown", filesKey);
  // Extensions first: the kept layout places their panels and the kept
  // theme may be one of theirs.
  const extensionsLoaded = loadExtensions().then(() => loadLayout());
  // A task's menu closes on a click elsewhere, or when the window changes.
  document.addEventListener("pointerdown", (e) => {
    if (!e.target.closest("#task-menu")) closeMenu();
  });
  window.addEventListener("blur", () => closeMenu());
  window.addEventListener("resize", () => closeMenu());
  // Every shortcut; first in line, so a key being recorded goes nowhere else.
  document.addEventListener("keydown", onKey, true);
  labelControls();
  loadShortcuts();
  extensionsLoaded.then(() => loadThemes());
  // A theme that follows the computer redraws its pictures when it changes.
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => renderAppearance());

  await loadAgentChoices();
  $("open-settings").addEventListener("click", () => openSettings());
  $("isolation-warning").addEventListener("click", () => openPolicy());
  loadIsolation();
  const sections = [...document.querySelectorAll("#settings-tabs .tab")];
  for (const tab of sections) {
    tab.addEventListener("click", () => openSettings(tab.dataset.view));
  }
  $("settings-tabs").addEventListener("keydown", (e) => {
    const at = sections.indexOf(document.activeElement);
    if (at === -1) return;
    const to = { ArrowRight: at + 1, ArrowLeft: at - 1, Home: 0, End: sections.length - 1 }[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const tab = sections[(to + sections.length) % sections.length];
    openSettings(tab.dataset.view).then(() => tab.focus());
  });
  // Dragging a task in the list.
  document.addEventListener("pointermove", dragMove);
  document.addEventListener("pointerup", dragEnd);
  document.addEventListener("pointercancel", dragEnd);

  await backend.onRun(onRun);
  await backend.onQuestions(onQuestions);
  if (backend.onDaemon) await backend.onDaemon(onDaemon);
  // A task's numbers came back from a scan in the background.
  if (backend.onTasks) await backend.onTasks(() => refresh());
  if (backend.onMenu) await backend.onMenu(onMenu);
  if (backend.onTerminal) await backend.onTerminal(onTerminal);
  if (backend.onTaskChanged) await backend.onTaskChanged((change) => tellExtensions("task", change, "events.read"));
  if (backend.onExtensions) await backend.onExtensions(() => loadExtensions());
  $("terminal-empty-new").addEventListener("click", () => newTerminal());
  $("terminal-find-input").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      find(e.shiftKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeFind();
    }
  });
  $("terminal-find-input").addEventListener("input", () => find(false));
  $("terminal-find-next").addEventListener("click", () => find(false));
  $("terminal-find-prev").addEventListener("click", () => find(true));
  $("terminal-find-close").addEventListener("click", closeFind);
  new MutationObserver(mirrorTerminalStyles).observe($("terminal-host"), { subtree: true, childList: true, characterData: true });
  // The terminal fills its panel as the panel changes size.
  new ResizeObserver(() => {
    const entry = activeTerminal();
    if (entry && !entry.el.hidden && isShown("terminal")) entry.fit.fit();
  }).observe($("terminal-host"));
  $("prompt").addEventListener("input", fitPrompt);
  // Enter runs the prompt; Shift+Enter starts a new line in it.
  $("prompt").addEventListener("keydown", (e) => {
    if (e.key !== "Enter" || e.shiftKey || e.altKey || e.metaKey || e.ctrlKey || e.isComposing) return;
    e.preventDefault();
    if (!$("send").disabled) $("composer").requestSubmit();
  });
  new ResizeObserver(fitPrompt).observe($("panel-live"));
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
  // Servers come and go with what runs in the copy; looked for while the
  // window is in sight.
  setInterval(() => {
    if (document.visibilityState === "visible") loadPorts();
  }, 3000);
  await refresh();
}

let portsKey = "";

/** The servers working in the task's copy, as chips that open them. */
async function loadPorts() {
  const task = currentTask();
  const box = $("task-ports");
  if (!task || !box) return;
  let found = [];
  try {
    found = await call("ports", { task: task.id });
  } catch {
    found = [];
  }
  if (task.id !== state.current) return;
  const key = `${task.id}:${JSON.stringify(found)}`;
  if (key === portsKey) return;
  portsKey = key;
  // Loopback only, or reached from the network too.
  const local = (a) => a === "::1" || a.startsWith("127.") || a.startsWith("::ffff:127.");
  replace(
    box,
    ...found.map((l) =>
      h(
        "button",
        {
          class: `port-chip${l.reachable ? "" : " unreachable"}${l.reachable && !local(l.address) && !l.shielded ? " exposed" : ""}`,
          type: "button",
          disabled: !l.reachable,
          "data-key": `port:${l.port}`,
          title: !l.reachable
            ? `${l.process} (pid ${l.pid}) listens inside the sandbox's own network, which this computer does not reach`
            : local(l.address)
              ? `${l.process} (pid ${l.pid}) listens on this computer only, port ${l.port}. Open http://localhost:${l.port}/ in the browser`
              : l.shielded
                ? `${l.process} (pid ${l.pid}) listens on every address, port ${l.port}; Kule keeps your network off it, so only this computer reaches it. Open http://localhost:${l.port}/ in the browser`
              : `${l.process} (pid ${l.pid}) listens on ${l.address === "*" ? "every address" : l.address}, port ${l.port}: other computers on your network can reach it. Open http://localhost:${l.port}/ in the browser`,
          onclick: () => call("open_url", { url: `http://localhost:${l.port}/` }).catch(fail),
        },
        l.reachable && !local(l.address) && !l.shielded ? h("span", { class: "port-exposed", "aria-label": "reachable from the network" }, "⚠ ") : null,
        l.shielded ? h("span", { class: "port-shielded", "aria-label": "kept off the network by Kule" }, "🛡 ") : null,
        `localhost:${l.port}`,
        l.reachable ? h("span", { class: "port-open", "aria-hidden": "true" }, " ↗") : null
      )
    )
  );
  box.hidden = !found.length;
}

start();
