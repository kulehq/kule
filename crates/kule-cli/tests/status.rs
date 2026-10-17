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

#[test]
fn status_tells_where_another_task_changes_the_same() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let code = "impl Task {\n    fn commit(&self) {}\n    fn undo(&self) {}\n}\n";
    fs::write(ws.path().join("task.rs"), code).unwrap();
    let new_task = || {
        let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
        let id = created.split_whitespace().nth(1).unwrap().to_owned();
        let copy = home.path().join("tasks").join(&id).join("copy");
        (id, copy)
    };
    let (a, copy_a) = new_task();
    let (b, copy_b) = new_task();
    let (_, copy_c) = new_task();
    let commit = code.replace("commit(&self) {}", "commit(&self) { 1; }");
    fs::write(copy_a.join("task.rs"), &commit).unwrap();
    fs::write(copy_b.join("task.rs"), commit.replace("1;", "2;")).unwrap();
    fs::write(
        copy_c.join("task.rs"),
        code.replace("undo(&self) {}", "undo(&self) { 3; }"),
    )
    .unwrap();

    let status = kule(home.path(), &["status", &a]);
    let expected = format!("task {b} changes them too:\n  task.rs · method Task::commit\n");
    assert!(status.ends_with(&expected), "{status}");
}

#[test]
fn status_shows_moves_as_moves() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::create_dir(ws.path().join("photos")).unwrap();
    for i in 0..3 {
        fs::write(ws.path().join(format!("photos/{i}.jpg")), [i; 8]).unwrap();
    }
    fs::write(ws.path().join("notes.md"), "# Notes\n").unwrap();
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    let copy = home.path().join("tasks").join(id).join("copy");
    fs::rename(copy.join("photos"), copy.join("2026")).unwrap();
    fs::create_dir(copy.join("docs")).unwrap();
    fs::rename(copy.join("notes.md"), copy.join("docs/notes.md")).unwrap();

    let status = kule(home.path(), &["status", id]);
    assert!(
        status.starts_with(
            "6 file changes: +1 ~0 -0 »5\n  » photos/ → 2026/\n  + docs/\n  » notes.md → docs/notes.md\n"
        ),
        "{status}"
    );
    assert!(status.contains("passes on its own"), "{status}");
}
