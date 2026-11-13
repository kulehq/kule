// What the window tells the person Kule cut, ended or refused, worded once:
// text from what the daemon sends, which app.js and extensions.js lay out
// with `h` (never as HTML); and how much of it extensions are told. A
// classic script like the window's others; Node loads it too, for the
// tests under tests/.
"use strict";

/** A terminal device by number (`/dev/ttys003`, `/dev/pts/7`) as the set it
 * is one of: a shell probes every one around it. Any other path as it is. */
function deviceSet(path) {
  return path.replace(/^\/dev\/ttys\d+$/, "/dev/ttys*").replace(/^\/dev\/pts\/\d+$/, "/dev/pts/*");
}

/**
 * The sandbox's refusals with the same one (process, operation, path, and
 * where what was there was moved) once, counted, in the order each was
 * last seen: the newest last. A shell that probes the terminals around it
 * is one line, not a page, whichever terminals it tried.
 */
function groupDenials(denials) {
  const groups = new Map();
  for (const d of denials) {
    const path = deviceSet(d.path);
    const key = JSON.stringify([d.process, d.operation, path, d.saved || null]);
    const had = groups.get(key);
    groups.delete(key);
    groups.set(key, { ...d, path, count: (had ? had.count : 0) + 1 });
  }
  return [...groups.values()];
}

/** The newest `most` of `items`, and how many older ones are left out:
 * refusals, a group counting as many as it holds. */
function newest(items, most) {
  const shown = items.slice(-most);
  const left = items.slice(0, items.length - shown.length);
  return { shown, more: left.reduce((n, item) => n + (item.count || 1), 0) };
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

/**
 * What an approval card says the person approves: a tunnel Kule does not
 * look into, or what Kule does inside (`inside`, from the daemon: the
 * secrets it fills, a registry it holds to reads).
 */
function insideLine(inside) {
  if (!inside) {
    return "Approving lets it through as a tunnel: Kule sees the host and port, not what goes over it, and checks that its TLS hello asks for this host. If the hello asks for another host or hides the name, Kule cuts it and says so here.";
  }
  const does = [];
  if (inside.fills && inside.fills.length) does.push(`fills ${inside.fills.join(", ")} into its requests`);
  if (inside.read_only) does.push("lets commands read from it, never send to it");
  if (inside.git) does.push("lets fetches and reads through, and holds a push or any other write for you on its own");
  return `Kule looks inside this connection: it ${does.length ? does.join(" and ") : "reads each request"}. Approving adds nothing to what Kule fills.`;
}

/**
 * A question's title: a connection, or a request held inside one (`held`,
 * from the daemon: a push to a repository, another write to a git host).
 */
function questionTitle(q) {
  if (q.held && q.held.kind === "push") return `The agent wants to push to ${q.host}/${q.held.repo}`;
  if (q.held) return `The agent wants to write to ${q.host} (not a fetch or a push)`;
  return `The agent wants to connect to ${q.host}:${q.port}`;
}

/**
 * What a card says the person approves: for a held request, that one
 * thing for the rest of the run; for a connection, what Kule does inside
 * it, and for a git host Kule cannot see into, that a push could pass.
 */
function questionLine(q) {
  if (q.held && q.held.kind === "push") {
    return `This run already reaches ${q.host}; Kule held a push to ${q.held.repo} for you. Approving lets this run push to that repository alone, until it ends; any other repository is asked about on its own.`;
  }
  if (q.held) {
    return `This run already reaches ${q.host}; Kule held a request that would change something there (its API, say), not a fetch or a push. Approving lets this run's other writes to ${q.host} through until it ends.`;
  }
  const line = insideLine(q.inside);
  return q.push_unseen ? `${line} This is a git host: approving lets a push through as well as a fetch.` : line;
}

/** Whether a card offers "Approve and remember": a held request is
 * approved for its run alone. */
function rememberable(q) {
  return !q.held;
}

/** Why Kule refused a question nobody answered, by its answer. */
const KULE_REFUSED = {
  timed_out: "nobody answered in time",
  withdrawn: "the run ended before anyone answered",
  too_many: "too many of the run's questions were waiting already",
};

/**
 * How a connection ended, in Kule's words, when it did not go through as
 * asked (the engine's `Connection::told`); null when it did, or when the
 * policy refused it (its rules say why).
 */
function connectionEnd(c) {
  const said = (what, why) => (why ? `${what}: ${why}` : what);
  switch (c.outcome) {
    case "cut":
      return said("cut by Kule", c.cut);
    case "failed":
      return said("failed on its own", c.failed);
    case "refused":
      return c.refused ? said("refused by Kule", c.refused) : null;
    case "unresolved":
      return "the host's name did not resolve";
    case "private":
      return "refused by Kule: every address the host resolves to is on this computer or its network";
    case "unreachable":
      return "the host did not accept the connection";
    default:
      return null;
  }
}

/**
 * What became of a question, as Live shows it in the run: who decided, and
 * for one the person approved that did not go through as approved,
 * `after` (what Kule said then).
 */
function answeredLine(host, port, answer, after, held) {
  const where = held && held.kind === "push" ? `a push to ${host}/${held.repo}` : held ? `a write to ${host}` : `${host}:${port}`;
  if (answer === "approved") return after ? `You approved ${where}; then ${after}` : `You approved ${where}`;
  if (answer === "denied") return `You denied ${where}`;
  if (answer === "too_many") return `Kule refused ${where} without asking: ${KULE_REFUSED.too_many}`;
  if (KULE_REFUSED[answer]) return `Kule refused ${where}: ${KULE_REFUSED[answer]}`;
  return `${where}: ${answer}`;
}

/** Whether Kule, not the person, decided a question's `answer`. */
function refusedByKule(answer) {
  return Object.hasOwn(KULE_REFUSED, answer);
}

/** A logged connection's outcome, as the Timeline lists it. */
function connectionWhat(c) {
  const end = connectionEnd(c);
  if (c.verdict === "allow") return end || c.outcome;
  if (c.verdict === "ask") {
    if (c.answer === "approved") return (end ? `approved by you; then ${end}` : `approved by you · ${c.outcome}`) + heldWhat(c);
    if (c.answer === "denied") return "denied by you";
    if (refusedByKule(c.answer)) return `refused by Kule: ${KULE_REFUSED[c.answer]}`;
    return "refused, waits for approval";
  }
  return "refused, denied";
}

/** What became of the requests held inside a connection, for its line. */
function heldWhat(c) {
  const said = { approved: "approved by you", denied: "denied by you", ...KULE_REFUSED };
  return (c.held || [])
    .map((h) => {
      const what = h.kind === "push" ? `push to ${h.repo}` : "write";
      return ` · ${what} ${h.answer ? said[h.answer] || h.answer : "refused: nobody was there to ask"}`;
    })
    .join("");
}

/** The connections the Timeline lists, the same ones once, counted. */
function summarizeConnections(connections) {
  const seen = new Map();
  for (const c of connections) {
    const secrets = (c.secrets || []).join(", ");
    const key = JSON.stringify([c.host, c.port, connectionWhat(c), secrets]);
    const entry = seen.get(key) || { c, n: 0 };
    entry.n += 1;
    seen.set(key, entry);
  }
  return [...seen.values()].map(({ c, n }) => {
    const secrets = (c.secrets || []).length ? ` · with ${c.secrets.join(", ")}` : "";
    return `${c.host}:${c.port}${n > 1 ? ` ×${n}` : ""} · ${connectionWhat(c)}${secrets}`;
  });
}

/**
 * The questions as extensions with `questions.read` are told them: which
 * kind of connection each is, never the names of the secrets Kule would
 * fill.
 */
function questionsForExtensions(waiting) {
  return waiting.map((q) => ({
    ...q,
    inside: q.inside ? { read_only: Boolean(q.inside.read_only), fills_secrets: Boolean(q.inside.fills && q.inside.fills.length) } : null,
  }));
}

/** How long a question's card shows before Approve takes a click or a
 * key: a card that appears under the pointer, or under a key already on
 * its way, is not approved by accident. */
const QUESTION_GUARD_MS = 600;

/** Whether a card shown at `shownAt` takes an approval at `now`. */
function answerableYet(shownAt, now) {
  return shownAt !== undefined && now - shownAt >= QUESTION_GUARD_MS;
}

/** What a frame is told when it connects: the task chosen now only with
 * `tasks.read`, as `task-selected` is. */
function frameContext(context, task, perms) {
  return { ...context, task: perms.has("tasks.read") ? task || null : null };
}

/** What Kule answered an agent asking for permission: what Kule did, from
 * the option's kind, and the agent's own words for it only quoted after. */
function permissionLine(title, chosen, granted) {
  return `${granted ? "Kule allowed" : "Kule did not allow"} “${title}” (the agent's option: “${chosen}”)`;
}

/** What a terminal the person closed said as its shell ended (what the
 * sandbox refused it, what Kule ended), as notes for the task's Live: its
 * tab is gone, so they would be lost there. */
/** What a terminal tab shows until its shell starts, frame by frame. */
const SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/** The line a tab waiting for its shell shows at frame `i`, written over
 * itself; with `still` (the person asked for less motion) it does not
 * turn. */
function spinnerLine(i, text, still) {
  const frame = still ? "…" : SPINNER[i % SPINNER.length];
  return `\r\x1b[2K\x1b[2m${frame} ${text}\x1b[0m`;
}

/** The name a task is listed by: its folder's, or its id when its record
 * cannot be read and there is no folder to name it by. */
function taskFolder(task) {
  const parts = String(task.workspace || "").split("/").filter(Boolean);
  return parts.length ? parts[parts.length - 1] : task.id;
}

/** What a closed tab's shell said as it ended, for its task's Live; when
 * the system log dropped the kernel's messages (`lost`), a quiet note. */
function closedTerminalNotes(notes, lost) {
  return [
    ...(notes || []).map((note) => ({ kind: "note", text: `A terminal you closed: ${note}` })),
    ...(lost ? [{ kind: "note", quiet: true, text: `A terminal you closed: ${lost}` }] : []),
  ];
}

/** What a shell's end writes in its tab: Kule's notes in yellow, and when
 * the system log dropped the kernel's messages (`lost`), that in the dim
 * grey shells use for hints, as macOS does it every few seconds. */
function terminalEnd(notes, lost) {
  return [
    ...(notes || []).map((note) => `\r\n\x1b[33mkule: ${terminalText(note)}\x1b[0m`),
    ...(lost ? [`\r\n\x1b[90mkule: ${terminalText(lost)}\x1b[0m`] : []),
  ].join("");
}

/**
 * How Live marks one line: whose it is, and the mark before it. Kule's
 * own notes alone carry Kule's name and the class for it; a line an agent
 * names (a tool call's title) never does, whatever it says, so a sentence
 * in Kule's voice from an agent still reads as the agent's.
 */
function liveLine(item) {
  if (item.kind === "note") {
    return { who: "Kule", classes: item.quiet ? "event kule-note quiet" : "event kule-note", mark: "◆" };
  }
  const state = item.done ? (item.failed ? "failed" : "done") : "";
  return { who: null, classes: `event tool ${state}`.trim(), mark: item.done ? (item.failed ? "✗" : "✓") : "›" };
}

if (typeof module !== "undefined") {
  module.exports = {
    spinnerLine,
    SPINNER,
    taskFolder,
    closedTerminalNotes,
    terminalEnd,
    groupDenials,
    newest,
    denialLine,
    moreLine,
    extensionStatus,
    terminalText,
    openedNote,
    insideLine,
    questionTitle,
    questionLine,
    rememberable,
    heldWhat,
    connectionEnd,
    answeredLine,
    refusedByKule,
    connectionWhat,
    summarizeConnections,
    questionsForExtensions,
    permissionLine,
    frameContext,
    QUESTION_GUARD_MS,
    answerableYet,
    liveLine,
  };
}
