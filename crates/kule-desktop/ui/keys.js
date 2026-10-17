// Keyboard shortcuts: how a key press is read, written and shown on each
// system. A shortcut is written the same way everywhere, "Mod+Shift+K",
// where Mod is ⌘ on macOS and Ctrl on Windows and Linux, so one set of
// defaults and one saved choice serve all three.
"use strict";

const PLATFORM = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)
  ? "mac"
  : /Win/.test(navigator.platform || navigator.userAgent)
    ? "windows"
    : "linux";

const MODIFIERS = ["Mod", "Ctrl", "Alt", "Shift"];

/** Keys with names of their own, as KeyboardEvent.key gives them. */
const NAMED = new Set([
  "Enter", "Escape", "Backspace", "Delete", "Tab", "Space",
  "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight",
  "Home", "End", "PageUp", "PageDown",
  "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
]);

/** Punctuation by its place on the keyboard, for when Alt changes the
 * character a key gives (⌥ on a Mac). */
const BY_CODE = {
  Slash: "/", Comma: ",", Period: ".", Semicolon: ";", Quote: "'",
  BracketLeft: "[", BracketRight: "]", Backslash: "\\", Minus: "-", Equal: "=", Backquote: "`",
};

/** Combos the system or text editing owns; a shortcut never takes them. */
const RESERVED = {
  mac: ["Mod+Q", "Mod+W", "Mod+H", "Mod+M", "Mod+C", "Mod+V", "Mod+X", "Mod+A", "Mod+Z", "Mod+Shift+Z", "Mod+Tab", "Mod+Space", "Ctrl+Space", "Mod+Alt+Escape"],
  windows: ["Mod+C", "Mod+V", "Mod+X", "Mod+A", "Mod+Z", "Mod+Y", "Alt+F4", "Alt+Tab", "Alt+Space", "Mod+Escape", "Mod+Alt+Delete", "F11"],
  linux: ["Mod+C", "Mod+V", "Mod+X", "Mod+A", "Mod+Z", "Mod+Shift+Z", "Mod+Y", "Alt+F4", "Alt+Tab", "Alt+Space", "Mod+Alt+Delete", "Mod+Alt+ArrowUp", "Mod+Alt+ArrowDown", "Mod+Alt+ArrowLeft", "Mod+Alt+ArrowRight", "F11"],
}[PLATFORM];

/** One way of writing a combo: modifiers in a fixed order, then the key;
 * Ctrl is Mod outside macOS. */
function normalCombo(combo) {
  if (!combo) return "";
  const parts = combo.split("+").filter(Boolean);
  const key = parts.pop();
  const mods = new Set(parts.map((m) => (m === "Ctrl" && PLATFORM !== "mac" ? "Mod" : m)));
  return [...MODIFIERS.filter((m) => mods.has(m)), key].join("+");
}

function modsOf(e) {
  const mods = [];
  if (PLATFORM === "mac" ? e.metaKey : e.ctrlKey) mods.push("Mod");
  if (PLATFORM === "mac" && e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  return mods;
}

/** The ways a key press can be written: from the character it gives, and
 * from where the key is, so a shortcut set on one layout still works on
 * another. Empty for a modifier pressed alone, or the Windows key. */
function eventCombos(e) {
  if (["Meta", "Control", "Alt", "Shift", "AltGraph", "CapsLock"].includes(e.key)) return [];
  if (PLATFORM !== "mac" && e.metaKey) return [];
  const mods = modsOf(e);
  const combos = [];
  const fromKey = keyName(e.key, e.altKey && PLATFORM === "mac");
  if (fromKey) {
    // Shift that only picks the character ("?" is Shift+/) is part of it.
    const printable = fromKey.length === 1 && !/[A-Z0-9]/.test(fromKey);
    const m = printable ? mods.filter((x) => x !== "Shift") : mods;
    combos.push([...m, fromKey].join("+"));
  }
  const fromCode = codeName(e.code);
  if (fromCode) combos.push([...mods, fromCode].join("+"));
  return [...new Set(combos)];
}

function keyName(key, altered) {
  if (key === " ") return "Space";
  if (NAMED.has(key)) return key;
  if (key.length !== 1 || altered) return null;
  if (key === "+") return "Plus";
  return key.toUpperCase();
}

function codeName(code) {
  if (!code) return null;
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (NAMED.has(code)) return code;
  return BY_CODE[code] || null;
}

const MAC_SIGNS = { Mod: "⌘", Ctrl: "⌃", Alt: "⌥", Shift: "⇧" };
const KEY_SIGNS = {
  mac: { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Enter: "↩", Backspace: "⌫", Delete: "⌦", Escape: "Esc", Tab: "⇥", Space: "Space", Plus: "+" },
  other: { ArrowUp: "↑", ArrowDown: "↓", ArrowLeft: "←", ArrowRight: "→", Escape: "Esc", Plus: "+" },
};

/** A combo as this system writes it: ⌘⇧K on a Mac, Ctrl+Shift+K elsewhere. */
function formatCombo(combo) {
  if (!combo) return "";
  const parts = normalCombo(combo).split("+");
  const key = parts.pop();
  if (PLATFORM === "mac") {
    return parts.map((m) => MAC_SIGNS[m]).join("") + (KEY_SIGNS.mac[key] || key);
  }
  const names = { Mod: "Ctrl", Ctrl: "Ctrl", Alt: "Alt", Shift: "Shift" };
  return [...parts.map((m) => names[m]), KEY_SIGNS.other[key] || key].join("+");
}

/** For aria-keyshortcuts, which names the real keys. */
function ariaCombo(combo) {
  if (!combo) return "";
  const parts = normalCombo(combo).split("+");
  const key = parts.pop();
  const names = { Mod: PLATFORM === "mac" ? "Meta" : "Control", Ctrl: "Control", Alt: "Alt", Shift: "Shift" };
  return [...parts.map((m) => names[m]), key === "Plus" ? "+" : key].join("+");
}

function isReserved(combo) {
  return RESERVED.includes(normalCombo(combo));
}

/** Whether a combo can stand on its own as a shortcut: a modifier other
 * than Shift, or a function key. A letter alone would be typed. */
function isUsable(combo) {
  const parts = normalCombo(combo).split("+");
  const key = parts.pop();
  return parts.some((m) => m !== "Shift") || /^F([1-9]|1[0-2])$/.test(key);
}

/** Text the person is typing in, where only shortcuts meant for it fire. */
function typingIn(el) {
  if (!el || el === document.body) return false;
  if (el.isContentEditable) return true;
  if (el.tagName === "TEXTAREA" || el.tagName === "SELECT") return true;
  if (el.tagName !== "INPUT") return false;
  return !["button", "checkbox", "radio", "submit", "reset", "range", "color", "file"].includes(el.type);
}
