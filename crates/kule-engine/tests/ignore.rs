//! Paths the workspace's ignore rules exclude stay in the copy unless a
//! commit asks for them. Repositories are made by the real `git`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use kule_engine::{Error, Task};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

struct Setup {
    _home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    /// A repository ignoring `__pycache__/`, `.env` and `*.log`, with a
    /// tracked `keep.log` added on purpose.
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        write(&ws.path().join(".gitignore"), "__pycache__/\n.env\n*.log\n");
        write(
            &ws.path().join("calc.py"),
            "def add(a, b):\n    return a - b\n",
        );
        write(&ws.path().join("keep.log"), "tracked\n");
        git(ws.path(), &["init", "-q"]);
        git(ws.path(), &["add", ".gitignore", "calc.py"]);
        git(ws.path(), &["add", "-f", "keep.log"]);
        git(ws.path(), &["commit", "-q", "-m", "start"]);
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            ws,
            task,
        }
    }

    fn copy(&self, rel: &str) -> PathBuf {
        self.task.copy().join(rel)
    }

    fn work(&self, rel: &str) -> PathBuf {
        self.ws.path().join(rel)
    }

    /// The changed paths outside `.git`, each with whether it is ignored.
    fn marked(&self) -> Vec<(String, bool)> {
        self.task
            .effects()
            .unwrap()
            .files()
            .map(|e| (e.path.clone(), e.ignored))
            .collect()
    }
}

#[test]
fn ignored_paths_stay_in_the_copy_until_asked_for() {
    let s = Setup::new();
    write(&s.copy("calc.py"), "def add(a, b):\n    return a + b\n");
    write(&s.copy("__pycache__/calc.pyc"), "bytecode");
    write(&s.copy("__pycache__/deep/more.txt"), "text");
    write(&s.copy(".env"), "SECRET=1\n");
    write(&s.copy("run.log"), "log\n");
    assert_eq!(
        s.marked(),
        [
            (".env".to_owned(), true),
            ("__pycache__".to_owned(), true),
            ("__pycache__/calc.pyc".to_owned(), true),
            ("__pycache__/deep".to_owned(), true),
            ("__pycache__/deep/more.txt".to_owned(), true),
            ("calc.py".to_owned(), false),
            ("run.log".to_owned(), true),
        ]
    );

    let checkpoint = s.task.commit().unwrap().unwrap();
    assert_eq!(checkpoint.effects.len(), 1);
    assert!(
        fs::read_to_string(s.work("calc.py"))
            .unwrap()
            .contains("a + b")
    );
    for left in [".env", "__pycache__", "run.log"] {
        assert!(!s.work(left).exists(), "{left} reached the workspace");
    }
    // Still there, still uncommitted.
    assert_eq!(s.marked().len(), 6);
    assert!(s.marked().iter().all(|(_, ignored)| *ignored));
    assert!(s.task.commit().unwrap().is_none());

    s.task.commit_with(true, |_| Ok(())).unwrap().unwrap();
    assert_eq!(fs::read_to_string(s.work(".env")).unwrap(), "SECRET=1\n");
    assert!(s.work("__pycache__/calc.pyc").exists());
    assert!(s.marked().is_empty());
}

#[test]
fn a_tracked_file_is_never_ignored() {
    let s = Setup::new();
    write(&s.copy("keep.log"), "changed\n");
    assert_eq!(s.marked(), [("keep.log".to_owned(), false)]);
}

#[test]
fn the_rules_come_from_the_workspace_not_the_copy() {
    let s = Setup::new();
    // An agent trying to keep a new file out of the commit.
    write(&s.copy(".gitignore"), "__pycache__/\n.env\n*.log\nnew.py\n");
    write(&s.copy("new.py"), "print('hi')\n");
    assert_eq!(
        s.marked(),
        [
            (".gitignore".to_owned(), false),
            ("new.py".to_owned(), false)
        ]
    );
}

#[test]
fn a_workspace_without_a_repository_ignores_nothing() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    write(&ws.path().join(".gitignore"), "*.log\n");
    let task = Task::create(home.path(), ws.path()).unwrap();
    write(&task.copy().join("run.log"), "log\n");
    let effects = task.effects().unwrap();
    assert!(effects.files().all(|e| !e.ignored));
}

#[test]
fn undo_leaves_uncommitted_ignored_paths_alone_and_refuses_to_overwrite_them() {
    let s = Setup::new();
    write(&s.copy("calc.py"), "def add(a, b):\n    return a + b\n");
    write(&s.copy("run.log"), "log\n");
    s.task.commit().unwrap().unwrap();
    // run.log is not part of the commit: undo goes ahead and keeps it.
    s.task.undo().unwrap().unwrap();
    assert_eq!(fs::read_to_string(s.copy("run.log")).unwrap(), "log\n");

    write(&s.copy(".env"), "A=1\n");
    s.task.commit_with(true, |_| Ok(())).unwrap().unwrap();
    write(&s.copy(".env"), "A=2\n");
    match s.task.undo() {
        Err(Error::Invalid(message)) => assert!(message.contains(".env"), "{message}"),
        other => panic!("expected undo to refuse, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(s.copy(".env")).unwrap(), "A=2\n");
}

/// A repository with `pkg/a.rs` tracked and an ignored `pkg/.env` beside it.
fn with_package() -> Setup {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    write(&ws.path().join(".gitignore"), ".env\n");
    write(&ws.path().join("pkg/a.rs"), "fn a() {}\n");
    write(&ws.path().join("pkg/.env"), "SECRET=1\n");
    git(ws.path(), &["init", "-q"]);
    git(ws.path(), &["add", ".gitignore", "pkg/a.rs"]);
    git(ws.path(), &["commit", "-q", "-m", "start"]);
    let task = Task::create(home.path(), ws.path()).unwrap();
    Setup {
        _home: home,
        ws,
        task,
    }
}

#[test]
fn deleting_a_directory_takes_its_ignored_files_into_the_checkpoint() {
    let s = with_package();
    fs::remove_dir_all(s.copy("pkg")).unwrap();
    let checkpoint = s.task.commit().unwrap().unwrap();
    let paths: Vec<&str> = checkpoint.effects.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, ["pkg", "pkg/.env", "pkg/a.rs"]);
    assert!(!s.work("pkg").exists());
    assert!(s.marked().is_empty());

    s.task.undo().unwrap().unwrap();
    assert_eq!(
        fs::read_to_string(s.work("pkg/.env")).unwrap(),
        "SECRET=1\n"
    );
    assert_eq!(
        fs::read_to_string(s.work("pkg/a.rs")).unwrap(),
        "fn a() {}\n"
    );
}

#[test]
fn deleting_a_directory_does_not_take_a_persons_later_edit_to_an_ignored_file() {
    let s = with_package();
    write(&s.work("pkg/.env"), "SECRET=changed by a person, longer\n");
    fs::remove_dir_all(s.copy("pkg")).unwrap();
    match s.task.commit() {
        Err(Error::Conflict(paths)) => assert_eq!(paths, ["pkg/.env"]),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert!(s.work("pkg/.env").exists());
}
