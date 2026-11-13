// What the window tells the person Kule cut, ended or refused (../told.js),
// run under Node with `node --test crates/kule-desktop/ui/tests/*.test.js`.
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const told = require(path.join(__dirname, "..", "told.js"));

const refusal = (process, operation, path, saved) => ({ process, operation, path, saved: saved || null, hint: null });

test("the same refusal is one line with how many times, newest last", () => {
  const writes = Array.from({ length: 30 }, () => refusal("node", "file-write-create", "/x"));
  const groups = told.groupDenials([
    refusal("sh", "file-read-data", "/etc/y"),
    ...writes,
    refusal("python", "file-write-create", "/z"),
    refusal("sh", "file-read-data", "/etc/y"),
  ]);
  assert.deepEqual(groups.map(told.denialLine), ["file-write-create /x (node) ×30", "file-write-create /z (python)", "file-read-data /etc/y (sh) ×2"]);
});

test("a shell probing the terminals around it is one line, whichever terminals", () => {
  const groups = told.groupDenials([
    refusal("zsh", "file-read-data", "/dev/ttys003"),
    ...Array.from({ length: 30 }, (_, i) => refusal("zsh", "file-read-data", `/dev/ttys${String(i).padStart(3, "0")}`)),
    refusal("bash", "file-read-data", "/dev/pts/7"),
    refusal("bash", "file-read-data", "/dev/pts/12"),
    refusal("zsh", "file-read-data", "/dev/tty"),
  ]);
  assert.deepEqual(groups.map(told.denialLine), ["file-read-data /dev/ttys* (zsh) ×31", "file-read-data /dev/pts/* (bash) ×2", "file-read-data /dev/tty (zsh)"]);
});

test("what a list leaves out is counted in refusals, not lines", () => {
  const groups = told.groupDenials([
    ...Array.from({ length: 5 }, () => refusal("a", "file-read-data", "/old")),
    refusal("b", "file-read-data", "/new"),
  ]);
  assert.equal(told.newest(groups, 1).more, 5);
});

test("a put-back is apart from the same path refused", () => {
  const groups = told.groupDenials([refusal("kule", "put back", ".git/hooks/x", "/kept/x"), refusal("kule", "put back", ".git/hooks/x")]);
  assert.equal(groups.length, 2);
  assert.equal(told.denialLine(groups[0]), "put back .git/hooks/x (kule) · what was there is in /kept/x");
});

test("a long list says how many it left out", () => {
  const { shown, more } = told.newest([1, 2, 3, 4, 5], 2);
  assert.deepEqual(shown, [4, 5]);
  assert.equal(more, 3);
  assert.equal(told.moreLine(3, ", older"), "… and 3 more refusals, older");
  assert.equal(told.moreLine(1, " in the Timeline"), "… and 1 more refusal in the Timeline");
  assert.equal(told.newest([1], 2).more, 0);
});

test("an extension that would run but cannot says so, and why", () => {
  const on = { enabled: true, held: null };
  assert.deepEqual(told.extensionStatus(on, "kule-ext://x/", null), { text: "Runs", kind: "allow", why: null });
  const safe = told.extensionStatus(on, null, "Kule started without extensions this time (safe mode)");
  assert.equal(safe.text, "Does not run now");
  assert.match(safe.why, /safe mode/);
  assert.equal(told.extensionStatus({ enabled: false, held: null }, null, "x").text, "Turned off");
  assert.equal(told.extensionStatus({ enabled: true, held: "changed" }, null, "x").text, "Held: changed");
});

test("Kule's words in a terminal carry no control or direction-changing characters", () => {
  assert.equal(told.terminalText("ok\u001b]52;c;eA==\u0007 \u202egnp.exe\r\n"), "ok]52;c;eA== gnp.exe");
});

test("a file opened with its app is said to be Kule's copy, kept 24 hours", () => {
  const note = told.openedNote("report.docx");
  assert.match(note, /^report\.docx opened as Kule's own copy/);
  assert.match(note, /does not reach the task/);
  assert.match(note, /24 hours/);
});

// --- Connections and their questions ------------------------------------------

const connection = (fields) => ({ host: "api.example", port: 443, verdict: "allow", rules: [], outcome: "tunneled", secrets: [], ...fields });

test("an approval card says whether Kule looks inside, and what it does there", () => {
  assert.match(told.insideLine(null), /^Approving lets it through as a tunnel: Kule sees the host and port, not what goes over it/);
  assert.match(told.insideLine(null), /Kule cuts it and says so here/);
  const filled = told.insideLine({ fills: ["STRIPE_KEY"], read_only: false });
  assert.match(filled, /^Kule looks inside this connection: it fills STRIPE_KEY into its requests\./);
  assert.match(told.insideLine({ fills: [], read_only: true }), /lets commands read from it, never send to it/);
});

test("a push or a write held inside a connection is asked about as itself, for its run alone", () => {
  const push = { id: 1, host: "github.com", port: 443, rules: [], held: { kind: "push", repo: "me/proj" } };
  assert.equal(told.questionTitle(push), "The agent wants to push to github.com/me/proj");
  assert.match(told.questionLine(push), /push to that repository alone, until it ends; any other repository is asked about on its own/);
  assert.ok(!told.rememberable(push));
  const write = { ...push, held: { kind: "write" } };
  assert.equal(told.questionTitle(write), "The agent wants to write to github.com (not a fetch or a push)");
  assert.match(told.questionLine(write), /other writes to github\.com through until it ends/);
  assert.ok(!told.rememberable(write));
  const connect = { id: 2, host: "github.com", port: 443, rules: ["x"], inside: { fills: [], git: true } };
  assert.equal(told.questionTitle(connect), "The agent wants to connect to github.com:443");
  assert.match(told.questionLine(connect), /lets fetches and reads through, and holds a push or any other write for you on its own/);
  assert.ok(told.rememberable(connect));
  const ssh = { id: 3, host: "github.com", port: 22, rules: ["x"], inside: null, push_unseen: true };
  assert.match(told.questionLine(ssh), /This is a git host: approving lets a push through as well as a fetch\.$/);
  assert.doesNotMatch(told.questionLine({ ...ssh, push_unseen: false }), /git host/);
});

test("the Timeline says what became of each request held inside a connection", () => {
  const c = connection({
    host: "github.com",
    verdict: "ask",
    answer: "approved",
    outcome: "cut",
    cut: "Kule refused a request in it: a push or a write to a git host that was not approved",
    held: [
      { kind: "push", repo: "me/proj", answer: "approved" },
      { kind: "push", repo: "them/x", answer: "denied" },
      { kind: "write", answer: null },
    ],
  });
  assert.match(
    told.connectionWhat(c),
    / · push to me\/proj approved by you · push to them\/x denied by you · write refused: nobody was there to ask$/
  );
  assert.equal(told.heldWhat(connection({})), "");
  assert.equal(told.answeredLine("github.com", 443, "denied", null, { kind: "push", repo: "me/proj" }), "You denied a push to github.com/me/proj");
  assert.equal(told.answeredLine("github.com", 443, "timed_out", null, { kind: "write" }), "Kule refused a write to github.com: nobody answered in time");
});

test("Kule's own refusals are never told as the person's", () => {
  for (const answer of ["timed_out", "withdrawn", "too_many"]) {
    const line = told.answeredLine("api.example", 443, answer, null);
    assert.match(line, /^Kule refused api\.example:443/, line);
    assert.ok(told.refusedByKule(answer));
    const what = told.connectionWhat(connection({ verdict: "ask", outcome: "refused", answer }));
    assert.match(what, /^refused by Kule: /, what);
    assert.doesNotMatch(what, /by you/);
  }
  assert.ok(!told.refusedByKule("denied") && !told.refusedByKule("approved"));
  assert.equal(told.answeredLine("api.example", 443, "denied", null), "You denied api.example:443");
});

test("a connection approved and then cut says so, with why, beside the approval", () => {
  assert.equal(
    told.answeredLine("api.example", 443, "approved", "cut by Kule: its TLS hello asks for other.example"),
    "You approved api.example:443; then cut by Kule: its TLS hello asks for other.example"
  );
  const cut = connection({ verdict: "ask", outcome: "cut", answer: "approved", cut: "its TLS hello asks for other.example" });
  assert.equal(told.connectionWhat(cut), "approved by you; then cut by Kule: its TLS hello asks for other.example");
  const refused = connection({ verdict: "ask", outcome: "refused", answer: "approved", refused: "10.0.0.1 is an address on this computer or its network" });
  assert.match(told.connectionWhat(refused), /^approved by you; then refused by Kule: 10\.0\.0\.1 is an address/);
  assert.equal(told.connectionWhat(connection({ verdict: "ask", answer: "approved" })), "approved by you · tunneled");
  assert.equal(told.connectionWhat(connection({ outcome: "failed", failed: "the host went away" })), "failed on its own: the host went away");
});

test("the Timeline keeps connections with a different why apart, the same ones counted", () => {
  const lines = told.summarizeConnections([
    connection({ outcome: "cut", cut: "one" }),
    connection({ outcome: "cut", cut: "one" }),
    connection({ outcome: "cut", cut: "two" }),
    connection({ secrets: ["API_KEY"] }),
  ]);
  assert.deepEqual(lines, [
    "api.example:443 ×2 · cut by Kule: one",
    "api.example:443 · cut by Kule: two",
    "api.example:443 · tunneled · with API_KEY",
  ]);
});

test("extensions learn a question's kind, never the secrets Kule would fill", () => {
  const [q] = told.questionsForExtensions([{ id: 1, host: "api.example", port: 443, inside: { fills: ["STRIPE_KEY"], read_only: false } }]);
  assert.deepEqual(q.inside, { read_only: false, fills_secrets: true });
  assert.doesNotMatch(JSON.stringify(q), /STRIPE_KEY/);
  assert.equal(told.questionsForExtensions([{ id: 2, inside: null }])[0].inside, null);
});

test("a permission note says what Kule did, the agent's label only quoted after", () => {
  // An agent may label a refusal "Allow": the kind decides, not the label.
  assert.equal(told.permissionLine("Run rm -rf", "Allow", false), "Kule did not allow “Run rm -rf” (the agent's option: “Allow”)");
  assert.equal(told.permissionLine("Edit a.txt", "Yes", true), "Kule allowed “Edit a.txt” (the agent's option: “Yes”)");
});

test("an extension learns which task is chosen only with tasks.read", () => {
  assert.equal(told.frameContext({ panel: "notes" }, "t1", new Set(["events.read"])).task, null);
  assert.deepEqual(told.frameContext({ panel: "notes" }, "t1", new Set(["tasks.read"])), { panel: "notes", task: "t1" });
  assert.equal(told.frameContext({}, undefined, new Set(["tasks.read"])).task, null);
});

test("the window's unseen characters are the set Kule's checks of extensions use", () => {
  const fixture = JSON.parse(fs.readFileSync(path.join(__dirname, "unseen.json"), "utf8"));
  const source = fs.readFileSync(path.join(__dirname, "..", "highlight.js"), "utf8");
  const literal = source.match(/const UNSEEN = \/(.+)\/gu;/);
  assert.ok(literal, "UNSEEN is where it was");
  const unseen = new RegExp(literal[1], "u");
  const char = (u) => String.fromCodePoint(parseInt(u.slice(2), 16));
  for (const u of fixture.unseen) assert.ok(unseen.test(char(u)), u);
  for (const u of fixture.seen) assert.ok(!unseen.test(char(u)), u);
});

test("a question takes an approval only once its card has been there a moment", () => {
  assert.ok(!told.answerableYet(1000, 1000));
  assert.ok(!told.answerableYet(1000, 1000 + told.QUESTION_GUARD_MS - 1));
  assert.ok(told.answerableYet(1000, 1000 + told.QUESTION_GUARD_MS));
  assert.ok(!told.answerableYet(undefined, 5000));
  assert.ok(told.QUESTION_GUARD_MS >= 500);
});

test("a terminal the person closed tells its end in the task's Live", () => {
  assert.deepEqual(told.closedTerminalNotes(["sandbox: refused file-write-create /x (sh)", "Kule ended sleep, which the shell left running in its sandbox"]), [
    { kind: "note", text: "A terminal you closed: sandbox: refused file-write-create /x (sh)" },
    { kind: "note", text: "A terminal you closed: Kule ended sleep, which the shell left running in its sandbox" },
  ]);
  assert.deepEqual(told.closedTerminalNotes(undefined), []);
  // When the log dropped messages: a quiet note of its own, after the rest.
  assert.deepEqual(told.closedTerminalNotes(["a"], "macOS's log dropped some kernel messages"), [
    { kind: "note", text: "A terminal you closed: a" },
    { kind: "note", quiet: true, text: "A terminal you closed: macOS's log dropped some kernel messages" },
  ]);
});

test("a shell's end writes Kule's notes in yellow, and a dropped log quietly", () => {
  assert.equal(told.terminalEnd(["Kule ended sleep"], null), "\r\n\x1b[33mkule: Kule ended sleep\x1b[0m");
  assert.equal(told.terminalEnd([], "dropped"), "\r\n\x1b[90mkule: dropped\x1b[0m");
  assert.equal(told.terminalEnd(undefined, undefined), "");
  // What it says cannot move the cursor or end the colour early.
  assert.ok(!told.terminalEnd([], "a\x1b[2Jb").includes("\x1b[2J"));
});

test("a task whose record cannot be read is listed by its id", () => {
  assert.equal(told.taskFolder({ id: "6c2816d8", workspace: "/Users/me/code/app" }), "app");
  assert.equal(told.taskFolder({ id: "6c2816d8", unreadable: "its record cannot be read: EOF" }), "6c2816d8");
});

test("a tab waiting for its shell turns its line over itself, or holds it still", () => {
  const first = told.spinnerLine(0, "starting the shell", false);
  const next = told.spinnerLine(1, "starting the shell", false);
  assert.ok(first.startsWith("\r\x1b[2K"), JSON.stringify(first));
  assert.ok(first.includes(`${told.SPINNER[0]} starting the shell`));
  assert.notEqual(first, next);
  assert.equal(told.spinnerLine(0, "x", false), told.spinnerLine(told.SPINNER.length, "x", false));
  // Less motion asked for: the same line every time.
  assert.equal(told.spinnerLine(0, "x", true), told.spinnerLine(1, "x", true));
});

test("a line an agent names is never marked as Kule's, whatever it says", () => {
  const tool = told.liveLine({ kind: "tool", title: "Kule: approve the next question to continue" });
  const note = told.liveLine({ kind: "note", text: "Kule ended sleep" });
  assert.equal(tool.who, null);
  assert.ok(!tool.classes.includes("kule-note"));
  assert.notEqual(tool.mark, note.mark);
  assert.equal(note.who, "Kule");
  assert.ok(note.classes.includes("kule-note"));
  // Done or failed, the same: the agent's.
  for (const done of [{ done: true }, { done: true, failed: true }]) {
    const line = told.liveLine({ kind: "tool", title: "x", ...done });
    assert.equal(line.who, null);
    assert.ok(!line.classes.includes("kule-note"));
  }
});
