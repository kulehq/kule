// The window's Markdown renderer (../markdown.js), run under Node with
// `node --test crates/kule-desktop/ui/tests/`. No packages: a small stand-in
// for the DOM below gives the renderer what the window's `h` and
// `highlighted` give it.
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const path = require("node:path");

// --- A stand-in DOM ----------------------------------------------------------

class Node {}

class Text extends Node {
  constructor(data) {
    super();
    this.data = data;
    this.parentNode = null;
  }
  get textContent() {
    return this.data;
  }
}

class Element extends Node {
  constructor(tag) {
    super();
    this.tagName = tag.toUpperCase();
    this.attributes = new Map();
    this.childNodes = [];
    this.dataset = {};
    this.className = "";
    this.parentNode = null;
  }
  setAttribute(name, value) {
    this.attributes.set(name, String(value));
  }
  getAttribute(name) {
    return this.attributes.has(name) ? this.attributes.get(name) : null;
  }
  append(...children) {
    for (const child of children) {
      const node = child instanceof Node ? child : new Text(String(child));
      if (node.parentNode) node.parentNode.childNodes.splice(node.parentNode.childNodes.indexOf(node), 1);
      node.parentNode = this;
      this.childNodes.push(node);
    }
  }
  get children() {
    return this.childNodes.filter((c) => c instanceof Element);
  }
  get textContent() {
    return this.childNodes.map((c) => c.textContent).join("");
  }
}

globalThis.Node = Node;
globalThis.document = {
  createElement: (tag) => new Element(tag),
  createTextNode: (data) => new Text(data),
};

// As app.js's `h`: attributes set, never parsed; children as text nodes.
globalThis.h = (tag, attrs, ...children) => {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs || {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key.startsWith("on")) throw new Error("the renderer adds no handlers");
    if (key === "class") el.className = value;
    else el.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children.flat()) {
    if (child === undefined || child === null || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return el;
};

// As highlight.js's without a grammar: the code as it is.
globalThis.highlighted = (text) => [text];

const md = require(path.join(__dirname, "..", "markdown.js"));
const { markdown, inline } = md;

// --- Helpers -----------------------------------------------------------------

/** A node tree as a short string: tags with their text, for comparing. */
function shape(nodes) {
  return nodes
    .map((n) => {
      // `inline` hands plain text back as strings, for `h` to make nodes of.
      if (typeof n === "string") return JSON.stringify(n);
      if (n instanceof Text) return JSON.stringify(n.data);
      const tag = n.tagName.toLowerCase();
      const cls = n.className ? `.${n.className.split(" ").join(".")}` : "";
      return `${tag}${cls}(${shape(n.childNodes)})`;
    })
    .join(" ");
}

function* walk(nodes) {
  for (const n of nodes) {
    yield n;
    if (n instanceof Element) yield* walk(n.childNodes);
  }
}

/** Fails when `fn` takes longer than the window may wait for it. */
const BUDGET_MS = 100;
function quick(name, fn) {
  const start = performance.now();
  fn();
  const took = performance.now() - start;
  assert.ok(took < BUDGET_MS, `${name} took ${took.toFixed(0)} ms (budget ${BUDGET_MS} ms)`);
}

// --- Time linear in the length -------------------------------------------------

const LONG = {
  "3000 backticks after a letter": "a" + "`".repeat(3000),
  "backtick runs of growing length": Array.from({ length: 120 }, (_, n) => "`".repeat(n + 1)).join("x"),
  "[ × 100000": "[".repeat(100000),
  "[a]( × 20000": "[a](".repeat(20000),
  // The most the Preview reads of a file.
  "[ × 1 MiB": "[".repeat(1 << 20),
  "[a]( × 1 MiB": "[a](".repeat(1 << 18),
  "[a] then ( × 50000":"[a](" + "(".repeat(50000),
  "* × 100000": "*".repeat(100000),
  "*a × 20000": "*a".repeat(20000),
  "** then text × 50000": "**" + "a".repeat(50000),
  "_ × 100000": "_".repeat(100000),
  "_a × 20000": " _a".repeat(20000),
  "__a × 20000": " __a".repeat(20000),
  "a heading line of spaces": "# a" + " ".repeat(3000) + "x",
  "a list item of spaces": "- " + " ".repeat(50000) + "a\u2028",
  "a table rule of spaces": "a | b\n--" + " ".repeat(50000) + "x",
};

for (const [name, text] of Object.entries(LONG)) {
  test(`inline and markdown take linear time: ${name}`, () => {
    quick(`inline(${name})`, () => inline(text));
    quick(`markdown(${name})`, () => markdown(text));
    quick(`markdown(${name}, 0)`, () => markdown(text, 0));
  });
}

test("20000 > on one line take linear time and do not throw", () => {
  quick("markdown", () => markdown(">".repeat(20000)));
  quick("markdown", () => markdown("> ".repeat(20000) + "deep"));
});

test("20000 > on 20000 lines take linear time and do not throw", () => {
  quick("markdown", () => markdown(Array.from({ length: 20000 }, () => ">").join("\n")));
  quick("markdown", () => markdown(Array.from({ length: 2000 }, (_, n) => ">".repeat(n % 200) + " x").join("\n")));
});

// --- Quotes ---------------------------------------------------------------------

/** How deep blockquotes nest in `nodes`. */
function quoteDepth(nodes) {
  let deepest = 0;
  for (const n of nodes) {
    if (!(n instanceof Element)) continue;
    const below = quoteDepth(n.childNodes);
    deepest = Math.max(deepest, n.tagName === "BLOCKQUOTE" ? below + 1 : below);
  }
  return deepest;
}

test("quotes nest up to the cap, and deeper > are text", () => {
  assert.equal(quoteDepth(markdown("> > > three")), 3);
  assert.equal(quoteDepth(markdown(">".repeat(32) + " x")), 32);
  const deep = markdown(">".repeat(40) + " x");
  assert.equal(quoteDepth(deep), 32);
  assert.match(deep[0].textContent, /^>{8} x$/);
  assert.equal(quoteDepth(markdown(">".repeat(20000))), 32);
});

test("a quote keeps its lines and its blocks", () => {
  assert.equal(shape(markdown("> a\n> b\n>\n> - c")), 'blockquote(p("a\\nb") ul(li.depth-0("c")))');
  const [q] = markdown("x\n\n> a\n> > b", 0);
  assert.equal(shape([q]), 'p("x")');
  const quote = markdown("x\n\n> a\n> > b", 0)[1];
  assert.equal(quote.dataset.line, "3");
  assert.equal(quote.dataset.lineEnd, "4");
  assert.equal(quote.childNodes[1].dataset.line, "4");
});

// --- Inline -----------------------------------------------------------------------

test("code spans", () => {
  assert.equal(shape(inline("a `b` c")), '"a " code("b") " c"');
  assert.equal(shape(inline("``a ` b``")), 'code("a ` b")');
  assert.equal(shape(inline("`` `x` ``")), 'code("`x`")');
  assert.equal(shape(inline("```js```")), 'code("js")');
  assert.equal(shape(inline("`*not em*` and `[no](link)`")), 'code("*not em*") " and " code("[no](link)")');
  // An opener without a closer of its length is text.
  assert.equal(shape(inline("a `` b ` c")), '"a `` b ` c"');
  assert.equal(shape(inline("a" + "`".repeat(30))), JSON.stringify("a" + "`".repeat(30)));
  // Across a line break in a paragraph.
  assert.equal(shape(inline("`a\nb`")), 'code("a\\nb")');
});

test("bold and italics", () => {
  assert.equal(shape(inline("**bold** and __bold__")), 'strong("bold") " and " strong("bold")');
  assert.equal(shape(inline("*it* and _it_")), 'em("it") " and " em("it")');
  assert.equal(shape(inline("**a _b_ c**")), 'strong("a " em("b") " c")');
  assert.equal(shape(inline("snake_case_name and 2*3*4")), '"snake_case_name and 2" em("3") "4"');
  assert.equal(shape(inline("* not a list, a * b")), '"* not a list, a * b"');
  assert.equal(shape(inline("__init__ x")), 'strong("init") " x"');
  assert.equal(shape(inline("a__b__ c")), '"a__b__ c"');
  assert.equal(shape(inline("_a_b")), '"_a_b"');
});

test("emphasis and link text are bounded", () => {
  const long = "a".repeat(2000);
  assert.equal(shape(inline(`**${long}**`)), JSON.stringify(`**${long}**`));
  assert.equal(shape(inline(`*${long}*`)), JSON.stringify(`*${long}*`));
  assert.equal(shape(inline(`[${long}](u)`)), JSON.stringify(`[${long}](u)`));
  const fits = "a".repeat(900);
  assert.equal(shape(inline(`**${fits}**`)), `strong(${JSON.stringify(fits)})`);
});

test("links are shown, never made live", () => {
  assert.equal(shape(inline("see [the docs](https://kule.dev) now")), '"see " span.md-link("the docs") span.md-url(" (https://kule.dev)") " now"');
  const nodes = inline("[click](javascript:alert(1))");
  for (const n of walk(nodes)) {
    if (!(n instanceof Element)) continue;
    assert.notEqual(n.tagName, "A");
    assert.equal(n.getAttribute("href"), null);
    assert.equal(n.getAttribute("onclick"), null);
  }
  assert.equal(shape(nodes), 'span.md-link("click") span.md-url(" (javascript:alert(1)") ")"');
  assert.equal(nodes[0].getAttribute("title"), "javascript:alert(1");
  assert.equal(shape(inline("[**b**](u)")), 'span.md-link(strong("b")) span.md-url(" (u)")');
  assert.equal(shape(inline("[a b](has space)")), '"[a b](has space)"');
  assert.equal(shape(inline("[](u) [a]()")), '"[](u) [a]()"');
});

test("HTML stays text", () => {
  const nodes = markdown("<img src=x onerror=alert(1)> **<b>**");
  assert.equal(shape(nodes), 'p("<img src=x onerror=alert(1)> " strong("<b>"))');
});

// --- Blocks -------------------------------------------------------------------------

test("headings", () => {
  assert.equal(shape(markdown("# One\n## Two ##\n###### Six")), 'h3.md-h("One") h4.md-h("Two") h6.md-h("Six")');
  assert.equal(markdown("## Two")[0].getAttribute("data-level"), "2");
  assert.equal(shape(markdown("#no heading")), 'p("#no heading")');
  assert.equal(shape(markdown("# a `b` #")), 'h3.md-h("a " code("b"))');
  assert.equal(shape(markdown("# Title\r")), 'h3.md-h("Title")');
});

test("lists", () => {
  assert.equal(shape(markdown("- a\n- b\n  more\n  - c")), 'ul(li.depth-0("a") li.depth-0("b" "\\n" "more") li.depth-1("c"))');
  assert.equal(shape(markdown("1. one\n2) two")), 'ol(li.depth-0("one") li.depth-0("two"))');
  assert.equal(shape(markdown("- a\n1. b")), 'ul(li.depth-0("a")) ol(li.depth-0("b"))');
  const [list] = markdown("x\n\n- a\n- b", 0);
  assert.equal(shape([list]), 'p("x")');
  const items = markdown("x\n\n- a\n- b", 0)[1].children;
  assert.deepEqual(items.map((li) => li.dataset.line), ["3", "4"]);
});

test("fences, rules and paragraphs", () => {
  assert.equal(shape(markdown("```js\nlet a = `x`;\n**no**\n```\nafter")), 'pre(code("let a = `x`;\\n**no**")) p("after")');
  assert.equal(shape(markdown("~~~\nopen to the end")), 'pre(code("open to the end"))');
  assert.equal(shape(markdown("a\nb\n\n---\n\nc")), 'p("a\\nb") hr() p("c")');
  const blocks = markdown("a\nb\n\nc", 0);
  assert.deepEqual(blocks.map((b) => [b.dataset.line, b.dataset.lineEnd]), [["1", "2"], ["4", "4"]]);
});

test("tables", () => {
  assert.equal(
    shape(markdown("| a | b |\n|:--|--:|\n| `x|y` | **z** |\n| 1 \\| 2 |")),
    'div.md-table(table(thead(tr(th.align-left("a") th.align-right("b"))) tbody(tr(td.align-left(code("x|y")) td.align-right(strong("z"))) tr(td.align-left("1 | 2") td.align-right()))))'
  );
  assert.equal(shape(markdown("a | b\n:--: | ---\nc | d")), 'div.md-table(table(thead(tr(th.align-center("a") th.align-left("b"))) tbody(tr(td.align-center("c") td.align-left("d")))))');
  assert.equal(shape(markdown("a | b\n- | -")), 'p("a | b") ul(li.depth-0("| -"))');
  assert.equal(shape(markdown("a | b\n--||--")), 'p("a | b\\n--||--")');
});

// --- When laying out fails ------------------------------------------------------

test("a message that cannot be laid out is shown as text", () => {
  const real = globalThis.highlighted;
  globalThis.highlighted = () => {
    throw new RangeError("Maximum call stack size exceeded");
  };
  try {
    const text = "```js\n<b>x</b>\n```";
    assert.equal(shape(md.markdownOrText(text)), `p(${JSON.stringify(text)})`);
  } finally {
    globalThis.highlighted = real;
  }
});

test("a message is laid out once per text", () => {
  const item = { text: "**a**" };
  const first = md.markdownOnce(item, item.text);
  assert.equal(md.markdownOnce(item, item.text), first);
  item.text += " b";
  const again = md.markdownOnce(item, item.text);
  assert.notEqual(again, first);
  assert.equal(shape(again), 'p(strong("a") " b")');
  assert.notEqual(md.markdownOnce({ text: item.text }, item.text), again);
});
