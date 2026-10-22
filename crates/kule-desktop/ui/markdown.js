// Kule's Markdown: an agent's messages in the stream and .md files in the
// Preview, laid out as nodes. A classic script like the window's others
// (it uses `h` from app.js and `highlighted` from highlight.js when called);
// Node loads it too, for the tests under tests/.
"use strict";

// An agent answers in Markdown. It is laid out here with DOM calls alone:
// the text comes from the agent, so nothing of it is ever parsed as HTML,
// and links are shown, not followed (the window has no business navigating).
//
// Nothing here backtracks over a line's length: each part is found by
// scanning forward, so the time taken grows with the length of the text
// and no faster, whatever the text holds.

/** The most characters bold, italics, or a link's text may hold; an opener
 * whose closer lies farther is shown as it is. */
const INLINE_SPAN_MAX = 1000;
/** The most characters a link's address may hold. */
const LINK_URL_MAX = 4096;
/** How deep quotes nest; a `>` past it is shown as text. */
const MAX_QUOTE_DEPTH = 32;

const isWordChar = (c) => c !== undefined && /\w/.test(c);
const isSpaceChar = (c) => c !== undefined && /\s/.test(c);

/**
 * Finds the next `needle` (a character, or a /g pattern) at or after a
 * place in `text`. Each answer is kept: asked again from a later place
 * before it, the same answer holds, so scanning forward reads the text
 * once however many openers ask for the same closer.
 */
function nextFinder(text) {
  const kept = new Map();
  return (needle, from) => {
    const hit = kept.get(needle);
    if (hit && hit.from <= from && (hit.at === -1 || from <= hit.at)) return hit.at;
    let at;
    if (typeof needle === "string") at = text.indexOf(needle, from);
    else {
      needle.lastIndex = from;
      const m = needle.exec(text);
      at = m ? m.index : -1;
    }
    kept.set(needle, { from, at });
    return at;
  };
}

/** Where each run of backticks of each length starts, in order, so a
 * code span's closer (the next run of its opener's length) is found
 * without searching. */
function backtickRuns(text) {
  const runs = new Map();
  for (let i = text.indexOf("`"); i !== -1; ) {
    let end = i;
    while (text[end] === "`") end++;
    const n = end - i;
    if (!runs.has(n)) runs.set(n, { starts: [], next: 0 });
    runs.get(n).starts.push(i);
    i = text.indexOf("`", end);
  }
  return runs;
}

/** Inline Markdown: code spans, bold, italics and links, as nodes. */
function inline(text) {
  const nodes = [];
  if (!/[`*_[]/.test(text)) {
    if (text) nodes.push(text);
    return nodes;
  }
  const next = nextFinder(text);
  const space = /\s/g;
  let runs = null;
  let at = 0;
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    let span = null;
    let end = i + 1;
    if (c === "`") {
      // A run of n backticks opens a code span that the next run of
      // exactly n closes; without one, the run is text.
      let close = i;
      while (text[close] === "`") close++;
      const n = close - i;
      runs ??= backtickRuns(text);
      const same = runs.get(n);
      while (same.next < same.starts.length && same.starts[same.next] <= i) same.next++;
      if (same.next < same.starts.length) {
        const to = same.starts[same.next];
        span = [h("code", {}, text.slice(i + n, to).trim())];
        end = to + n;
      } else {
        end = close;
      }
    } else if (c === "*" && text[i + 1] === "*") {
      const j = next("*", i + 2);
      if (j > i + 2 && j - i <= INLINE_SPAN_MAX && text[j + 1] === "*") {
        span = [h("strong", {}, ...inline(text.slice(i + 2, j)))];
        end = j + 2;
      }
    } else if (c === "*" && i + 1 < text.length && !isSpaceChar(text[i + 1])) {
      const j = next("*", i + 1);
      if (j !== -1 && j - i <= INLINE_SPAN_MAX) {
        span = [h("em", {}, ...inline(text.slice(i + 1, j)))];
        end = j + 1;
      }
    } else if (c === "_" && !isWordChar(text[i - 1])) {
      if (text[i + 1] === "_") {
        const j = next("_", i + 2);
        if (j > i + 2 && j - i <= INLINE_SPAN_MAX && text[j + 1] === "_" && !isWordChar(text[j + 2])) {
          span = [h("strong", {}, ...inline(text.slice(i + 2, j)))];
          end = j + 2;
        }
      } else if (i + 1 < text.length && !isSpaceChar(text[i + 1])) {
        const j = next("_", i + 1);
        if (j !== -1 && j - i <= INLINE_SPAN_MAX && !isWordChar(text[j + 1])) {
          span = [h("em", {}, ...inline(text.slice(i + 1, j)))];
          end = j + 1;
        }
      }
    } else if (c === "[") {
      // [text](address): the address runs to a `)` with no space before it.
      const j = next("]", i + 1);
      if (j > i + 1 && j - i <= INLINE_SPAN_MAX && text[j + 1] === "(") {
        const k = next(")", j + 2);
        const gap = next(space, j + 2);
        if (k > j + 2 && k - j <= LINK_URL_MAX && (gap === -1 || k < gap)) {
          const url = text.slice(j + 2, k);
          span = [h("span", { class: "md-link", title: url }, ...inline(text.slice(i + 1, j))), h("span", { class: "md-url" }, ` (${url})`)];
          end = k + 1;
        }
      }
    }
    if (span) {
      if (i > at) nodes.push(text.slice(at, i));
      nodes.push(...span);
      at = end;
    }
    i = end;
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

/** Whether `line` is a table's rule under its head, as `|:--|--:|`: cells
 * of two or more dashes, a colon at either end, pipes between. */
function tableRule(line) {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|")) row = row.slice(0, -1);
  return row.split("|").every((c) => /^:?-{2,}:?$/.test(c.trim()));
}

/** Characters a line holds that a pattern's `.` does not match. */
const LINE_BREAKS = /[\n\r\u2028\u2029]/;

/** A list item's [line, indent, marker, text], or null. */
function listItemOf(line) {
  const m = /^(\s*)([-*+]|\d{1,9}[.)])\s/.exec(line);
  if (!m) return null;
  const rest = line.slice(m[0].length).trimStart();
  return LINE_BREAKS.test(rest) ? null : [line, m[1], m[2], rest];
}

/** A heading's [line, hashes, text] (closing hashes taken off), or null. */
function headingOf(line) {
  const m = /^\s{0,3}(#{1,6})\s/.exec(line);
  if (!m) return null;
  let rest = line.slice(m[0].length).trimStart().trimEnd();
  let cut = rest.length;
  while (cut > 0 && rest[cut - 1] === "#") cut--;
  rest = rest.slice(0, cut).trimEnd();
  return LINE_BREAKS.test(rest) ? null : [line, m[1], rest];
}

/**
 * Blocks laid out from Markdown `text`: paragraphs, headings, lists,
 * quotes, code and tables. With `base` (the line before the text's first,
 * 0 for a whole file), each block and list item carries the source lines
 * it came from as data-line and data-line-end, counted from 1, so a place
 * picked in the Preview names its line in the file. `depth` is how deep in
 * quotes the text is.
 */
function markdown(text, base, depth = 0) {
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
  const quoted = (line) => depth < MAX_QUOTE_DEPTH && /^\s{0,3}>/.test(line);
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
    if (line.includes("|") && i + 1 < lines.length && lines[i + 1].includes("-") && tableRule(lines[i + 1])) {
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
    const title = headingOf(line);
    if (title) {
      flush();
      put(h(`h${Math.min(title[1].length + 2, 6)}`, { class: "md-h", "data-level": title[1].length }, ...inline(title[2])), start);
      continue;
    }
    if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flush();
      put(h("hr", {}), start);
      continue;
    }
    const item = listItemOf(line);
    if (item) {
      flush();
      const ordered = /\d/.test(item[2]);
      const items = [];
      while (i < lines.length) {
        const m = listItemOf(lines[i]);
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
    if (quoted(line)) {
      // One level of quote taken off; what it holds is laid out a level
      // deeper, at most MAX_QUOTE_DEPTH deep.
      flush();
      const inner = [];
      while (i < lines.length && quoted(lines[i])) inner.push(lines[i++].replace(/^\s{0,3}>\s?/, ""));
      i--;
      put(h("blockquote", {}, ...markdown(inner.join("\n"), base === undefined ? undefined : base + start, depth + 1)), start, i);
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

/** `markdown(text, base)`, or, should laying it out fail, the text as it
 * is: one message or file that cannot be laid out leaves the rest of the
 * window working. */
function markdownOrText(text, base) {
  try {
    return markdown(text, base);
  } catch {
    const p = h("p", {}, text);
    if (base !== undefined) {
      p.dataset.line = String(base + 1);
      p.dataset.lineEnd = String(base + text.split("\n").length);
    }
    return [p];
  }
}

/** Laid out once per text of `owner` (a stream item): the stream is drawn
 * again on every event of a run, and a message that has not changed keeps
 * its nodes, which the new drawing moves into place. */
const laidOut = new WeakMap();
function markdownOnce(owner, text) {
  const kept = laidOut.get(owner);
  if (kept && kept.text === text) return kept.nodes;
  const nodes = markdownOrText(text);
  laidOut.set(owner, { text, nodes });
  return nodes;
}

if (typeof module !== "undefined") {
  module.exports = { markdown, markdownOrText, markdownOnce, inline, cells, tableRule, listItemOf, headingOf, MAX_QUOTE_DEPTH, INLINE_SPAN_MAX };
}
