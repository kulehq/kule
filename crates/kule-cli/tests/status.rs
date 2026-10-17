//! `kule status` as a person reads it.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn kule(home: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "kule {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn status_lists_what_changed_inside_each_file() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(
        ws.path().join("task.rs"),
        "impl Task {\n    fn commit(&self) {}\n}\n",
    )
    .unwrap();
    fs::write(ws.path().join("notes.md"), "# Plan\nFirst.\n").unwrap();
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    let copy = home.path().join("tasks").join(id).join("copy");
    fs::write(
        copy.join("task.rs"),
        "impl Task {\n    fn commit(&self) { 1; }\n    fn check(&self) {}\n}\n",
    )
    .unwrap();
    fs::write(
        copy.join("notes.md"),
        "# Plan\nFirst.\n\n## Later\nSecond.\n",
    )
    .unwrap();

    let status = kule(home.path(), &["status", id]);
    assert!(
        status.contains(
            "  ~ notes.md\n      + section Plan › Later\n  ~ task.rs\n      ~ method Task::commit\n      + method Task::check\n"
        ),
        "{status}"
    );
}
