// What the window tells the person Kule cut, ended or refused, worded once:
// text from what the daemon sends, which app.js and extensions.js lay out
// with `h` (never as HTML). A classic script like the window's others; Node
// loads it too, for the tests under tests/.
"use strict";

/** What the window says where the sandbox reports no refusals (Linux): not
 * that there were none. The engine's words (`kule_engine::UNREPORTED`). */
const UNREPORTED = "Linux does not tell Kule what the sandbox refused; each refusal reached the command as an error";

/**
 * The sandbox's refusals with the same one (process, operation, path, and
 * where what was there was moved) once, counted, in the order each was
 * last seen: the newest last. A shell that probes the terminals around it
 * is one line, not a page.
 */
function groupDenials(denials) {
  const groups = new Map();
  for (const d of denials) {
    const key = JSON.stringify([d.process, d.operation, d.path, d.saved || null]);
    const had = groups.get(key);
    groups.delete(key);
    groups.set(key, { ...d, count: (had ? had.count : 0) + 1 });
  }
  return [...groups.values()];
}

/** The newest `most` of `items`, and how many older ones are left out. */
function newest(items, most) {
  const shown = items.slice(-most);
  return { shown, more: items.length - shown.length };
}

/** A refusal (or a group of the same) as one line. */
function denialLine(d) {
  const count = d.count > 1 ? ` ×${d.count}` : "";
  const saved = d.saved ? ` · what was there is in ${d.saved}` : "";
  return `${d.operation} ${d.path} (${d.process})${count}${saved}`;
}

/** "… and N more, older", or "… and N more in the Timeline": what a list
 * left out, said where it ends. */
function moreLine(more, where) {
  return `… and ${more} more ${more === 1 ? "refusal" : "refusals"}${where}`;
}

/**
 * What an extension's card says it does now: `text` and the chip `kind`,
 * and `why`, when it would run but nothing may (`off`, from the host:
 * safe mode, extensions not served, WebRTC still on).
 */
function extensionStatus(l, prefix, off) {
  if (l.held) return { text: `Held: ${l.held}`, kind: "ask", why: null };
  if (!l.enabled) return { text: "Turned off", kind: "", why: null };
  if (!prefix) return { text: "Does not run now", kind: "deny", why: off || "extensions cannot run in this window now" };
  return { text: "Runs", kind: "allow", why: null };
}

/** Kule's own words for a terminal, with no control character left in
 * them: a file name they carry cannot move the cursor, set a title or
 * reach the clipboard, nor turn the text after it around. */
function terminalText(text) {
  return text.replace(/[\u0000-\u001f\u007f-\u009f\u061c\u200e\u200f\u202a-\u202e\u2066-\u2069]/g, "");
}

/** What the window says once a file opened in the person's own app. */
function openedNote(name) {
  return `${name} opened as Kule's own copy: what you change there does not reach the task, and Kule removes the copy after 24 hours. Save it elsewhere to keep it.`;
}

if (typeof module !== "undefined") {
  module.exports = { UNREPORTED, groupDenials, newest, denialLine, moreLine, extensionStatus, terminalText, openedNote };
}
