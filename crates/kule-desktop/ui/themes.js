// Themes: the colours of the window's named parts, Kule's own and the
// person's. A theme sets the CSS variables the stylesheet already uses, on
// the root element, and says whether it is light or dark; "System" sets
// nothing and follows the computer.
"use strict";

/** The parts a theme colours, in the order the editor shows them. The
 * daemon checks every theme against the same names. */
const THEME_PARTS = [
  ["bg", "Background"],
  ["surface", "Panels"],
  ["sunken", "Recessed areas"],
  ["border", "Lines"],
  ["text", "Text"],
  ["muted", "Secondary text"],
  ["faint", "Faint text"],
  ["accent", "Accent"],
  ["accent-text", "Text on the accent"],
  ["accent-soft", "Accent tint"],
  ["ok", "Passes"],
  ["ok-soft", "Passes tint"],
  ["ask", "Waits for you"],
  ["ask-soft", "Waits tint"],
  ["deny", "Denied"],
  ["deny-soft", "Denied tint"],
];

const palette = (values) => Object.fromEntries(THEME_PARTS.map(([part], i) => [part, values[i]]));

const BUILT_IN_THEMES = [
  { id: "system", name: "System", scheme: null, colors: {} },
  {
    id: "light", name: "Light", scheme: "light",
    colors: palette(["#f6f7f9", "#ffffff", "#eef0f3", "#dde1e7", "#1b1f27", "#5f6878", "#687080", "#2563eb", "#ffffff", "#e3ecfd", "#15803d", "#dcf3e4", "#a14a07", "#fdf0d9", "#b91c1c", "#fbe2e2"]),
  },
  {
    id: "dark", name: "Dark", scheme: "dark",
    colors: palette(["#12151b", "#191d25", "#10131a", "#2a303b", "#e6e9ef", "#9aa3b2", "#8d96a5", "#4f8cff", "#0b1020", "#1b2a47", "#4ade80", "#14301f", "#fbbf24", "#3a2c0f", "#f87171", "#3d1717"]),
  },
  {
    id: "coral", name: "Coral", scheme: "dark",
    colors: palette(["#131418", "#1b1d22", "#0f1013", "#2b2e36", "#eef0f3", "#a2a8b4", "#8e95a2", "#ff5a3c", "#1a0d09", "#3a1d16", "#4ade80", "#14301f", "#fbbf24", "#3a2c0f", "#ff7a7a", "#3d1717"]),
  },
  {
    id: "nord", name: "Nord", scheme: "dark",
    colors: palette(["#2e3440", "#3b4252", "#2a2f3a", "#4c566a", "#eceff4", "#d8dee9", "#a3adbf", "#88c0d0", "#2e3440", "#3b4f5c", "#a3be8c", "#3a4a3c", "#ebcb8b", "#4a4433", "#e6939a", "#3d2b2e"]),
  },
  {
    id: "solarized-light", name: "Solarized Light", scheme: "light",
    colors: palette(["#fdf6e3", "#fffbf0", "#eee8d5", "#e0d8bf", "#073642", "#586e75", "#5c6f76", "#1f74b4", "#fdf6e3", "#dbe8ef", "#5f7a00", "#ebefcf", "#735400", "#f4e7c2", "#c0291f", "#f6dcd5"]),
  },
  {
    id: "contrast-dark", name: "High contrast, dark", scheme: "dark",
    colors: palette(["#000000", "#0a0a0a", "#000000", "#ffffff", "#ffffff", "#e6e6e6", "#cfcfcf", "#ffd400", "#000000", "#333000", "#00ff7f", "#003319", "#ffd400", "#332b00", "#ff6b6b", "#3a0000"]),
  },
  {
    id: "contrast-light", name: "High contrast, light", scheme: "light",
    colors: palette(["#ffffff", "#ffffff", "#f2f2f2", "#000000", "#000000", "#1a1a1a", "#333333", "#0040c0", "#ffffff", "#dbe5ff", "#006400", "#d9f2d9", "#8a4b00", "#ffecd1", "#b00000", "#ffd9d9"]),
  },
];

/** What a theme's part looks like: its own colour, else its scheme's. */
function themeColor(theme, part) {
  if (theme.colors && theme.colors[part]) return theme.colors[part];
  const base = BUILT_IN_THEMES.find((t) => t.id === (theme.scheme === "dark" ? "dark" : "light"));
  return base.colors[part];
}

/** Puts `theme` on the window; System takes every override away. */
function applyTheme(theme) {
  const root = document.documentElement;
  for (const [part] of THEME_PARTS) {
    if (theme.scheme) root.style.setProperty(`--${part}`, themeColor(theme, part));
    else root.style.removeProperty(`--${part}`);
  }
  if (theme.scheme) root.dataset.scheme = theme.scheme;
  else delete root.dataset.scheme;
}

const isHex = (v) => /^#[0-9a-f]{6}$/i.test(v);

/** WCAG contrast ratio between two #rrggbb colours. */
function contrast(a, b) {
  const lum = (hex) => {
    const [r, g, b2] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
    const lin = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b2);
  };
  const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
}

/** The pairs a person must be able to read, and how readable they are. */
function readability(theme) {
  const c = (p) => themeColor(theme, p);
  const pairs = [
    ["Text on the background", "text", "bg", 4.5],
    ["Text on panels", "text", "surface", 4.5],
    ["Secondary text on panels", "muted", "surface", 4.5],
    ["Faint text on panels", "faint", "surface", 3],
    ["Text on the accent", "accent-text", "accent", 4.5],
    ["Waits on its tint", "ask", "ask-soft", 4.5],
    ["Denied on its tint", "deny", "deny-soft", 4.5],
  ];
  return pairs.map(([label, fg, bg, need]) => ({ label, ratio: contrast(c(fg), c(bg)), need }));
}
