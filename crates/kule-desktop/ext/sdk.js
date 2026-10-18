// Kule's extension SDK: `kule`, in every frame of an extension.
//
// The frame is sandboxed (an opaque origin, no network, no storage of its
// own). Its only way out is the MessagePort Kule hands it once, when it
// loads; everything below is a request over that port, which Kule answers
// or refuses by the permissions the person approved.
"use strict";

(function () {
  const pending = new Map();
  const handlers = new Map();
  const commands = new Map();
  let port = null;
  let next = 1;
  const queue = [];
  let ready;
  const whenReady = new Promise((resolve) => (ready = resolve));

  function send(message) {
    if (port) port.postMessage(message);
    else queue.push(message);
  }

  function request(method, params) {
    return new Promise((resolve, reject) => {
      const id = next++;
      pending.set(id, { resolve, reject });
      send({ id, method, params: params === undefined ? null : params });
    });
  }

  function emit(name, data) {
    for (const handler of handlers.get(name) || []) {
      try {
        handler(data);
      } catch (e) {
        console.error(e);
      }
    }
  }

  function onMessage(event) {
    const m = event.data;
    if (!m || typeof m !== "object") return;
    if (typeof m.id === "number" && pending.has(m.id)) {
      const { resolve, reject } = pending.get(m.id);
      pending.delete(m.id);
      if (typeof m.error === "string") reject(new Error(m.error));
      else resolve(m.result);
      return;
    }
    if (typeof m.command === "string") {
      const run = commands.get(m.command);
      if (run) Promise.resolve().then(() => run(m.args)).catch((e) => console.error(e));
      return;
    }
    if (typeof m.event === "string") {
      if (m.event === "theme") applyTheme(m.data);
      emit(m.event, m.data);
    }
  }

  // Kule's colours as CSS variables (--kule-bg, --kule-text, ...), so a
  // panel can look like the rest of the window.
  function applyTheme(theme) {
    if (!theme || typeof theme !== "object") return;
    const root = document.documentElement;
    for (const [part, value] of Object.entries(theme.colors || {})) {
      if (/^[a-z-]{1,24}$/.test(part) && /^#[0-9a-f]{6}$/i.test(value)) {
        root.style.setProperty("--kule-" + part, value);
      }
    }
    if (theme.scheme === "light" || theme.scheme === "dark") root.dataset.scheme = theme.scheme;
  }

  // The first message from the window carries the port; later ones are
  // ignored, so nothing else can hand the frame another.
  window.addEventListener("message", function connect(event) {
    if (port || event.source !== window.parent) return;
    const m = event.data;
    if (!m || m.kule !== "connect" || !event.ports || event.ports.length !== 1) return;
    window.removeEventListener("message", connect);
    port = event.ports[0];
    port.onmessage = onMessage;
    for (const message of queue.splice(0)) port.postMessage(message);
    kule.extension = m.extension;
    kule.context = m.context || {};
    applyTheme(m.theme);
    ready();
  });

  // What the extension logs, and what goes wrong in it, goes to the window,
  // which shows it in Settings → Extensions: a frame has no console of its
  // own to look at.
  function shown(value) {
    if (value instanceof Error) return value.stack || String(value);
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }
  function report(level, values) {
    send({ log: { level, text: values.map(shown).join(" ").slice(0, 2000) } });
  }
  for (const level of ["log", "info", "warn", "error"]) {
    const original = console[level].bind(console);
    console[level] = (...values) => {
      original(...values);
      report(level, values);
    };
  }
  window.addEventListener("error", (e) => {
    const where = e.filename ? ` (${e.filename.split("/").pop()}:${e.lineno})` : "";
    report("error", [`${e.message}${where}`]);
  });
  window.addEventListener("unhandledrejection", (e) => {
    report("error", ["Unhandled rejection:", e.reason]);
  });

  // Keys with a modifier go to the window, so its shortcuts (the command
  // palette, moving between panels) work while a panel has the focus.
  document.addEventListener(
    "keydown",
    (e) => {
      const fkey = /^F([1-9]|1[0-9])$/.test(e.key);
      if (!(e.metaKey || e.ctrlKey || e.altKey || fkey)) return;
      send({
        key: {
          key: e.key,
          code: e.code,
          metaKey: e.metaKey,
          ctrlKey: e.ctrlKey,
          altKey: e.altKey,
          shiftKey: e.shiftKey,
        },
      });
    },
    true,
  );

  const kule = {
    /** Resolves once Kule has connected the frame. */
    ready: whenReady,
    /** { id, name, version } of this extension. */
    extension: null,
    /** Where this frame is: { panel, task } for a panel, {} for main. */
    context: {},
    commands: {
      /** Runs `run(args)` when the person picks the command `id` from the manifest. */
      register(id, run) {
        commands.set(String(id), run);
      },
      /** Runs one of this extension's commands, or one of Kule's that only moves the view. */
      execute(id, args) {
        return request("commands.execute", { id: String(id), args });
      },
    },
    events: {
      /**
       * "run" (an agent's events), "task" (created, committed, rolled back,
       * undone, removed), "questions", "task-selected", "theme", "config".
       */
      on(name, handler) {
        const list = handlers.get(name) || [];
        list.push(handler);
        handlers.set(name, list);
        return () => handlers.set(name, (handlers.get(name) || []).filter((h) => h !== handler));
      },
    },
    tasks: {
      list: () => request("tasks.list"),
      get: (task) => request("tasks.get", { task }),
    },
    review: {
      get: (task) => request("review.get", { task }),
      /** notes: [{ path?, level: "info" | "warn", text }]; replaces this extension's notes on the task. */
      annotate: (task, notes) => request("review.annotate", { task, notes }),
    },
    files: {
      list: (task, path) => request("files.list", { task, path: path || "" }),
      read: (task, path) => request("files.read", { task, path }),
    },
    storage: {
      get: () => request("storage.get"),
      set: (value) => request("storage.set", { value }),
    },
    config: {
      get: () => request("config.get"),
    },
    ui: {
      notify: (title, body) => request("ui.notify", { title: String(title), body: String(body || "") }),
    },
    net: {
      /** https only, to the hosts in the manifest. Resolves to { status, headers, body }. */
      fetch: (url, init) => request("net.fetch", { url: String(url), init: init || {} }),
    },
  };

  Object.freeze(kule.commands);
  Object.freeze(kule.events);
  window.kule = kule;
})();
