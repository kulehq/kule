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
  const probes = Array.from({ length: 30 }, () => refusal("zsh", "file-read-data", "/dev/ttys004"));
  const groups = told.groupDenials([
    refusal("zsh", "file-read-data", "/dev/ttys003"),
    ...probes,
    refusal("node", "file-write-create", "/x"),
    refusal("zsh", "file-read-data", "/dev/ttys003"),
  ]);
  assert.deepEqual(
    groups.map(told.denialLine),
    ["file-read-data /dev/ttys004 (zsh) ×30", "file-write-create /x (node)", "file-read-data /dev/ttys003 (zsh) ×2"]
  );
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

test("the Linux note is the engine's own words", () => {
  const engine = fs.readFileSync(path.join(__dirname, "..", "..", "..", "kule-engine", "src", "sandbox.rs"), "utf8");
  assert.ok(engine.includes(`pub const UNREPORTED: &str = "${told.UNREPORTED}";`));
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
