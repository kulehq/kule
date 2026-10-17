//! Two tasks on one folder that change the same file: the second commit
//! merges what keeps apart, and refuses what both changed.

use std::fs;

use kule_engine::{Error, Task};
use tempfile::TempDir;

const CODE: &str = "impl Task {\n    fn commit(&self) {\n        1;\n    }\n\n    fn undo(&self) {\n        2;\n    }\n}\n";

struct Setup {
    home: TempDir,
    ws: TempDir,
}

impl Setup {
    fn new() -> Setup {
        let s = Setup {
            home: TempDir::new().unwrap(),
            ws: TempDir::new().unwrap(),
        };
        fs::write(s.ws.path().join("task.rs"), CODE).unwrap();
        fs::write(s.ws.path().join("notes.txt"), "a\nb\nc\n").unwrap();
        s
    }

    fn task(&self) -> Task {
        Task::create(self.home.path(), self.ws.path()).unwrap()
    }

    fn work(&self, file: &str) -> String {
        fs::read_to_string(self.ws.path().join(file)).unwrap()
    }
}

fn write(task: &Task, file: &str, text: &str) {
    fs::write(task.copy().join(file), text).unwrap();
}

fn conflicts(result: kule_engine::Result<Option<kule_engine::Checkpoint>>) -> Vec<String> {
    match result {
        Err(Error::Conflict(paths)) => paths,
        other => panic!("not a conflict: {other:?}"),
    }
}

#[test]
fn changes_to_different_methods_both_reach_the_folder() {
    let s = Setup::new();
    let (a, b) = (s.task(), s.task());
    write(&a, "task.rs", &CODE.replace("1;", "10;"));
    write(&b, "task.rs", &CODE.replace("2;", "20;"));
    write(&b, "notes.txt", "a\nb\nC\n");
    write(&a, "notes.txt", "A\nb\nc\n");
    a.commit().unwrap().unwrap();

    let checkpoint = b.commit().unwrap().unwrap();
    assert_eq!(checkpoint.merged, ["notes.txt", "task.rs"]);
    assert_eq!(
        s.work("task.rs"),
        CODE.replace("1;", "10;").replace("2;", "20;")
    );
    assert_eq!(s.work("notes.txt"), "A\nb\nC\n");
    // The copy holds what was committed: nothing left to commit.
    assert!(b.effects().unwrap().is_empty());

    // Undoing the second commit leaves the first one's work.
    b.undo().unwrap().unwrap();
    assert_eq!(s.work("task.rs"), CODE.replace("1;", "10;"));
}

#[test]
fn changes_to_the_same_method_are_refused_by_name() {
    let s = Setup::new();
    let (a, b) = (s.task(), s.task());
    write(&a, "task.rs", &CODE.replace("1;", "10;"));
    write(&b, "task.rs", &CODE.replace("1;", "11;"));
    a.commit().unwrap().unwrap();
    assert_eq!(
        conflicts(b.commit()),
        ["task.rs (both changed method Task::commit)"]
    );
    assert_eq!(s.work("task.rs"), CODE.replace("1;", "10;"));
    assert_eq!(
        fs::read_to_string(b.copy().join("task.rs")).unwrap(),
        CODE.replace("1;", "11;")
    );
}

#[test]
fn a_change_made_by_hand_in_the_folder_is_a_conflict_as_before() {
    // Nothing kept what the file held before the hand-made change: no base
    // to merge from.
    let s = Setup::new();
    let b = s.task();
    write(&b, "task.rs", &CODE.replace("2;", "20;"));
    // A different size, so the change shows whatever the clock does.
    fs::write(s.ws.path().join("task.rs"), CODE.replace("1;", "100;")).unwrap();
    assert_eq!(conflicts(b.commit()), ["task.rs"]);
}
