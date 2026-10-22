---
title: Keyboard
sidebar_label: Keyboard
description: Kule's default keyboard shortcuts, the command palette, and how to set your own keys.
---

# Keyboard

Everything the window does is a **command**, and every command can have a
shortcut. **Mod** means **⌘** on macOS and **Ctrl** on Linux and Windows,
so one set of shortcuts serves all three.

- **Mod+K** opens the command palette: find any command by name, with its
  keys, including the commands that have none by default.
- **Mod+/** lists the shortcuts.
- *Settings → Shortcuts* lets you set your own (see
  [Changing a shortcut](#changing-a-shortcut)).

## Default shortcuts

### General

| Action | Keys |
| --- | --- |
| All commands (command palette) | Mod+K |
| Keyboard shortcuts | Mod+/ |
| Settings | Mod+, |

### Tasks

| Action | Keys |
| --- | --- |
| New task | Mod+N |
| Next / previous task | Ctrl+Tab / Ctrl+Shift+Tab |
| Move the task up / down the list | Alt+Shift+↑ / Alt+Shift+↓ (Alt+↑/↓ with the list focused) |
| Task actions (⋮ menu) | Mod+. |
| Rename the task | F2 |
| Open the task's copy in your terminal app, outside the sandbox | Mod+T |
| Open the task's copy in your editor | Mod+Shift+O |
| Fork the task | Mod+Shift+D |
| Bring the folder's changes into the copy | Mod+Shift+G |
| Delete the task (press twice) | Mod+Backspace |

### Panels and layout

| Action | Keys |
| --- | --- |
| Show Tasks / Files | Mod+0 / Mod+Shift+E |
| Show the agent chat / Review / Timeline / Preview | Mod+1 / Mod+2 / Mod+3 / Mod+4 |
| Show or hide the agent chat | Mod+Shift+J |
| Show or hide the left side / bottom panel / right side | Mod+B / Mod+J / Mod+Alt+B |
| Show or hide the terminal | Ctrl+\` |
| New terminal | Ctrl+Shift+\` |
| Find in the terminal | ⌘F on macOS, Ctrl+Shift+F elsewhere |

### The agent

| Action | Keys |
| --- | --- |
| Write to the agent (focus the prompt) | Mod+L |
| Run the prompt | Enter or Mod+Enter (Shift+Enter: new line) |
| Stop the agent at work | Mod+Shift+K |
| Ask the agent about what is picked in Preview | Mod+Shift+A |
| Approve / deny the waiting connection | Mod+Shift+Y / Mod+Shift+N |

### Review and files

| Action | Keys |
| --- | --- |
| Commit, or approve and commit | Mod+Shift+Enter |
| Roll back the changes (press twice) | Mod+Shift+Backspace |
| Undo the last commit (press twice) | Mod+Shift+U |
| Open the picked file with its app | Mod+O |
| Show the picked file in Finder / Files | Mod+Alt+R |
| Copy where the picked file is on disk | Mod+Alt+C |

### Appearance

| Action | Keys |
| --- | --- |
| Make the text larger / smaller | Mod+= / Mod+- |

### Commands with no default

These are in the command palette and can be given keys in *Settings →
Shortcuts*:

- New group; group policies; isolation on this computer; open or close every
  agent's details;
- show the task's copy in Finder / Files; show or hide the main area; reset
  the layout;
- the task's budget, its group's budget and limits, how long the agent waits
  for you;
- approve and remember the waiting connection;
- use the balanced / careful policy;
- export / import your profile; search the shortcuts;
- next theme, new theme from the one in use, import / export a theme, turn
  motion on or off, turn copying selected text on or off, use Kule's fonts and
  sizes again;
- the terminal's next, previous, clear and close.

## Keys Kule never takes

Combos the system or text editing owns are never taken, whatever you set:

| System | Reserved |
| --- | --- |
| macOS | ⌘Q, ⌘W, ⌘H, ⌘M, ⌘C, ⌘V, ⌘X, ⌘A, ⌘Z, ⌘⇧Z, ⌘Tab, ⌘Space, Ctrl+Space, ⌘⌥Esc |
| Linux | Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+A, Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y, Alt+F4, Alt+Tab, Alt+Space, Ctrl+Alt+Delete, Ctrl+Alt+arrows, F11 |
| Windows | Ctrl+C, Ctrl+V, Ctrl+X, Ctrl+A, Ctrl+Z, Ctrl+Y, Alt+F4, Alt+Tab, Alt+Space, Ctrl+Esc, Ctrl+Alt+Delete, F11 |

In a text field, only the shortcuts meant for it fire, so typing is never
taken over.

## Changing a shortcut

1. Open *Settings → Shortcuts* (or run *Search the shortcuts* from Mod+K).
2. Search by name or by keys, like "theme" or "⌘⇧".
3. Click a command's keys and press the ones you want:
   - **Escape** cancels;
   - **Backspace** or **Delete** leaves the command with no shortcut;
   - a combo the system or text editing owns is refused, and so is one that
     would just type a character (add ⌘, ⌃ or ⌥ on macOS, Ctrl or Alt
     elsewhere, or use a function key);
   - a combo another command has moves to this one, and Kule says from
     which.
4. **Put back every default** returns to Kule's own.

Your shortcuts are kept in `shortcuts.json` in Kule's home and survive
restarts. A shortcut set on one keyboard layout keeps working on another,
since Kule also reads where the key is.

To take your shortcuts to another computer, export them in a profile
(*Settings → Profile*, see [Settings](settings.md)).

The menu bar shows the keys you set next to each item.
