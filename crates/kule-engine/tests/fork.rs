//! Forking a task: a new task on the same folder starting from this one's
//! copy, now or at a step; on the real filesystem.

use std::fs;
use std::path::Path;

use kule_engine::Task;
use tempfile::TempDir;

const ENV: &str = "API_KEY=sk_live_51Hreal\n";

struct Setup {
    home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        fs::write(ws.path().join("a.txt"), "a0\n").unwrap();
        fs::write(ws.path().join("same.txt"), "same\n").unwrap();
        fs::write(ws.path().join(".env"), ENV).unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup { home, ws, task }
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    fs::write(root.join(rel), text).unwrap();
}

fn read(root: &Path, rel: &str) -> Option<String> {
    fs::read_to_string(root.join(rel)).ok()
}

/// What a task would commit, as path and change.
fn changes(task: &Task) -> Vec<(String, String)> {
    task.effects()
        .unwrap()
        .files()
        .map(|e| (e.path.clone(), format!("{:?}", e.change)))
        .collect()
}

#[test]
fn a_fork_starts_where_its_task_is_and_goes_its_own_way() {
    let s = Setup::new();
    let copy = s.task.copy();
    write(&copy, "a.txt", "a1\n");
    write(&copy, "b.txt", "b1\n");
    s.task.step("one").unwrap();

    let fork = s.task.fork(None).unwrap();
    assert_ne!(fork.id(), s.task.id());
    assert_eq!(fork.workspace(), s.task.workspace());
    // The same changes, and nothing else: the fork's untouched files are
    // not changes for having been cloned.
    assert_eq!(changes(&fork), changes(&s.task));
    assert_eq!(changes(&fork).len(), 2, "{:?}", changes(&fork));
    assert!(fork.steps().unwrap().is_empty());
    // The secret stays masked in the fork.
    assert!(!read(&fork.copy(), ".env").unwrap().contains("sk_live"));

    // Each goes its own way.
    write(&fork.copy(), "a.txt", "fork\n");
    write(&copy, "same.txt", "source\n");
    assert_eq!(read(&copy, "a.txt").as_deref(), Some("a1\n"));
    assert_eq!(read(&fork.copy(), "same.txt").as_deref(), Some("same\n"));
    // It is listed with the others.
    let ids: Vec<String> = Task::list(s.home.path())
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert!(ids.contains(&fork.id().to_owned()), "{ids:?}");
}

#[test]
fn a_fork_can_start_at_an_earlier_step() {
    let s = Setup::new();
    let copy = s.task.copy();
    write(&copy, "a.txt", "a1\n");
    s.task.step("one").unwrap();
    write(&copy, "a.txt", "a2\n");
    write(&copy, "late.txt", "late\n");
    s.task.step("two").unwrap();

    let fork = s.task.fork(Some(1)).unwrap();
    assert_eq!(read(&fork.copy(), "a.txt").as_deref(), Some("a1\n"));
    assert_eq!(read(&fork.copy(), "late.txt"), None);
    assert_eq!(
        changes(&fork),
        [("a.txt".to_owned(), "Modified".to_owned())]
    );
    // The task forked from is as it was.
    assert_eq!(read(&copy, "a.txt").as_deref(), Some("a2\n"));

    // At the start: the fork holds what the folder holds.
    let start = s.task.fork(Some(0)).unwrap();
    assert!(changes(&start).is_empty(), "{:?}", changes(&start));
    assert!(s.task.fork(Some(7)).is_err());
}

#[test]
fn a_fork_commits_to_the_folder_with_the_real_secret() {
    let s = Setup::new();
    let fork = s.task.fork(None).unwrap();
    let masked = read(&fork.copy(), ".env").unwrap();
    write(&fork.copy(), ".env", &format!("{masked}PORT=3000\n"));
    write(&fork.copy(), "a.txt", "from the fork\n");
    fork.commit().unwrap();
    assert_eq!(
        read(s.ws.path(), "a.txt").as_deref(),
        Some("from the fork\n")
    );
    assert_eq!(
        read(s.ws.path(), ".env").unwrap(),
        format!("{ENV}PORT=3000\n")
    );
}
