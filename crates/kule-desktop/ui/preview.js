// The Preview: a file of the task's copy, large, as what it is: text with
// its line numbers, Markdown laid out, a picture. Built with DOM calls and
// textContent like the rest of the window; nothing in a file is parsed as
// HTML, and a picture is shown only when the daemon found it to be one.
// Loaded before app.js, whose helpers (h, replace, call, markdown, …) it
// uses once the window runs.
"use strict";

const preview = {
  task: null,
  path: null,
  /** created, modified or deleted, from the task's review. */
  change: null,
  view: null,
  /** What the view on screen was read for; another key reads it again. */
  key: null,
  token: 0,
  /** How each kind of file is shown, as last chosen. */
  mode: {},
};

const PREVIEW_MODES = {
  markdown: [
    ["rendered", "Rendered"],
    ["source", "Source"],
  ],
  text: [
    ["text", "As written"],
    ["formatted", "Formatted"],
  ],
  image: [
    ["fit", "Fit"],
    ["actual", "100%"],
  ],
};

const previewMode = (v) => preview.mode[v.kind] || PREVIEW_MODES[v.kind]?.[0][0];

function sizeText(bytes) {
  if (bytes < 1024) return `${bytes} bytes`;
  const units = ["KB", "MB", "GB"];
  let n = bytes / 1024;
  let u = 0;
  while (n >= 1024 && u < units.length - 1) {
    n /= 1024;
    u++;
  }
  return `${n < 10 ? n.toFixed(1) : Math.round(n)} ${units[u]}`;
}

function kindText(v) {
  switch (v.kind) {
    case "text":
      return v.lang || "Text";
    case "markdown":
      return "Markdown";
    case "image":
      return `${v.mime.slice(6).toUpperCase()} · ${v.width} × ${v.height}`;
    default:
      return "";
  }
}

/** Shows file `path` of the task Files is on, and brings the Preview up. */
function previewFile(path, change) {
  Object.assign(preview, { task: files.task, path, change: change || null, view: null, key: null });
  activatePanel("preview");
}

/** Reads the file again when what the Preview shows may be out of date:
 * another file, or the task's changes moved on. */
async function loadPreview(task) {
  if (preview.task !== task.id) Object.assign(preview, { task: task.id, path: null, change: null, view: null, key: null });
  const body = $("preview");
  if (!preview.path) {
    preview.key = null;
    replace(
      body,
      h(
        "div",
        { class: "preview-empty" },
        h("p", {}, "Choose a file in Files to see it here."),
        h("button", { class: "button", type: "button", onclick: () => revealPanel("files") }, "Show Files")
      )
    );
    return;
  }
  if (files.task === task.id && files.changes.size) preview.change = files.changes.get(preview.path) || null;
  if (preview.change === "deleted") {
    preview.key = null;
    replace(body, previewHead(null), h("p", { class: "preview-note" }, "The task deleted this file; Review shows what it was."));
    return;
  }
  const key = [task.id, preview.path, task.changes, task.running].join("\n");
  if (key === preview.key) return;
  preview.key = key;
  const token = ++preview.token;
  let view;
  try {
    view = await call("view_file", { task: task.id, path: preview.path });
  } catch (err) {
    if (token !== preview.token) return;
    preview.key = null;
    replace(body, previewHead(null), h("p", { class: "preview-note" }, String(err)));
    return;
  }
  if (token !== preview.token || task.id !== state.current) return;
  const again = preview.view && preview.view.kind === view.kind;
  preview.view = view;
  renderPreview(again);
}

/** Draws the view; `keepScroll` keeps the place in a file read again. */
function renderPreview(keepScroll) {
  const v = preview.view;
  const body = $("preview");
  const before = body.querySelector(".preview-body");
  const at = keepScroll && before ? [before.scrollTop, before.scrollLeft] : null;
  replace(
    body,
    previewHead(v),
    previewBody(v),
    (v.kind === "text" || v.kind === "markdown") && v.cut
      ? h("p", { class: "preview-note" }, `Only the first 1 MB of ${sizeText(v.size)} is shown.`)
      : null
  );
  const after = body.querySelector(".preview-body");
  if (at && after) [after.scrollTop, after.scrollLeft] = at;
}

function previewHead(v) {
  const modes = v && (v.kind !== "text" || v.formatted) ? PREVIEW_MODES[v.kind] : null;
  const now = v && previewMode(v);
  return h(
    "div",
    { class: "preview-head" },
    h("span", { class: "path preview-path", title: preview.path }, preview.path),
    v ? h("span", { class: "preview-meta" }, [sizeText(v.size), kindText(v)].filter(Boolean).join(" · ")) : null,
    preview.change ? h("span", { class: `preview-change ${preview.change}` }, preview.change) : null,
    h("span", { class: "preview-gap" }),
    modes
      ? h(
          "div",
          { class: "preview-modes", role: "group", "aria-label": "Show the file" },
          ...modes.map(([mode, label]) =>
            h(
              "button",
              {
                class: `preview-mode${mode === now ? " on" : ""}`,
                type: "button",
                "aria-pressed": String(mode === now),
                "data-key": `preview-mode:${mode}`,
                onclick: () => {
                  preview.mode[v.kind] = mode;
                  renderPreview(false);
                },
              },
              label
            )
          )
        )
      : null,
    preview.change
      ? h("button", { class: "button ghost", type: "button", "data-key": "preview-changes", onclick: () => revealPanel("review") }, "Changes")
      : null,
    h(
      "button",
      {
        class: "button ghost",
        type: "button",
        "aria-label": "Read the file again",
        title: "Read the file again",
        "data-key": "preview-reload",
        onclick: () => {
          preview.key = null;
          const task = currentTask();
          if (task) loadPreview(task);
        },
      },
      "↻"
    )
  );
}

function previewBody(v) {
  switch (v.kind) {
    case "text":
      return previewLines(previewMode(v) === "formatted" && v.formatted ? v.formatted : v.text);
    case "markdown":
      return previewMode(v) === "source"
        ? previewLines(v.text)
        : h("div", { class: "preview-body preview-md", tabindex: "0" }, ...markdown(v.text, 0));
    case "image":
      return h(
        "div",
        { class: `preview-body preview-image ${previewMode(v)}`, tabindex: "0" },
        h("img", { src: `data:${v.mime};base64,${v.data}`, alt: preview.path, width: v.width, height: v.height })
      );
    default:
      return h(
        "div",
        { class: "preview-body preview-other" },
        h("p", {}, `${sizeText(v.size)}, ${v.reason}.`),
        h("p", { class: "muted" }, "The Preview shows text, Markdown and PNG, JPEG, GIF or WebP pictures.")
      );
  }
}

/** Text with its line numbers beside it. */
function previewLines(text) {
  const count = (text.endsWith("\n") ? text.slice(0, -1) : text).split("\n").length;
  const numbers = Array.from({ length: count }, (_, i) => i + 1).join("\n");
  return h(
    "div",
    { class: "preview-body preview-text", tabindex: "0" },
    h("pre", { class: "preview-gutter", "aria-hidden": "true" }, numbers),
    h("pre", { class: "preview-code" }, text)
  );
}
