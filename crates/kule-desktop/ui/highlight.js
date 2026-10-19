// What makes the file explorer and the Preview look like an editor's: a
// file's icon, as VS Code's Material Icon Theme picks it, and code coloured
// by Prism. Prism only splits text into tokens here; the tokens become
// spans with textContent, so nothing in a file is parsed as HTML. Loaded
// after vendor/material-icons/icons.js and vendor/prism/prism.js.
"use strict";

/** Characters that show nothing yet change what text means or how it
 * reads: control characters (but tab, line feed and carriage return), the
 * marks that turn text around, zero-width ones, the soft hyphen, line and
 * paragraph separators, and the tag characters text can hide words in. */
const UNSEEN = /[\u{0}-\u{8}\u{B}\u{C}\u{E}-\u{1F}\u{7F}-\u{9F}\u{AD}\u{61C}\u{200B}-\u{200F}\u{2028}-\u{202E}\u{2060}-\u{2069}\u{FEFF}\u{E0000}-\u{E007F}]/gu;

/** `text` as a person should read it: each unseen character written out,
 * as ⟨U+202E⟩, so a file name, a line of code or anything an agent wrote
 * cannot pass for something else. Every text the window shows goes
 * through it. */
function shown(text) {
  return text.replace(UNSEEN, (c) => `⟨U+${c.codePointAt(0).toString(16).toUpperCase().padStart(4, "0")}⟩`);
}

/** Whether the window is light now: the theme's scheme, else the computer's. */
function lightScheme() {
  const scheme = document.documentElement.dataset.scheme;
  return scheme ? scheme === "light" : !matchMedia("(prefers-color-scheme: dark)").matches;
}

/** The icon of a file or folder named `name`, by its name first and its
 * extensions after, longest first (`a.spec.ts` before `ts`). */
function iconName(name, isDir, open) {
  const I = MATERIAL_ICONS;
  const n = name.toLowerCase();
  const swap = (table, key, icon) => (lightScheme() && I.light[`${table}:${key}`]) || icon;
  if (isDir) {
    if (open && I.foldersOpen[n]) return swap("folderNamesExpanded", n, I.foldersOpen[n]);
    if (!open && I.folders[n]) return swap("folderNames", n, I.folders[n]);
    return open ? I.folderOpen : I.folder;
  }
  if (I.names[n]) return swap("fileNames", n, I.names[n]);
  const parts = n.split(".");
  for (let i = 1; i < parts.length; i++) {
    const ext = parts.slice(i).join(".");
    if (I.extensions[ext]) return swap("fileExtensions", ext, I.extensions[ext]);
  }
  return I.file;
}

function fileGlyph(name, isDir, open) {
  return h("img", {
    class: "file-glyph",
    src: `vendor/material-icons/${iconName(name, isDir, open)}.svg`,
    alt: "",
    width: 16,
    height: 16,
    draggable: "false",
  });
}

/** Prism's grammar for a language as the Preview names it, or as a Markdown
 * fence does (`rs`, `ts`, `sh`). */
const PRISM_NAMES = {
  rust: "rust", rs: "rust",
  typescript: "typescript", ts: "typescript", tsx: "tsx",
  javascript: "javascript", js: "javascript", jsx: "jsx", mjs: "javascript", cjs: "javascript",
  json: "json", jsonc: "json", json5: "json5",
  xml: "markup", svg: "markup", html: "markup", htm: "markup", markup: "markup", plist: "markup",
  css: "css", scss: "scss", less: "less",
  python: "python", py: "python",
  go: "go", golang: "go",
  "c#": "csharp", csharp: "csharp", cs: "csharp",
  "f#": "fsharp", fsharp: "fsharp", fs: "fsharp",
  java: "java", kotlin: "kotlin", kt: "kotlin", swift: "swift",
  c: "c", h: "c", "c++": "cpp", cpp: "cpp", hpp: "cpp", "objective-c": "objectivec", objc: "objectivec",
  ruby: "ruby", rb: "ruby", php: "php",
  shell: "bash", bash: "bash", sh: "bash", zsh: "bash", console: "shell-session", "shell-session": "shell-session",
  powershell: "powershell", ps1: "powershell", batch: "batch", bat: "batch",
  sql: "sql", toml: "toml", yaml: "yaml", yml: "yaml", ini: "ini", properties: "properties",
  markdown: "markdown", md: "markdown",
  dockerfile: "docker", docker: "docker", makefile: "makefile", make: "makefile",
  diff: "diff", patch: "diff", lua: "lua", dart: "dart", scala: "scala",
  graphql: "graphql", gql: "graphql", protobuf: "protobuf", proto: "protobuf",
  hcl: "hcl", terraform: "hcl", tf: "hcl", r: "r", haskell: "haskell", hs: "haskell",
  elixir: "elixir", ex: "elixir", erlang: "erlang", erl: "erlang", clojure: "clojure", clj: "clojure",
  ocaml: "ocaml", ml: "ocaml", zig: "zig", latex: "latex", tex: "latex", groovy: "groovy", gradle: "groovy",
};

/** Larger text is shown without colours: tokenizing it would hold the window up. */
const HIGHLIGHT_LIMIT = 300_000;

/** `text` as nodes coloured for `language`, or as plain text when Kule has
 * no grammar for it or it is too long. */
function highlighted(text, language) {
  const id = language && PRISM_NAMES[String(language).toLowerCase()];
  const grammar = id && window.Prism && Prism.languages[id];
  if (!grammar || text.length > HIGHLIGHT_LIMIT) return [text];
  let tokens;
  try {
    tokens = Prism.tokenize(text, grammar);
  } catch {
    return [text];
  }
  const nodes = (list) =>
    list.map((t) => {
      if (typeof t === "string") return document.createTextNode(shown(t));
      const span = document.createElement("span");
      const aliases = t.alias ? [].concat(t.alias) : [];
      span.className = [t.type, ...aliases].map((c) => `tok-${c}`).join(" ");
      span.append(...nodes(Array.isArray(t.content) ? t.content : [t.content]));
      return span;
    });
  return nodes(tokens);
}
