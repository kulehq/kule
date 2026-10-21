---
title: Files and Preview
sidebar_label: Files and Preview
description: Browse a task's copy, preview text, code, pictures, sheets and documents, open files outside Kule, and ask the agent about a place in a file.
---

# Files and Preview

**Files** shows the task's copy, the folder the agent works in, and
**Preview** shows one of its files large. Both show the copy as it is now,
not your real folder. Neither ever follows a link or leaves the copy.

## Files

**Files** (Mod+Shift+E) shows the task's copy as a tree:

- each file and folder has its icon, as VS Code's Material Icon Theme draws
  it (a Rust file, a `package.json`, a `src` folder, …);
- files the task changed are marked;
- the button at the top shows the whole copy in Finder (Files on Linux).

Click a file to open it in Preview. Right-click a file or folder for the
[outside Kule](#outside-kule) actions.

## Preview

**Preview** (Mod+4) shows the picked file:

| Kind | How it shows |
| --- | --- |
| Text and code | With line numbers and colours, as in an editor: 53 languages, up to 300 KB coloured and 1 MB shown. |
| JSON or XML packed onto a line or two | *Formatted* lays it out (up to 1 MB). |
| Markdown | Laid out, or as its source. |
| PNG, JPEG, GIF, WebP | Fitted to the panel or at 100%. |
| CSV and Excel (`.xlsx`) | A grid from A1, each sheet a tab. A bar shows a picked cell's value or formula. Formulas are not computed: a cell shows the value Excel saved with it. |
| Word (`.docx`) | Headings, paragraphs, numbered lists and tables, to read. Formatting, pictures and notes are left out. |
| Anything else | Its size and why it is not shown. |

The colours follow the theme in use. Excel and Word files are read in memory,
never unpacked to disk, and each part is inflated only up to a cap.

## Outside Kule

Right-click a file or folder in Files, or use the commands:

| Action | macOS | Linux |
| --- | --- | --- |
| **Open with its app** (the picked file) | ⌘O | Ctrl+O |
| **Show in Finder / Files** (the picked file, or the copy) | ⌥⌘R | Ctrl+Alt+R |
| **Copy path** (where it is on disk) | ⌥⌘C | Ctrl+Alt+C |
| **Open the copy in your editor** (task's ⋮ menu) | ⇧⌘O | Ctrl+Shift+O |

Where the Preview does not show a file (a PDF, a film, a slide deck), it
offers *Open with its app* and *Show in Finder*. Above a Word or Excel file,
which it shows only in part, *Open with its app* is there too.

### Open with its app

Opens documents, pictures, sound and video with the app your computer uses
for them: PDF, Word, Excel, PowerPoint, OpenDocument, Pages, Numbers,
Keynote, EPUB, RTF, CSV, plain text, and the common picture, sound and video
kinds.

:::warning What Kule will not open

Never a program, a script, a web page or SVG, a link file, or an Office file
with macros. The agent wrote them, and they would run on your computer,
outside the sandbox. A file must also be what its name says: a script named
`notes.pdf` is not opened. And it waits while the agent is at work.

:::

### Show in Finder / Files

Points at the file in its folder and never opens it, so a folder named like
an app is not started.

### Open the copy in your editor

VS Code, Cursor or Zed, the ones your computer has. The menu lists each, the
one you used last first. The first time for each editor, Kule says what an
editor runs by itself from a folder (tasks, settings, recommended
extensions, git hooks) and how to keep it from doing so:

- in **VS Code**, answer *no* when it asks whether you trust the authors
  (Restricted Mode);
- **Cursor** does not ask unless Workspace Trust is on in its settings.

It is refused, as the terminal is, while an agent is at work in the task,
and while the task changed what would run there by itself: git config or
hooks, a toolchain pin, an editor's settings and tasks. What you change in
the editor shows in Review like the
agent's work.

## Asking the agent about a place in a file

In Preview, point at what you mean instead of describing it:

- **pick some text**: lines of code, a paragraph, a Markdown section;
- **drag over cells** in a sheet;
- **drag over a region** of a picture.

An **Ask** button shows beside what you picked (Mod+Shift+A does the same).
To ask about the whole file, use *Ask about it* above the Preview, or
Mod+Shift+A with nothing picked. The box that opens offers two things:

| Choice | What happens |
| --- | --- |
| **Ask now** | Write the question; the agent gets it at once. |
| **Add to prompt** | The place becomes a chip over the prompt line, so you can collect several places (up to 20) and write one prompt about them. Chips stay with their task until the prompt is sent. |

The agent gets the places as a numbered list after your prompt, each named
so it can find it:

- lines of a file;
- the source lines and headings of a Markdown section;
- a range of cells and the values in it;
- a region in the picture's own pixels;
- a numbered paragraph of a Word document.

An agent that takes pictures also gets each picked region as a picture
(three to a prompt). One that does not gets where the region is, and Live
says so. Live shows a prompt's places as chips over it.

## From a terminal

[`kule path TASK [FILE]`](../cli/path.md) prints where the copy, or a file
in it, is on disk (`cd "$(kule path TASK)"`), and
[`kule open TASK FILE`](../cli/open.md) opens a file of the copy with its
app, held to the same kinds and checks (`--dry-run` checks it and prints
where it is).
