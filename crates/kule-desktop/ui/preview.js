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

/** Cells of a sheet drawn at first, and added at a time after, in whole
 * rows: a wide sheet shows fewer rows at once. */
const PREVIEW_CELLS = 20000;

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
  Object.assign(preview, { task: files.task, path, change: change || null, view: null, key: null, sheet: 0, rows: 0 });
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
  // Not under the person's hands: the file is read again once the box
  // they write in has closed.
  if (key === preview.key || document.querySelector(".preview-bubble")) return;
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
    fileGlyph(baseName(preview.path), false, false),
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
    preview.change !== "deleted"
      ? h(
          "button",
          { class: "button ghost", type: "button", "data-key": "preview-ask-file", title: "Ask the agent about the whole file", onclick: askAboutFile },
          "Ask about it"
        )
      : null,
    // What the Preview shows only in part (a Word file's text, an Excel
    // file's saved values) is seen whole in its own app.
    v && (v.kind === "document" || v.kind === "table") && preview.change !== "deleted" && outside.kinds.has(extOf(preview.path))
      ? openWithAppButton("button ghost")
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
      return previewLines(previewMode(v) === "formatted" && v.formatted ? v.formatted : v.text, v.lang);
    case "markdown":
      return previewMode(v) === "source"
        ? previewLines(v.text, "markdown")
        : h("div", { class: "preview-body preview-md", tabindex: "0" }, ...markdown(v.text, 0));
    case "image":
      return previewImage(v);
    case "table":
      return previewTable(v);
    case "document":
      return previewDocument(v);
    default:
      return h(
        "div",
        { class: "preview-body preview-other" },
        h("p", {}, `${sizeText(v.size)}, ${v.reason}.`),
        h("p", { class: "muted" }, "The Preview shows text, Markdown, CSV, Excel and Word files, and PNG, JPEG, GIF or WebP pictures."),
        // A PDF, a film, a slide deck: its own app shows it.
        preview.change !== "deleted"
          ? h(
              "div",
              { class: "preview-outside" },
              outside.kinds.has(extOf(preview.path)) ? openWithAppButton("button") : null,
              h(
                "button",
                { class: "button ghost", type: "button", "data-key": "preview-show", onclick: () => showOutside(preview.task, preview.path) },
                `Show in ${FILE_MANAGER}`
              )
            )
          : null
      );
  }
}

/** Opens the file in the Preview with its app; waits while the agent is at
 * work, as the daemon does. */
function openWithAppButton(cls) {
  const task = (state.tasks || []).find((t) => t.id === preview.task);
  const why = cannotOpen(preview.path, task);
  return h(
    "button",
    {
      class: cls,
      type: "button",
      "data-key": "preview-open",
      disabled: Boolean(why),
      title: why || "Open it with the app your computer opens it with",
      onclick: () => openWithApp(preview.task, preview.path),
    },
    "Open with its app"
  );
}

/** Text with its line numbers beside it, coloured as `language`. */
function previewLines(text, language) {
  const count = (text.endsWith("\n") ? text.slice(0, -1) : text).split("\n").length;
  const numbers = Array.from({ length: count }, (_, i) => i + 1).join("\n");
  return h(
    "div",
    { class: "preview-body preview-text", tabindex: "0" },
    h("pre", { class: "preview-gutter", "aria-hidden": "true" }, numbers),
    h("pre", { class: "preview-code" }, ...highlighted(text, language))
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
  const step = Math.max(50, Math.min(1000, Math.floor(PREVIEW_CELLS / cols)));
  const upTo = Math.min(sheet.rows.length, preview.rows || step);
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
  const rows = sheet.rows.slice(0, upTo).map((row, r) =>
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
  // A cell picked shows what it holds; dragging, or a shift-click, picks a
  // range, which can be asked about.
  let from = null;
  let dragging = false;
  const cellOf = (e) => {
    const td = e.target.closest?.("td");
    return td && table.contains(td) ? [Number(td.dataset.r), Number(td.dataset.c), td] : null;
  };
  const mark = (to) => {
    const [r0, r1] = [Math.min(from[0], to[0]), Math.max(from[0], to[0])];
    const [c0, c1] = [Math.min(from[1], to[1]), Math.max(from[1], to[1])];
    for (const td of table.querySelectorAll("td.picked")) td.classList.remove("picked");
    for (let r = r0; r <= r1; r++) {
      const tr = table.tBodies[0].rows[r];
      for (let c = c0; c <= c1; c++) tr?.cells[c + 1]?.classList.add("picked");
    }
    return { r0, r1, c0, c1 };
  };
  const done = (to) => {
    const span = mark(to);
    const start = `${columnName(span.c0)}${span.r0 + 1}`;
    const end = `${columnName(span.c1)}${span.r1 + 1}`;
    const values = [];
    for (let r = span.r0; r <= Math.min(span.r1, span.r0 + 9); r++) {
      const row = [];
      for (let c = span.c0; c <= Math.min(span.c1, span.c0 + 9); c++) row.push(sheet.rows[r]?.[c] ?? "");
      values.push(row);
    }
    const csv = /\.(csv|tsv)$/i.test(preview.path);
    offerPlace({
      ref: { kind: "cells", path: preview.path, sheet: csv ? null : sheet.name, range: start === end ? start : `${start}:${end}`, values },
      rect: to[2].getBoundingClientRect(),
    });
  };
  const table = h(
    "table",
    {
      class: "preview-grid",
      onpointerdown: (e) => {
        const at = cellOf(e);
        if (!at || e.button !== 0) return;
        e.preventDefault();
        if (!(e.shiftKey && from)) from = at;
        dragging = true;
        table.querySelector("td.active")?.classList.remove("active");
        at[2].classList.add("active");
        const f = formulas.get(`${at[0]}:${at[1]}`);
        where.textContent = `${columnName(at[1])}${at[0] + 1}`;
        what.textContent = shown(f !== undefined ? `=${f}` : sheet.rows[at[0]]?.[at[1]] ?? "");
        mark(at);
      },
      onpointerover: (e) => {
        // A button let go outside the grid ends the drag there.
        if (dragging && !(e.buttons & 1)) dragging = false;
        const at = dragging && cellOf(e);
        if (at) mark(at);
      },
      onpointerup: (e) => {
        if (!dragging) return;
        dragging = false;
        const at = cellOf(e);
        if (at) done(at);
      },
    },
    h("thead", {}, head),
    h("tbody", {}, ...rows)
  );
  const more = sheet.rows.length - upTo;
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
              preview.rows = upTo + step;
              renderPreview(true);
            },
          },
          more > step
            ? `Show ${step.toLocaleString()} more rows (${more.toLocaleString()} left)`
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
                  preview.rows = 0;
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

/** A picture; dragging over it draws a region to ask about, kept in the
 * picture's own pixels whatever its size on screen. */
function previewImage(v) {
  const img = h("img", { src: `data:${v.mime};base64,${v.data}`, alt: preview.path, width: v.width, height: v.height, draggable: "false" });
  const box = h("div", { class: "preview-region", hidden: true });
  const body = h("div", { class: `preview-body preview-image ${previewMode(v)}`, tabindex: "0" }, img, box);
  let start = null;
  const inPicture = (e) => {
    const r = img.getBoundingClientRect();
    return [Math.min(Math.max(e.clientX, r.left), r.right), Math.min(Math.max(e.clientY, r.top), r.bottom)];
  };
  const draw = (a, b) => {
    const at = body.getBoundingClientRect();
    const [x0, x1] = [Math.min(a[0], b[0]), Math.max(a[0], b[0])];
    const [y0, y1] = [Math.min(a[1], b[1]), Math.max(a[1], b[1])];
    box.hidden = false;
    box.style.left = `${x0 - at.left + body.scrollLeft}px`;
    box.style.top = `${y0 - at.top + body.scrollTop}px`;
    box.style.width = `${x1 - x0}px`;
    box.style.height = `${y1 - y0}px`;
    return { x0, x1, y0, y1 };
  };
  img.addEventListener("pointerdown", (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    img.setPointerCapture(e.pointerId);
    start = inPicture(e);
    draw(start, start);
  });
  img.addEventListener("pointermove", (e) => {
    if (start) draw(start, inPicture(e));
  });
  img.addEventListener("pointerup", (e) => {
    if (!start) return;
    const s = draw(start, inPicture(e));
    start = null;
    const r = img.getBoundingClientRect();
    const scale = v.width / r.width;
    const x = Math.round((s.x0 - r.left) * scale);
    const y = Math.round((s.y0 - r.top) * scale);
    const w = Math.min(Math.round((s.x1 - s.x0) * scale), v.width - x);
    const hgt = Math.min(Math.round((s.y1 - s.y0) * scale), v.height - y);
    if (w < 2 || hgt < 2) {
      box.hidden = true;
      hideAsk();
      return;
    }
    offerPlace({
      ref: { kind: "region", path: preview.path, x, y, w, h: hgt, width: v.width, height: v.height, crop: cropOf(img, x, y, w, hgt) },
      rect: box.getBoundingClientRect(),
    });
  });
  return body;
}

/** The size a region's picture may have, base64; Kule refuses larger. */
const CROP_LIMIT = 200 * 1024;
/** The most pictures of regions one prompt carries. */
const CROPS_LIMIT = 3;

/** The region of `img` as a picture of its own, for an agent that takes
 * pictures: at most 1024 pixels a side, PNG, or JPEG when that is smaller
 * than the limit. Nothing when it does not fit. */
function cropOf(img, x, y, w, hgt) {
  const scale = Math.min(1, 1024 / Math.max(w, hgt));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(w * scale));
  canvas.height = Math.max(1, Math.round(hgt * scale));
  const g = canvas.getContext("2d");
  if (!g) return undefined;
  g.drawImage(img, x, y, w, hgt, 0, 0, canvas.width, canvas.height);
  for (const [mime, quality] of [["image/png"], ["image/jpeg", 0.85], ["image/jpeg", 0.6]]) {
    const url = canvas.toDataURL(mime, quality);
    const data = url.slice(url.indexOf(",") + 1);
    // Base64 of at most CROP_LIMIT bytes.
    if (url.startsWith(`data:${mime};base64,`) && data.length <= Math.ceil(CROP_LIMIT / 3) * 4) return { mime, data };
  }
  return undefined;
}

// --- Asking about a place ----------------------------------------------------
//
// A place picked in the Preview (lines, a section, cells, a region, a
// paragraph) gets a small Ask button beside it; the button opens a box to
// ask the agent now, or to add the place to the prompt as a chip. Chips are
// kept per task until the prompt goes.

/** Chips waiting in each task's prompt: task id → [{ ref }]. */
const promptRefs = new Map();
/** The place the Ask button or the box is for. */
let offered = null;

/** A place as a chip shows it: a mark and a short name. */
function placeLabel(ref) {
  const base = baseName(ref.path);
  const lines = (a, b) => (a === b ? `${a}` : `${a}–${b}`);
  switch (ref.kind) {
    case "lines":
      return ["≡", `${base}:${lines(ref.start, ref.end)}`];
    case "section":
      return ["§", `${base}:${lines(ref.start, ref.end)}${ref.trail.length ? ` · ${ref.trail[ref.trail.length - 1]}` : ""}`];
    case "quote":
      return ["❝", `${base} “${ref.quote.length > 24 ? `${ref.quote.slice(0, 24)}…` : ref.quote}”`];
    case "cells":
      return ["▦", `${base} ${ref.sheet ? `${ref.sheet}!` : ""}${ref.range}`];
    case "region":
      return ["▭", `${base} ${ref.w}×${ref.h}`];
    case "block":
      return ["¶", `${base} ¶${ref.index}`];
    default:
      return ["↗", base];
  }
}

/** What was picked, for the box to quote back. */
const placeQuote = (ref) => ref.quote || (ref.values ? ref.values.map((r) => r.join("  ")).join(" / ") : "");

/** The place the window's text selection is in the Preview, if any. */
function selectedPlace() {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || !sel.rangeCount || !preview.view) return null;
  const range = sel.getRangeAt(0);
  const body = $("preview").querySelector(".preview-body");
  if (!body || !body.contains(range.commonAncestorContainer)) return null;
  const quote = sel.toString().trim();
  if (!quote) return null;
  const rects = range.getClientRects();
  const rect = rects.length ? rects[rects.length - 1] : range.getBoundingClientRect();
  const el = (node) => (node.nodeType === Node.ELEMENT_NODE ? node : node.parentElement);
  const v = preview.view;
  const path = preview.path;
  const code = body.querySelector(".preview-code");
  if (code && code.contains(range.startContainer) && code.contains(range.endContainer)) {
    // A formatted view's lines are not the file's.
    if (v.kind === "text" && v.formatted && previewMode(v) === "formatted") return { ref: { kind: "quote", path, quote }, rect };
    // Offsets in the text, and the lines they fall on.
    const offset = (node, at) => {
      const r = document.createRange();
      r.selectNodeContents(code);
      r.setEnd(node, at);
      return r.toString().length;
    };
    const text = code.textContent;
    const a = offset(range.startContainer, range.startOffset);
    const b = offset(range.endContainer, range.endOffset);
    const lineAt = (i) => text.slice(0, i).split("\n").length;
    return { ref: { kind: "lines", path, start: lineAt(a), end: lineAt(Math.max(a, b - 1)), quote }, rect };
  }
  const first = el(range.startContainer)?.closest("[data-line], [data-block]");
  const last = el(range.endContainer)?.closest("[data-line], [data-block]");
  if (!first || !body.contains(first)) return null;
  if (first.dataset.block) {
    // The heading the block is under.
    let heading = null;
    for (const hd of body.querySelectorAll(".md-h[data-block]")) {
      if (hd === first || first.compareDocumentPosition(hd) & Node.DOCUMENT_POSITION_FOLLOWING) break;
      heading = hd.textContent;
    }
    return { ref: { kind: "block", path, index: Number(first.dataset.block), heading, quote }, rect };
  }
  let start = Number(first.dataset.line);
  let end = Math.max(start, Number((last && body.contains(last) ? last : first).dataset.lineEnd));
  // Narrowed to the one source line the picked text is on, when it is on one.
  const source = v.text.split("\n");
  const probe = quote.split("\n")[0].slice(0, 40);
  const on = [];
  for (let n = start; n <= end; n++) if (source[n - 1]?.includes(probe)) on.push(n);
  if (on.length === 1 && !quote.includes("\n")) start = end = on[0];
  // The headings the place is under, outermost first.
  let trail = [];
  for (const hd of body.querySelectorAll(".md-h[data-level]")) {
    if (first.compareDocumentPosition(hd) & Node.DOCUMENT_POSITION_FOLLOWING && hd !== first) break;
    const level = Number(hd.dataset.level);
    trail = trail.filter((t) => t.level < level);
    trail.push({ level, text: hd.textContent });
  }
  return { ref: { kind: "section", path, start, end, trail: trail.map((t) => t.text), quote }, rect };
}

/** Puts the Ask button beside a picked place. */
function offerPlace(place) {
  closeAsk();
  offered = place;
  const button = h(
    "button",
    {
      class: "preview-ask",
      type: "button",
      title: `Ask the agent about this (${shortcutText("preview.ask")})`,
      "data-key": "preview-ask",
      onpointerdown: (e) => e.preventDefault(),
      onclick: () => openAsk(place),
    },
    "Ask"
  );
  placeNear(button, place.rect);
}

/** The shortcut of command `id`, as the window writes it. */
function shortcutText(id) {
  const command = commandById(id);
  const combo = command ? bindingOf(command) : "";
  return combo ? formatCombo(combo) : "no shortcut";
}

/** Places `el` in the Preview's body under (or over) `rect`. */
function placeNear(el, rect) {
  const body = $("preview").querySelector(".preview-body") || $("preview");
  body.append(el);
  const at = body.getBoundingClientRect();
  const left = Math.min(Math.max(rect.right - at.left + body.scrollLeft - 20, 8), body.scrollLeft + body.clientWidth - el.offsetWidth - 8);
  let top = rect.bottom - at.top + body.scrollTop + 6;
  if (rect.bottom + el.offsetHeight + 12 > at.bottom && rect.top - el.offsetHeight - 6 > at.top) {
    top = rect.top - at.top + body.scrollTop - el.offsetHeight - 6;
  }
  el.style.left = `${Math.max(left, 8)}px`;
  el.style.top = `${Math.max(top, body.scrollTop + 8)}px`;
}

function hideAsk() {
  document.querySelector(".preview-ask")?.remove();
  if (!document.querySelector(".preview-bubble")) offered = null;
}

function closeAsk() {
  document.querySelector(".preview-ask")?.remove();
  document.querySelector(".preview-bubble")?.remove();
  offered = null;
}

/** The box for a place: what it is, a field to write in, Ask now and Add
 * to prompt. */
function openAsk(place) {
  const task = currentTask();
  if (!task) return;
  closeAsk();
  offered = place;
  const [mark, label] = placeLabel(place.ref);
  const quote = placeQuote(place.ref);
  const field = h("textarea", {
    class: "preview-bubble-text",
    rows: 2,
    "aria-label": `What to ask about ${label}`,
    placeholder: "What should the agent do here?",
    "data-key": "preview-bubble-text",
  });
  const busy = task.running;
  const now = h("button", { class: "button primary", type: "button", disabled: busy || !state.agents.length, onclick: () => askNow(place, field.value) }, "Ask now");
  const add = h("button", { class: "button", type: "button", onclick: () => addToPrompt(place, field.value) }, "Add to prompt");
  const bubble = h(
    "div",
    {
      class: "preview-bubble",
      role: "dialog",
      "aria-label": `Ask about ${label}`,
      onkeydown: (e) => {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          closeAsk();
          $("preview").querySelector(".preview-body")?.focus();
        } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
          e.preventDefault();
          if (!now.disabled) now.click();
        }
      },
    },
    h("div", { class: "preview-bubble-ref" }, h("span", { class: "chip-mark", "aria-hidden": "true" }, mark), label),
    quote ? h("p", { class: "preview-bubble-quote" }, quote.length > 160 ? `${quote.slice(0, 160)}…` : quote) : null,
    field,
    h(
      "div",
      { class: "preview-bubble-actions" },
      now,
      add,
      h("span", { class: "preview-bubble-hint" }, busy ? "The agent is at work; add it to the prompt." : `${PLATFORM === "mac" ? "⌘" : "Ctrl+"}↵ asks · Esc closes`)
    )
  );
  placeNear(bubble, place.rect);
  field.focus();
}

async function askNow(place, text) {
  const task = currentTask();
  const prompt = text.trim();
  if (!task) return;
  if (!prompt) {
    toast("Write what the agent should do here first.");
    document.querySelector(".preview-bubble-text")?.focus();
    return;
  }
  try {
    await call("start_run", { task: task.id, agent: $("agent").value, prompt, refs: [place.ref] });
    closeAsk();
    window.getSelection()?.removeAllRanges();
    toast(`Asked ${$("agent").value} about ${placeLabel(place.ref)[1]}`);
    await refresh();
  } catch (err) {
    fail(err);
  }
}

function addToPrompt(place, text) {
  const task = currentTask();
  if (!task) return;
  const chips = promptRefs.get(task.id) || [];
  const key = JSON.stringify(place.ref);
  if (!chips.some((c) => JSON.stringify(c.ref) === key)) {
    if (chips.length >= 20) {
      toast("A prompt names at most 20 places.", "error");
      return;
    }
    let ref = place.ref;
    if (ref.crop && chips.filter((c) => c.ref.crop).length >= CROPS_LIMIT) {
      ref = { ...ref, crop: undefined };
      toast(`Only ${CROPS_LIMIT} pictures go with one prompt; this region goes as where it is.`);
    }
    chips.push({ ref });
  }
  promptRefs.set(task.id, chips);
  const words = text.trim();
  if (words) {
    const prompt = $("prompt");
    prompt.value = prompt.value.trim() ? `${prompt.value.trimEnd()}\n${words}` : words;
    fitPrompt();
  }
  closeAsk();
  window.getSelection()?.removeAllRanges();
  renderPromptRefs(task);
  toast(`${placeLabel(place.ref)[1]} is in the prompt`);
}

/** A place as a chip; `remove` gives it a button that takes it away. */
function placeChip(ref, remove) {
  const [mark, label] = placeLabel(ref);
  return h(
    "span",
    { class: "place-chip", title: `${ref.path}${placeQuote(ref) ? `: ${placeQuote(ref)}` : ""}` },
    h("span", { class: "chip-mark", "aria-hidden": "true" }, mark),
    label,
    remove ? h("button", { class: "place-chip-remove", type: "button", "aria-label": `Take ${label} out of the prompt`, "data-key": `chip:${label}`, onclick: remove }, "×") : null
  );
}

/** The chips over the task's prompt. */
function renderPromptRefs(task) {
  const box = $("prompt-refs");
  if (!box) return;
  const chips = (task && promptRefs.get(task.id)) || [];
  replace(
    box,
    ...chips.map((c, i) =>
      placeChip(c.ref, () => {
        chips.splice(i, 1);
        renderPromptRefs(task);
        $("prompt").focus();
      })
    )
  );
  box.hidden = !chips.length;
}

/** Whatever is picked in the Preview gets the Ask button. */
function watchSelection() {
  const root = $("preview");
  const offer = () =>
    setTimeout(() => {
      if (document.querySelector(".preview-bubble")) return;
      const place = selectedPlace();
      if (place) offerPlace(place);
      else if (offered && !["cells", "region"].includes(offered.ref.kind)) hideAsk();
    }, 0);
  root.addEventListener("mouseup", offer);
  root.addEventListener("keyup", (e) => {
    if (e.shiftKey || e.key === "Shift") offer();
  });
  // A click elsewhere closes the box, the way a menu closes.
  document.addEventListener("pointerdown", (e) => {
    const bubble = document.querySelector(".preview-bubble");
    if (bubble && !bubble.contains(e.target)) closeAsk();
  });
}

/** Asks about what is picked, or about the whole file when nothing is. */
function askHere() {
  if (!preview.path || !isShown("preview")) return toast("Open a file in the Preview first.");
  const place = selectedPlace() || (offered && !document.querySelector(".preview-bubble") ? offered : null);
  if (place) return openAsk(place);
  askAboutFile();
}

function askAboutFile() {
  const head = $("preview").querySelector("[data-key='preview-ask-file']");
  const rect = head ? head.getBoundingClientRect() : $("preview").getBoundingClientRect();
  openAsk({ ref: { kind: "file", path: preview.path }, rect });
}
