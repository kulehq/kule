//! The entity map of a task's changes, read from the copy and the workspace.

use std::fs;
use std::path::Path;

use kule_engine::entities::{Change, Edit, Kind, Outline, Skip};
use kule_engine::{FileOutline, Task};
use tempfile::TempDir;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn changes(outline: &FileOutline) -> Vec<(Edit, Kind, &str)> {
    match &outline.outline {
        Outline::Changes { changes } => changes
            .iter()
            .map(
                |Change {
                     edit, kind, name, ..
                 }| (*edit, *kind, name.as_str()),
            )
            .collect(),
        Outline::Skipped { reason } => panic!("{} skipped: {reason:?}", outline.path),
    }
}

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        write(
            &ws.path().join("src/task.rs"),
            "impl Task {\n    fn commit(&self) {}\n}\n",
        );
        write(&ws.path().join("notes.md"), "# Plan\nFirst.\n\n# Done\n");
        write(&ws.path().join("old.py"), "def f():\n    pass\n");
        write(&ws.path().join("photo.jpg"), "not text");
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            task,
        }
    }

    fn outline(&self) -> Vec<FileOutline> {
        self.task
            .outline(&self.task.pending(false).unwrap())
            .unwrap()
    }
}

#[test]
fn each_changed_file_is_outlined_by_what_changed_in_it() {
    let s = Setup::new();
    let copy = s.task.copy();
    write(
        &copy.join("src/task.rs"),
        "impl Task {\n    fn commit(&self) { self.check() }\n    fn check(&self) {}\n}\n",
    );
    write(
        &copy.join("notes.md"),
        "# Plan\nFirst, then second.\n\n# Done\n",
    );
    write(&copy.join("src/new.rs"), "pub struct New;\n");
    write(&copy.join("photo.jpg"), "still not text");
    fs::remove_file(copy.join("old.py")).unwrap();

    let outline = s.outline();
    let paths: Vec<&str> = outline.iter().map(|o| o.path.as_str()).collect();
    assert_eq!(paths, ["notes.md", "old.py", "src/new.rs", "src/task.rs"]);
    assert_eq!(
        changes(&outline[0]),
        [(Edit::Modified, Kind::Section, "Plan")]
    );
    assert_eq!(changes(&outline[1]), [(Edit::Removed, Kind::Function, "f")]);
    assert_eq!(changes(&outline[2]), [(Edit::Added, Kind::Struct, "New")]);
    assert_eq!(
        changes(&outline[3]),
        [
            (Edit::Modified, Kind::Method, "Task::commit"),
            (Edit::Added, Kind::Method, "Task::check"),
        ]
    );
}

#[test]
fn a_workspace_changed_since_tells_that_the_before_is_unknown() {
    let s = Setup::new();
    write(&s.task.copy().join("src/task.rs"), "impl Task {}\n");
    write(&s.task.workspace().join("src/task.rs"), "// someone else\n");
    let outline = s.outline();
    assert_eq!(
        outline[0].outline,
        Outline::Skipped {
            reason: Skip::BeforeUnknown
        }
    );
}

#[cfg(unix)]
#[test]
fn a_link_in_the_copy_is_not_followed() {
    let s = Setup::new();
    let outside = TempDir::new().unwrap();
    write(&outside.path().join("secret.rs"), "fn secret() {}\n");
    let copy = s.task.copy();
    fs::remove_file(copy.join("src/task.rs")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret.rs"), copy.join("src/task.rs")).unwrap();
    let outline = s.outline();
    // The file's old side is listed as gone; the link is not read.
    assert_eq!(outline.len(), 1);
    assert_eq!(changes(&outline[0]), [(Edit::Removed, Kind::Impl, "Task")]);
}
