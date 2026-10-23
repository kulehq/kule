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
