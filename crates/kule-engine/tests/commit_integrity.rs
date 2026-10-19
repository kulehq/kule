//! A commit takes exactly what was reviewed: the content, not only which
//! paths changed, and nothing from outside the copy, whatever a process
//! still running in the copy does meanwhile.

use std::fs;
use std::os::unix::fs::symlink;

use kule_engine::{Error, Task};
use tempfile::TempDir;

fn task() -> (TempDir, TempDir, Task) {
    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    fs::create_dir(ws.path().join("d")).unwrap();
    fs::write(ws.path().join("d/keep.txt"), "kept\n").unwrap();
    fs::write(ws.path().join("a.txt"), "one\n").unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    (home, ws, task)
}

#[test]
fn content_changed_after_the_review_is_not_committed() {
    let (_home, ws, task) = task();
    fs::write(task.copy().join("a.txt"), "two\n").unwrap();
    let reviewed = task.pending(false).unwrap().digest();

    // Same size, same path, same kind: only the content tells.
    fs::write(task.copy().join("a.txt"), "evil").unwrap();
    let outcome = task.commit_if(|set| {
        if set.digest() == reviewed {
            Ok(())
        } else {
            Err(Error::Invalid("not what was reviewed".into()))
        }
    });
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(ws.path().join("a.txt")).unwrap(),
        "one\n"
    );
}

#[test]
fn content_rewritten_during_a_commit_is_not_kept() {
    let (_home, ws, task) = task();
    fs::write(task.copy().join("a.txt"), "two\n").unwrap();
    let copy = task.copy();
    let outcome = task.commit_if(|_| {
        // After the check, before the copy: a process left in the copy.
        fs::write(copy.join("a.txt"), "evil\n").unwrap();
        Ok(())
    });
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(ws.path().join("a.txt")).unwrap(),
        "one\n"
    );
}

#[test]
fn a_link_put_on_the_way_during_a_commit_brings_nothing_from_outside() {
    let (_home, ws, task) = task();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("a.txt"), "SECRET\n").unwrap();
    fs::write(task.copy().join("d/a.txt"), "reviewed\n").unwrap();

    let copy = task.copy();
    let outcome = task.commit_if(|_| {
        // A process left in the copy swaps the directory for a link to
        // where a file of the same name holds something else.
        fs::rename(copy.join("d"), copy.join("d.moved")).unwrap();
        symlink(outside.path(), copy.join("d")).unwrap();
        Ok(())
    });
    let landed = fs::read_to_string(ws.path().join("d/a.txt")).unwrap_or_default();
    assert!(!landed.contains("SECRET"), "{outcome:?}: {landed:?}");
}
