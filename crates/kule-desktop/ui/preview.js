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
  /** The sheet in front, and how many of its rows are drawn. */
  sheet: 0,
  rows: 0,
};

/** Rows of a sheet drawn at first, and added at a time after. */
const PREVIEW_ROWS = 1000;

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
    case "table":
      return v.sheets.length > 1 ? `${v.sheets.length} sheets` : "Table";
    case "document":
      return "Word document";
    default:
      return "";
  }
}

/** Shows file `path` of the task Files is on, and brings the Preview up. */
function previewFile(path, change) {
  Object.assign(preview, { task: files.task, path, change: change || null, view: null, key: null, sheet: 0, rows: PREVIEW_ROWS });
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
    case "table":
      return previewTable(v);
    case "document":
      return previewDocument(v);
    default:
      return h(
        "div",
        { class: "preview-body preview-other" },
        h("p", {}, `${sizeText(v.size)}, ${v.reason}.`),
        h("p", { class: "muted" }, "The Preview shows text, Markdown, CSV, Excel and Word files, and PNG, JPEG, GIF or WebP pictures.")
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

/** Column `n` (from 0) as a spreadsheet names it: A, …, Z, AA. */
function columnName(n) {
  let name = "";
  for (n += 1; n > 0; n = Math.floor((n - 1) / 26)) name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
  return name;
}

const NUMBER = /^[-+]?[\d.,\s]*\d[\d.,]*\s*%?$|^[-+]?\d[\d.,]*[eE][-+]?\d+$/;

/** A sheet as a grid from A1, its sheets as tabs under it, and a bar over
 * it that shows the picked cell's value or formula. */
function previewTable(v) {
  const at = Math.min(preview.sheet, v.sheets.length - 1);
  const sheet = v.sheets[at];
  const cols = sheet.rows.reduce((n, r) => Math.max(n, r.length), 1);
  const shown = Math.min(sheet.rows.length, preview.rows || PREVIEW_ROWS);
  const formulas = new Map((sheet.formulas || []).map((f) => [`${f.row}:${f.col}`, f.text]));
  const where = h("span", { class: "preview-fx-cell" }, "");
  const what = h("span", { class: "preview-fx-text" }, "Pick a cell to see what it holds.");
  const fx = h("div", { class: "preview-fx", "aria-live": "polite" }, where, what);
  const head = h(
    "tr",
    {},
    h("th", { class: "preview-corner", "aria-hidden": "true" }),
    ...Array.from({ length: cols }, (_, c) => h("th", { scope: "col" }, columnName(c)))
  );
  const rows = sheet.rows.slice(0, shown).map((row, r) =>
    h(
      "tr",
      {},
      h("th", { scope: "row" }, String(r + 1)),
      ...Array.from({ length: cols }, (_, c) => {
        const text = row[c] ?? "";
        const formula = formulas.has(`${r}:${c}`);
        return h("td", { class: `${NUMBER.test(text) ? "num" : ""}${formula ? " formula" : ""}`, "data-r": r, "data-c": c }, text);
      })
    )
  );
  const table = h(
    "table",
    {
      class: "preview-grid",
      onclick: (e) => {
        const td = e.target.closest("td");
        if (!td) return;
        table.querySelector("td.active")?.classList.remove("active");
        td.classList.add("active");
        const r = Number(td.dataset.r);
        const c = Number(td.dataset.c);
        where.textContent = `${columnName(c)}${r + 1}`;
        const f = formulas.get(`${r}:${c}`);
        what.textContent = f !== undefined ? `=${f}` : sheet.rows[r]?.[c] ?? "";
      },
    },
    h("thead", {}, head),
    h("tbody", {}, ...rows)
  );
  const more = sheet.rows.length - shown;
  const grid = h(
    "div",
    { class: "preview-body preview-table", tabindex: "0", "aria-label": `${sheet.name}, ${sheet.rows.length} rows` },
    table,
    more > 0
      ? h(
          "button",
          {
            class: "button ghost preview-more",
            type: "button",
            onclick: () => {
              preview.rows = shown + PREVIEW_ROWS;
              renderPreview(true);
            },
          },
          more > PREVIEW_ROWS
            ? `Show ${PREVIEW_ROWS.toLocaleString()} more rows (${more.toLocaleString()} left)`
            : `Show the other ${more.toLocaleString()} rows`
        )
      : null
  );
  const tabs =
    v.sheets.length > 1
      ? h(
          "div",
          { class: "preview-sheets", role: "tablist", "aria-label": "Sheets" },
          ...v.sheets.map((s, i) =>
            h(
              "button",
              {
                class: `preview-sheet${i === at ? " on" : ""}`,
                type: "button",
                role: "tab",
                "aria-selected": String(i === at),
                "data-key": `preview-sheet:${i}`,
                onclick: () => {
                  preview.sheet = i;
                  preview.rows = PREVIEW_ROWS;
                  renderPreview(false);
                },
              },
              s.name
            )
          )
        )
      : null;
  const cut = sheet.cut ? h("p", { class: "preview-note" }, "The sheet has more rows or columns than are shown.") : null;
  return [fx, grid, cut, tabs];
}

/** A Word document's text: each block carries its number (data-block,
 * from 1), which is how a place in it is named. */
function previewDocument(v) {
  const blocks = v.blocks.map((b, i) => {
    const n = { "data-block": i + 1 };
    switch (b.kind) {
      case "heading":
        return h(`h${Math.min(b.level + 2, 6)}`, { ...n, class: "md-h", "data-level": b.level }, b.text);
      case "list_item":
        return h(
          "div",
          { ...n, class: `preview-item depth-${Math.min(b.level, 4)}` },
          h("span", { class: "preview-marker", "aria-hidden": "true" }, b.marker),
          h("span", {}, b.text)
        );
      case "table":
        return h(
          "div",
          { ...n, class: "md-table" },
          h("table", {}, h("tbody", {}, ...b.rows.map((row, r) => h("tr", {}, ...row.map((cell) => h(r === 0 ? "th" : "td", {}, cell))))))
        );
      default:
        return h("p", n, b.text);
    }
  });
  return [
    h(
      "div",
      { class: "preview-body preview-md preview-doc", tabindex: "0" },
      ...(blocks.length ? blocks : [h("p", { class: "muted" }, "The document has no text.")])
    ),
    v.cut ? h("p", { class: "preview-note" }, "The document goes on past what is shown.") : null,
  ];
}
