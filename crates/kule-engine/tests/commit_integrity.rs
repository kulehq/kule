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

/// A path reviewed as deleted that holds a file by the time it is copied
/// is not committed: deletions are verified too.
#[test]
fn a_deletion_turned_back_into_a_file_is_not_committed() {
    let (_home, ws, task) = task();
    fs::remove_file(task.copy().join("d/keep.txt")).unwrap();
    let copy = task.copy();
    let outcome = task.commit_if(|_| {
        fs::write(copy.join("d/keep.txt"), "evil\n").unwrap();
        Ok(())
    });
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(ws.path().join("d/keep.txt")).unwrap(),
        "kept\n"
    );
}

/// A settings file with secrets in it is checked as the rest are: one
/// rewritten after the review is not committed.
#[test]
fn a_settings_file_rewritten_during_a_commit_is_not_kept() {
    let (_home, ws, task) = task();
    fs::write(task.copy().join(".env.local"), "PORT=1\n").unwrap();
    let copy = task.copy();
    let outcome = task.commit_if(|_| {
        fs::write(copy.join(".env.local"), "PORT=2\nRUN=evil\n").unwrap();
        Ok(())
    });
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(!ws.path().join(".env.local").exists());
}

/// A file changed in the folder too is merged from the copy's side as it
/// was reviewed, or not at all.
#[test]
fn a_merge_takes_the_copy_as_it_was_reviewed() {
    let lines = |changed: &[usize]| {
        (1..=12)
            .map(|i| {
                if changed.contains(&i) {
                    format!("changed {i}\n")
                } else {
                    format!("line {i}\n")
                }
            })
            .collect::<String>()
    };
    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    fs::write(ws.path().join("notes.txt"), lines(&[])).unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    fs::write(task.copy().join("notes.txt"), lines(&[10])).unwrap();
    fs::write(ws.path().join("notes.txt"), lines(&[2])).unwrap();
    let copy = task.copy();
    let outcome = task.commit_if(|_| {
        // Still far from the folder's change: it would merge cleanly.
        fs::write(copy.join("notes.txt"), lines(&[10, 11])).unwrap();
        Ok(())
    });
    assert!(outcome.is_err(), "{outcome:?}");
    assert_eq!(
        fs::read_to_string(ws.path().join("notes.txt")).unwrap(),
        lines(&[2])
    );

    // Untouched since the review, it merges.
    fs::write(copy.join("notes.txt"), lines(&[10])).unwrap();
    task.commit().unwrap();
    assert_eq!(
        fs::read_to_string(ws.path().join("notes.txt")).unwrap(),
        lines(&[2, 10])
    );
}

/// A folder in the person's folder turned into a link while a commit runs
/// takes nothing out of the folder: the commit does not write through it.
#[test]
fn a_link_put_in_the_folder_during_a_commit_takes_nothing_outside() {
    let (_home, ws, task) = task();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("new.txt"), "theirs\n").unwrap();
    fs::write(task.copy().join("d/new.txt"), "reviewed\n").unwrap();
    let work = ws.path().to_owned();
    let outcome = task.commit_if(|_| {
        fs::rename(work.join("d"), work.join("d.moved")).unwrap();
        symlink(outside.path(), work.join("d")).unwrap();
        Ok(())
    });
    assert!(outcome.is_err(), "{outcome:?}");
    // Neither written over nor, putting things back, removed.
    assert_eq!(
        fs::read_to_string(outside.path().join("new.txt")).unwrap(),
        "theirs\n"
    );
}
