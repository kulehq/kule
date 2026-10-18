//! The menu bar: on macOS at the top of the screen, on Windows and Linux at
//! the top of the window. Every item of Kule's own runs one of the window's
//! commands (the same ones the keyboard and the palette run), so the menu
//! sends the command's id to the window and the window does the rest.
//!
//! The shortcut shown next to an item is the person's, sent by the window
//! whenever it changes. The window leaves out a shortcut that text editing
//! owns too (⌘⌫, ⌥⇧↑): a menu's shortcut fires in a text field as well,
//! and those keys belong to the text there.

use std::collections::HashMap;

use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Runtime};

/// What an item of Kule's own is called in the menu events.
pub const PREFIX: &str = "cmd:";

enum Entry {
    Command(&'static str, &'static str),
    Separator,
}

use Entry::{Command, Separator};

const FILE: &[Entry] = &[
    Command("task.new", "New Task…"),
    Separator,
    Command("profile.import", "Import Profile…"),
    Command("profile.export", "Export Profile…"),
    Command("theme.import", "Import Theme…"),
    Command("theme.export", "Export Theme…"),
];

const VIEW: &[Entry] = &[
    Command("app.palette", "Command Palette…"),
    Separator,
    Command("view.tasks", "Tasks"),
    Command("view.files", "Files"),
    Command("view.terminal", "Show or Hide the Terminal"),
    Command("terminal.new", "New Terminal"),
    Command("view.live", "Live"),
    Command("view.review", "Review"),
    Command("view.timeline", "Timeline"),
    Command("view.preview", "Preview"),
    Separator,
    Command("task.next", "Next Task"),
    Command("task.prev", "Previous Task"),
    Separator,
    Command("theme.next", "Next Theme"),
    Separator,
    Command("chat.toggle", "Show or Hide the Agent Chat"),
    Command("layout.main", "Show or Hide the Main Area"),
    Command("layout.left", "Show or Hide the Left Side"),
    Command("layout.bottom", "Show or Hide the Bottom Panel"),
    Command("layout.right", "Show or Hide the Right Side"),
    Command("layout.reset", "Reset the Layout"),
];

const TASK: &[Entry] = &[
    Command("prompt.focus", "Write to the Agent"),
    Command("preview.ask", "Ask About the Picked Place"),
    Command("prompt.run", "Run the Prompt"),
    Separator,
    Command("task.rename", "Rename…"),
    Command("task.menu", "Task Actions…"),
    Command("task.terminal", "Open Terminal in the Copy"),
    Command("task.fork", "Fork Task"),
    Command("task.up", "Move Up"),
    Command("task.down", "Move Down"),
    Separator,
    Command("review.commit", "Commit"),
    Command("review.rollback", "Roll Back…"),
    Command("timeline.undo", "Undo the Last Commit…"),
    Separator,
    Command("question.approve", "Approve the Waiting Connection"),
    Command("question.deny", "Deny the Waiting Connection"),
    Separator,
    Command("task.delete", "Delete Task…"),
];

const HELP: &[Entry] = &[Command("app.shortcuts", "Keyboard Shortcuts")];

/// The menu bar, with `keys` (command id → accelerator like
/// `CmdOrCtrl+Shift+K`) next to the items they belong to.
pub fn build<R: Runtime>(
    app: &AppHandle<R>,
    keys: &HashMap<String, String>,
) -> tauri::Result<Menu<R>> {
    let file = {
        let mut s = SubmenuBuilder::new(app, "File");
        s = add(app, s, FILE, keys)?;
        if !cfg!(target_os = "macos") {
            s = add(
                app,
                s.separator(),
                &[Command("app.settings", "Settings…")],
                keys,
            )?
            .separator()
            .quit();
        } else {
            s = s.separator().close_window();
        }
        s.build()?
    };
    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view = {
        let s = add(app, SubmenuBuilder::new(app, "View"), VIEW, keys)?;
        if cfg!(target_os = "macos") {
            s.separator().fullscreen()
        } else {
            s
        }
        .build()?
    };
    let task = add(app, SubmenuBuilder::new(app, "Task"), TASK, keys)?.build()?;
    let help = add(app, SubmenuBuilder::new(app, "Help"), HELP, keys)?.build()?;

    let mut menu = MenuBuilder::new(app);
    if cfg!(target_os = "macos") {
        let kule = add(
            app,
            SubmenuBuilder::new(app, "Kule").about(None).separator(),
            &[Command("app.settings", "Settings…")],
            keys,
        )?
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
        let window = SubmenuBuilder::new(app, "Window")
            .minimize()
            .maximize()
            .separator()
            .bring_all_to_front()
            .build()?;
        menu = menu.items(&[&kule, &file, &edit, &view, &task, &window, &help]);
    } else {
        menu = menu.items(&[&file, &edit, &view, &task, &help]);
    }
    menu.build()
}

fn add<'m, R: Runtime>(
    app: &'m AppHandle<R>,
    mut submenu: SubmenuBuilder<'m, R, AppHandle<R>>,
    entries: &[Entry],
    keys: &HashMap<String, String>,
) -> tauri::Result<SubmenuBuilder<'m, R, AppHandle<R>>> {
    for entry in entries {
        submenu = match entry {
            Separator => submenu.separator(),
            Command(id, label) => {
                let item = || MenuItemBuilder::with_id(format!("{PREFIX}{id}"), *label);
                // A shortcut the menu cannot take leaves the item without one
                // rather than the menu bar without the item.
                let built = match keys.get(*id).filter(|k| !k.is_empty()) {
                    Some(k) => item()
                        .accelerator(k)
                        .build(app)
                        .or_else(|_| item().build(app))?,
                    None => item().build(app)?,
                };
                submenu.item(&built)
            }
        };
    }
    Ok(submenu)
}
