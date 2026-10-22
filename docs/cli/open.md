---
title: kule open
sidebar_label: kule open
description: Open a document, picture, sound or video file from the task's copy with its usual app, after Kule checks it is safe to open.
---

# kule open

Open a file of the task's copy with its app: documents, pictures, sound and
video only.

## Synopsis

```sh
kule open [OPTIONS] <TASK> <FILE>
```

## Description

The agent wrote the files in the copy, so Kule does not trust them. Opening a
file with its app happens **outside** the sandbox and the proxy, on your
computer. A program, a script, a web page or an Office file with macros
would run with all of your access, and a document that loads a template, a
picture or data from elsewhere would reach the network past Kule's policy.
So `kule open` opens only files that do neither by being opened, and checks
each one first:

- **Its kind is on Kule's list:** PDF, Word, Excel, PowerPoint, OpenDocument
  (text, spreadsheet, presentation), CSV, plain text, and the common picture,
  sound and video kinds. Never a program, a script, a web page or SVG, a link
  file, or an Office file with macros.
- **It is what its name says:** Kule reads the file's first bytes. A script
  named `notes.pdf` is refused.
- **It keeps to itself:** Kule looks inside Word, Excel, PowerPoint,
  OpenDocument and CSV files. It refuses one with macros, controls or
  embedded objects; a link to anything outside the file but a web page or
  an email address you would click (a template, a picture, another workbook,
  a data connection); a Word field that fetches, includes or runs something;
  a formula that reaches the network or another program; and a CSV cell
  that a spreadsheet app would read as a formula. In a PDF, every object is
  read, those packed into compressed streams too: one with a script, a
  program to start, a form that sends itself, another document or a file
  it carries, media that runs, or an action that runs by itself as it opens
  is refused, as is an encrypted PDF. Links you click stay.
- **It is reached without a link,** inside the copy, and has only one name on
  disk.
- **No agent is at work** in the task.

What opens is **Kule's own copy** of the file, made before it is checked,
in `~/.kule/opened`, readable only by you. Nothing the agent does to its file
afterwards is what the app reads. The copy carries none of the file's
extended attributes, and on macOS it is marked as downloaded, as a file
from the web is. Changes you make in the app stay in that copy, not in the
task's: copies are removed a day after they were opened.

On macOS the copy opens with `open`; on Linux with `xdg-open`.

`--dry-run` does all the checks, makes the copy, and prints where it is,
without opening it.

## Arguments

| Name | Required | Meaning |
| --- | --- | --- |
| `TASK` | yes | The task's full id. See [Naming a task](index.md). |
| `FILE` | yes | A file inside the copy, relative to its top, like `reports/q3.pdf`. |

## Options

| Option | Value | Default | Meaning |
| --- | --- | --- | --- |
| `--dry-run` | | | Check the file and print where Kule's copy of it is, without opening it. |
| `--home` | path | `~/.kule` | Kule's home (also `KULE_HOME`). See [global options](index.md). |

## Examples

**Open a PDF the agent made:**

```sh
kule open 3422f830 report.pdf
```

**Check first:**

```text
$ kule open 3422f830 report.pdf --dry-run
/Users/me/.kule/opened/18e0e1b1ae198f08-54035-0/report.pdf
```

**A script is refused:**

```text
$ kule open 3422f830 run.sh
kule: run.sh: Kule opens documents, pictures, sound and video with their app, not this kind of file; it would run outside the sandbox
```

**A file that is not what its name says is refused:**

```text
$ kule open 3422f830 fake.pdf
kule: fake.pdf: it is not the .pdf file its name says, so Kule does not open it
```

## Output

Nothing when the file opens. With `--dry-run`, the absolute path of Kule's copy.

## Errors and fixes

| Message | What to do |
| --- | --- |
| `kule: <file>: Kule opens documents, pictures, sound and video with their app, not this kind of file; …` | By design. Look at the file in a text viewer, or with [kule exec](exec.md) inside the sandbox. |
| `kule: <file>: it is not the .<ext> file its name says, so Kule does not open it` | The content does not match the name. Treat the file with suspicion. |
| `kule: <file>: Kule does not open it with its app: …` | The file would run something or reach outside itself; the rest of the message says what. Look at it in the Preview, or inside the sandbox. |
| `kule: <file>: too large for Kule to look inside, …` | Word, Excel, PowerPoint, OpenDocument and CSV files over 20 MB, and PDFs over 100 MB, are not opened. |
| `kule: <file>: a path inside the copy, without ..` | Use a path relative to the copy's top. |
| `kule: an agent is at work in task <id> (process <pid>); wait until it is done` | Wait for the run to end. |

## In the app

Right-click a file in **Files** and choose *Open with its app* (⌘O on macOS,
Ctrl+O on Linux), or use the button the Preview shows for files it cannot
show itself. See [Files and Preview](../app/files-and-preview.md).

## See also

- [kule path](path.md)
- [Security](../security.md)
