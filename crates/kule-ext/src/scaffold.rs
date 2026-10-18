//! `kule ext new`: a folder to start an extension from, that installs as
//! it is.

use std::fs;
use std::path::Path;

use crate::error::{At, Result, invalid};
use crate::package::pack;
use crate::text::slug;

/// The SDK's types, for an editor.
pub const SDK_TYPES: &str = include_str!("../sdk/kule-ext.d.ts");

/// Writes a new extension `id` (`publisher.name`) into `dir`, which must not
/// exist or be empty.
pub fn scaffold(dir: &Path, id: &str) -> Result<()> {
    let (publisher, name) = id
        .split_once('.')
        .ok_or_else(|| invalid(format!("{id:?} must be publisher.name")))?;
    slug("the id's publisher", publisher, 20)?;
    slug("the id's name", name, 20)?;
    if dir.exists() && fs::read_dir(dir).at(dir)?.next().is_some() {
        return Err(invalid(format!("{} is not empty", dir.display())));
    }
    let title: String = name
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let manifest = serde_json::json!({
        "kule_extension": 1,
        "id": id,
        "name": title,
        "version": "0.1.0",
        "publisher": publisher,
        "description": "What it does, in a sentence.",
        "main": "main.js",
        "permissions": ["tasks.read"],
        "contributes": {
            "commands": [{ "id": "hello", "title": format!("{title}: say hello") }],
            "panels": [{ "id": "panel", "title": title, "entry": "panel.html" }]
        }
    });
    let files: [(&str, String); 7] = [
        (
            "kule-extension.json",
            serde_json::to_string_pretty(&manifest).expect("json") + "\n",
        ),
        (
            "main.js",
            format!(
                "// Runs in a hidden, sandboxed frame while Kule is open.\n\
                 kule.commands.register(\"hello\", async () => {{\n\
                 \x20 const tasks = await kule.tasks.list();\n\
                 \x20 console.log(`{title} sees ${{tasks.length}} tasks`);\n\
                 }});\n"
            ),
        ),
        (
            "panel.html",
            "<!doctype html>\n<meta charset=\"utf-8\">\n<link rel=\"stylesheet\" href=\"panel.css\">\n\
             <script src=\"_kule/sdk.js\"></script>\n<script src=\"panel.js\" defer></script>\n\
             <ul id=\"tasks\"></ul>\n"
                .to_owned(),
        ),
        (
            "panel.js",
            "// A panel: Kule's colours arrive as --kule-* CSS variables.\n\
             async function render() {\n\
             \x20 const tasks = await kule.tasks.list();\n\
             \x20 document.getElementById(\"tasks\").replaceChildren(\n\
             \x20   ...tasks.map((t) => Object.assign(document.createElement(\"li\"), { textContent: t.label || t.id }))\n\
             \x20 );\n\
             }\n\
             kule.ready.then(render);\n\
             kule.events.on(\"task-selected\", render);\n"
                .to_owned(),
        ),
        (
            "panel.css",
            "body {\n  margin: 0;\n  padding: 12px 16px;\n  background: var(--kule-bg);\n  color: var(--kule-text);\n  font: 13px system-ui, sans-serif;\n}\n"
                .to_owned(),
        ),
        ("kule-ext.d.ts", SDK_TYPES.to_owned()),
        (
            "README.md",
            format!(
                "# {title}\n\n\
                 Try it while you write it:\n\n    kule ext dev .\n    kule ext approve {id}\n\n\
                 Code changes run as they are; a changed manifest waits for approval again.\n\
                 Pack it to share it:\n\n    kule ext pack .\n\n\
                 See docs/extensions.md in Kule's repository for the manifest and the SDK.\n"
            ),
        ),
    ];
    fs::create_dir_all(dir).at(dir)?;
    for (name, text) in files {
        let path = dir.join(name);
        fs::write(&path, text).at(&path)?;
    }
    // What was written must pack; better to learn it here.
    pack(dir)?;
    Ok(())
}
