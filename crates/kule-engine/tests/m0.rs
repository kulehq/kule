//! Milestone M0: copy, effects, commit, rollback and undo on a real filesystem.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;

use kule_engine::{Change, Error, Kind, Task};
use tempfile::TempDir;
use walkdir::WalkDir;

/// What a tree looks like to someone who reads it: type, permission bits,
/// and the bytes or link target of every entry.
type Snapshot = BTreeMap<String, (Kind, u32, Vec<u8>)>;

fn snapshot(root: &Path) -> Snapshot {
    let mut snap = Snapshot::new();
    for entry in WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry = entry.unwrap();
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let meta = entry.path().symlink_metadata().unwrap();
        let ft = meta.file_type();
        let value = if ft.is_symlink() {
            let target = fs::read_link(entry.path()).unwrap();
            (
                Kind::Symlink,
                0,
                target.to_str().unwrap().as_bytes().to_vec(),
            )
        } else if ft.is_dir() {
            (Kind::Dir, meta.mode() & 0o7777, Vec::new())
        } else {
            (
                Kind::File,
                meta.mode() & 0o7777,
                fs::read(entry.path()).unwrap(),
            )
        };
        snap.insert(rel, value);
    }
    snap
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// A small repository with the shapes that tend to break copies: nested
/// directories, a binary file, an empty file, an executable, a symlink and an
/// empty directory.
fn fixture(root: &Path) {
    write(&root.join("README.md"), "hello\n");
    write(&root.join("src/main.rs"), "fn main() {}\n");
    write(
        &root.join("src/lib.rs"),
        "pub fn add(a: u8, b: u8) -> u8 { a + b }\n",
    );
    write(&root.join("src/empty.txt"), "");
    let binary: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
    write(&root.join("src/nested/deep/data.bin"), binary);
    fs::create_dir_all(root.join("src/empty-dir")).unwrap();
    write(&root.join("run.sh"), "#!/bin/sh\necho hi\n");
    chmod(&root.join("run.sh"), 0o755);
    symlink("src/main.rs", root.join("main-link")).unwrap();
}

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        fixture(ws.path());
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            task,
        }
    }

    fn work(&self) -> std::path::PathBuf {
        self.task.workspace().to_path_buf()
    }

    fn copy(&self) -> std::path::PathBuf {
        self.task.copy()
    }
}

/// The edits most tests make in the copy: one of each kind of change.
fn edit(copy: &Path) {
    write(&copy.join("src/new.rs"), "// new\n");
    write(
        &copy.join("src/main.rs"),
        "fn main() { println!(\"hi\"); }\n",
    );
    fs::remove_file(copy.join("README.md")).unwrap();
    chmod(&copy.join("run.sh"), 0o644);
    fs::remove_file(copy.join("main-link")).unwrap();
    symlink("src/lib.rs", copy.join("main-link")).unwrap();
    fs::create_dir(copy.join("docs")).unwrap();
}

fn changes(task: &Task) -> Vec<(String, Change)> {
    task.effects()
        .unwrap()
        .effects
        .into_iter()
        .map(|e| (e.path, e.change))
        .collect()
}

#[test]
fn the_copy_matches_the_workspace() {
    let s = Setup::new();
    assert_eq!(snapshot(&s.copy()), snapshot(&s.work()));
    assert!(s.task.effects().unwrap().is_empty());
}

/// M0's acceptance test.
#[test]
fn rm_rf_src_then_rollback_restores_every_byte() {
    let s = Setup::new();
    let pristine = snapshot(&s.work());

    fs::remove_dir_all(s.copy().join("src")).unwrap();

    let set = s.task.effects().unwrap();
    let deleted: Vec<&str> = set.effects.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        deleted,
        [
            "src",
            "src/empty-dir",
            "src/empty.txt",
            "src/lib.rs",
            "src/main.rs",
            "src/nested",
            "src/nested/deep",
            "src/nested/deep/data.bin"
        ]
    );
    assert!(set.effects.iter().all(|e| e.change == Change::Deleted));
    assert_eq!(
        snapshot(&s.work()),
        pristine,
        "the workspace must not notice"
    );

    s.task.rollback().unwrap();

    assert_eq!(snapshot(&s.copy()), pristine);
    assert_eq!(snapshot(&s.work()), pristine);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn effects_report_each_kind_of_change() {
    let s = Setup::new();
    edit(&s.copy());
    assert_eq!(
        changes(&s.task),
        [
            ("README.md".into(), Change::Deleted),
            ("docs".into(), Change::Created),
            ("main-link".into(), Change::Modified),
            ("run.sh".into(), Change::Modified),
            ("src/main.rs".into(), Change::Modified),
            ("src/new.rs".into(), Change::Created),
        ]
    );
}

#[test]
fn rewriting_the_same_bytes_is_not_a_change() {
    let s = Setup::new();
    let main = s.copy().join("src/main.rs");
    let before = fs::metadata(&main).unwrap();
    write(&main, "fn main() {}\n");
    write(
        &s.copy().join("src/lib.rs"),
        "pub fn add(a: u8, b: u8) -> u8 { a - b }\n",
    );
    // The stamp moved, so the byte comparison is what decides.
    let after = fs::metadata(&main).unwrap();
    assert_ne!(
        (before.ctime(), before.ctime_nsec()),
        (after.ctime(), after.ctime_nsec())
    );
    assert_eq!(changes(&s.task), [("src/lib.rs".into(), Change::Modified)]);
}

#[test]
fn commit_makes_the_workspace_match_the_copy() {
    let s = Setup::new();
    edit(&s.copy());
    let cp = s.task.commit().unwrap().expect("a checkpoint");
    assert_eq!(cp.effects.len(), 6);
    assert_eq!(snapshot(&s.work()), snapshot(&s.copy()));
    assert!(s.task.effects().unwrap().is_empty());
    assert!(s.task.commit().unwrap().is_none());
}

/// Build tools decide what to rebuild from mtimes. Directories are created
/// fresh, so only Kule can carry their times over; files keep theirs through
/// the clone on APFS but not through a plain copy.
#[test]
fn the_copy_keeps_modification_times() {
    let s = Setup::new();
    for rel in [
        "src",
        "src/nested/deep",
        "src/empty-dir",
        "src/main.rs",
        "run.sh",
    ] {
        let (w, c) = (
            fs::symlink_metadata(s.work().join(rel)).unwrap(),
            fs::symlink_metadata(s.copy().join(rel)).unwrap(),
        );
        assert_eq!(
            (w.mtime(), w.mtime_nsec()),
            (c.mtime(), c.mtime_nsec()),
            "{rel}"
        );
    }
}

/// Adding or removing a child moves its parent's mtime. After a rollback the
/// parents of what came back (the root included) must carry the workspace's
/// times again.
#[test]
fn rollback_restores_parent_directory_times() {
    let s = Setup::new();
    fs::remove_dir_all(s.copy().join("src/nested")).unwrap();
    write(&s.copy().join("extra.txt"), "x\n");
    s.task.rollback().unwrap();
    for rel in ["", "src"] {
        let (w, c) = (
            fs::metadata(s.work().join(rel)).unwrap(),
            fs::metadata(s.copy().join(rel)).unwrap(),
        );
        assert_eq!(
            (w.mtime(), w.mtime_nsec()),
            (c.mtime(), c.mtime_nsec()),
            "{rel:?}"
        );
    }
}

#[test]
fn undo_restores_parent_directory_times() {
    let s = Setup::new();
    let times = |root: &Path| -> Vec<(i64, i64)> {
        ["", "src"]
            .iter()
            .map(|rel| {
                let m = fs::metadata(root.join(rel)).unwrap();
                (m.mtime(), m.mtime_nsec())
            })
            .collect()
    };
    let before = times(&s.work());
    fs::remove_dir_all(s.copy().join("src/nested")).unwrap();
    write(&s.copy().join("docs/guide.md"), "# Guide\n");
    s.task.commit().unwrap();
    assert_ne!(times(&s.work()), before);
    s.task.undo().unwrap();
    assert_eq!(times(&s.work()), before);
    assert_eq!(times(&s.copy()), before);
}

#[test]
fn commit_keeps_modification_times() {
    let s = Setup::new();
    write(&s.copy().join("src/new.rs"), "// new\n");
    s.task.commit().unwrap();
    for rel in ["src/new.rs", "src/main.rs", "src/nested/deep/data.bin"] {
        let (w, c) = (
            fs::metadata(s.work().join(rel)).unwrap(),
            fs::metadata(s.copy().join(rel)).unwrap(),
        );
        assert_eq!(
            (w.mtime(), w.mtime_nsec()),
            (c.mtime(), c.mtime_nsec()),
            "{rel}"
        );
    }
}

#[test]
fn rollback_after_every_kind_of_change() {
    let s = Setup::new();
    let pristine = snapshot(&s.work());
    edit(&s.copy());
    let set = s.task.rollback().unwrap();
    assert_eq!(set.effects.len(), 6);
    assert_eq!(snapshot(&s.copy()), pristine);
    assert_eq!(snapshot(&s.work()), pristine);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn commit_refuses_when_the_workspace_changed_underneath() {
    let s = Setup::new();
    write(&s.copy().join("src/main.rs"), "fn main() { /* agent */ }\n");
    write(&s.work().join("src/main.rs"), "fn main() { /* human */ }\n");

    match s.task.commit() {
        Err(Error::Conflict(paths)) => {
            assert_eq!(paths, ["src/main.rs (both changed function main)"])
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(s.work().join("src/main.rs")).unwrap(),
        "fn main() { /* human */ }\n"
    );
    assert_eq!(
        fs::read_to_string(s.copy().join("src/main.rs")).unwrap(),
        "fn main() { /* agent */ }\n"
    );
}

#[test]
fn commit_refuses_to_delete_a_directory_holding_unknown_files() {
    let s = Setup::new();
    fs::remove_dir_all(s.copy().join("src/nested")).unwrap();
    write(&s.work().join("src/nested/human-notes.txt"), "keep me\n");

    match s.task.commit() {
        Err(Error::Conflict(paths)) => {
            assert!(paths.contains(&"src/nested".to_owned()), "{paths:?}")
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert!(s.work().join("src/nested/human-notes.txt").exists());
}

#[test]
fn undo_restores_the_workspace_and_the_copy() {
    let s = Setup::new();
    let pristine = snapshot(&s.work());
    edit(&s.copy());
    fs::remove_dir_all(s.copy().join("src/nested")).unwrap();
    s.task.commit().unwrap();
    assert_ne!(snapshot(&s.work()), pristine);

    let cp = s.task.undo().unwrap().expect("an undone checkpoint");
    assert_eq!(cp.id, 1);
    assert_eq!(snapshot(&s.work()), pristine);
    assert_eq!(snapshot(&s.copy()), pristine);
    assert!(s.task.effects().unwrap().is_empty());
    assert!(s.task.undo().unwrap().is_none());
}

#[test]
fn repeated_undo_walks_back_one_commit_at_a_time() {
    let s = Setup::new();
    let v0 = snapshot(&s.work());
    write(&s.copy().join("a.txt"), "one\n");
    s.task.commit().unwrap();
    let v1 = snapshot(&s.work());
    write(&s.copy().join("a.txt"), "two\n");
    s.task.commit().unwrap();

    assert_eq!(s.task.undo().unwrap().unwrap().id, 2);
    assert_eq!(snapshot(&s.work()), v1);
    assert_eq!(s.task.undo().unwrap().unwrap().id, 1);
    assert_eq!(snapshot(&s.work()), v0);
}

#[test]
fn undo_refuses_with_uncommitted_changes() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    s.task.commit().unwrap();
    write(&s.copy().join("b.txt"), "pending\n");
    assert!(matches!(s.task.undo(), Err(Error::Invalid(_))));
    assert!(s.work().join("a.txt").exists());
}

#[test]
fn undo_refuses_when_the_workspace_changed_after_the_commit() {
    let s = Setup::new();
    write(&s.copy().join("src/main.rs"), "fn main() { /* agent */ }\n");
    s.task.commit().unwrap();
    write(&s.work().join("src/main.rs"), "fn main() { /* human */ }\n");

    match s.task.undo() {
        Err(Error::Conflict(paths)) => assert_eq!(paths, ["src/main.rs"]),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(s.work().join("src/main.rs")).unwrap(),
        "fn main() { /* human */ }\n"
    );
}

#[test]
fn undo_refuses_to_remove_a_directory_holding_unknown_files() {
    let s = Setup::new();
    write(&s.copy().join("docs/guide.md"), "# Guide\n");
    s.task.commit().unwrap();
    write(&s.work().join("docs/human-notes.md"), "keep me\n");

    match s.task.undo() {
        Err(Error::Conflict(paths)) => assert_eq!(paths, ["docs"]),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert!(s.work().join("docs/human-notes.md").exists());
}

/// A rollback makes a person's file known to the baseline; undo must still
/// not take it away with the directory the commit created.
#[test]
fn undo_keeps_a_persons_file_the_baseline_knows_from_a_rollback() {
    let s = Setup::new();
    write(&s.copy().join("docs/guide.md"), "# Guide\n");
    s.task.commit().unwrap();
    write(&s.work().join("docs/notes.md"), "a person's notes\n");
    write(
        &s.copy().join("docs/notes.md"),
        "the agent's notes, longer\n",
    );
    assert!(matches!(s.task.commit(), Err(Error::Conflict(_))));
    s.task.rollback().unwrap();

    match s.task.undo() {
        Err(Error::Conflict(paths)) => assert_eq!(paths, ["docs"]),
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(
        fs::read_to_string(s.work().join("docs/notes.md")).unwrap(),
        "a person's notes\n"
    );
}

#[test]
fn type_changes_commit_and_roll_back() {
    let s = Setup::new();
    let pristine = snapshot(&s.work());
    let swap = |copy: &Path| {
        fs::remove_file(copy.join("README.md")).unwrap();
        write(&copy.join("README.md/index.md"), "now a directory\n");
        fs::remove_dir_all(copy.join("src/nested")).unwrap();
        write(&copy.join("src/nested"), "now a file\n");
    };

    swap(&s.copy());
    s.task.rollback().unwrap();
    assert_eq!(snapshot(&s.copy()), pristine);

    swap(&s.copy());
    s.task.commit().unwrap();
    assert_eq!(snapshot(&s.work()), snapshot(&s.copy()));
    s.task.undo().unwrap();
    assert_eq!(snapshot(&s.work()), pristine);
    assert_eq!(snapshot(&s.copy()), pristine);
}

#[test]
fn read_only_directories_survive_the_round_trip() {
    let s = Setup::new();
    let pristine = snapshot(&s.work());
    fs::remove_dir_all(s.copy().join("src/nested")).unwrap();
    s.task.commit().unwrap();
    s.task.undo().unwrap();
    assert_eq!(snapshot(&s.work()), pristine);

    // A directory made read-only in the copy is committed with its mode.
    chmod(&s.copy().join("src/empty-dir"), 0o555);
    s.task.commit().unwrap();
    assert_eq!(
        fs::metadata(s.work().join("src/empty-dir")).unwrap().mode() & 0o777,
        0o555
    );
}

/// Background processes (git maintenance, IDE indexers, build daemons) create
/// and delete files while Kule walks the tree. An entry that disappears
/// between listing and `lstat` is simply gone, not an error.
#[test]
fn effects_survive_files_vanishing_during_the_scan() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let s = Setup::new();
    let churn = s.copy().join("churn");
    fs::create_dir(&churn).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let (churn, stop) = (churn.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                for i in 0..50 {
                    let _ = fs::write(churn.join(format!("tmp-{i}.lock")), "x");
                }
                for i in 0..50 {
                    let _ = fs::remove_file(churn.join(format!("tmp-{i}.lock")));
                }
            }
        })
    };
    let result: Result<(), Error> = (0..300).try_for_each(|_| s.task.effects().map(drop));
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    result.unwrap();
}

/// A workspace where something keeps writing (an IDE, a watcher) must still
/// be copyable: each entry is taken as it stood when it was cloned.
#[test]
fn a_task_can_be_made_while_files_churn() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    fixture(ws.path());
    for i in 0..300 {
        write(
            &ws.path().join(format!("bulk/file-{i}.txt")),
            format!("{i}\n"),
        );
    }
    let churn = ws.path().join("churn");
    fs::create_dir(&churn).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let worker = {
        let (churn, stop) = (churn.clone(), stop.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                for i in 0..20 {
                    let _ = fs::write(churn.join(format!("tmp-{i}")), "x");
                }
                for i in 0..20 {
                    let _ = fs::remove_file(churn.join(format!("tmp-{i}")));
                }
            }
        })
    };
    let made: Result<Vec<Task>, Error> = (0..5)
        .map(|_| Task::create(home.path(), ws.path()))
        .collect();
    stop.store(true, Ordering::Relaxed);
    worker.join().unwrap();
    for task in made.unwrap() {
        assert!(task.effects().unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(task.copy().join("bulk/file-7.txt")).unwrap(),
            "7\n"
        );
    }
}

/// A file someone keeps writing while the task is made (an editor saving
/// over and over) may be copied mid-stream. Whatever the copy got, a change
/// the task makes to that file must not overwrite the human's later writes.
#[test]
fn a_file_written_during_creation_conflicts_if_the_task_changes_it() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    fixture(ws.path());
    let busy = ws.path().join("busy.txt");
    write(&busy, "0\n");
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (busy, stop) = (busy.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut i = 0u64;
            while !stop.load(Ordering::Relaxed) {
                i += 1;
                let _ = fs::write(&busy, format!("{i}\n"));
            }
        })
    };
    let task = Task::create(home.path(), ws.path());
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    let task = task.unwrap();
    // The human's last save, the same size as whatever the copy got (which
    // may be an empty file caught mid-write).
    let copied = fs::read_to_string(task.copy().join("busy.txt")).unwrap();
    write(&busy, "h".repeat(copied.len().max(1)));

    write(&task.copy().join("busy.txt"), "agent\n");
    match task.commit() {
        // With what both changed when Kule could tell what they began from.
        Err(Error::Conflict(paths)) => {
            assert!(
                paths.len() == 1 && paths[0].starts_with("busy.txt"),
                "{paths:?}"
            )
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_ne!(fs::read_to_string(&busy).unwrap(), "agent\n");
}

// Filesystems keep time in ticks (milliseconds on Linux ext4, two seconds on
// FAT), so a same-size write right after Kule reads a stamp can leave the
// stamp as it was. Each test below repeats a race that a coarse clock loses
// often; on a nanosecond clock (APFS) they pass either way.
const SAME_TICK_TRIES: usize = 20;

#[test]
fn a_same_size_write_right_after_creation_is_not_overwritten() {
    for _ in 0..SAME_TICK_TRIES {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        write(&ws.path().join("a.txt"), "1\n");
        let task = Task::create(home.path(), ws.path()).unwrap();
        write(&ws.path().join("a.txt"), "2\n");
        write(&task.copy().join("a.txt"), "agent\n");
        assert!(matches!(task.commit(), Err(Error::Conflict(_))));
        assert_eq!(fs::read_to_string(ws.path().join("a.txt")).unwrap(), "2\n");
    }
}

#[test]
fn a_same_size_write_during_creation_is_not_overwritten() {
    for _ in 0..SAME_TICK_TRIES {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        let a = ws.path().join("a.txt");
        write(&a, "1\n");
        for i in 0..500 {
            write(&ws.path().join(format!("b{i}.txt")), "b\n");
        }
        // Saves a.txt again as soon as its clone appears, while the rest of
        // the workspace is still being copied.
        // The copy is `<task>/w/<the workspace's name>`. Gives up after a
        // while, so a copy made elsewhere fails the test instead of hanging.
        let writer = {
            let (tasks, a) = (home.path().join("tasks"), a.clone());
            let name = ws.path().file_name().unwrap().to_owned();
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                while started.elapsed() < std::time::Duration::from_secs(30) {
                    let cloned = fs::read_dir(&tasks)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .any(|t| t.path().join("w").join(&name).join("a.txt").exists());
                    if cloned {
                        fs::write(&a, "2\n").unwrap();
                        return;
                    }
                    std::thread::yield_now();
                }
                panic!("the copy never appeared");
            })
        };
        let task = Task::create(home.path(), ws.path()).unwrap();
        writer.join().unwrap();
        let copied = fs::read_to_string(task.copy().join("a.txt")).unwrap();
        write(&task.copy().join("a.txt"), "agent\n");
        match task.commit() {
            Err(Error::Conflict(_)) => assert_eq!(fs::read_to_string(&a).unwrap(), "2\n"),
            Ok(_) => assert_eq!(copied, "2\n", "the human's save was overwritten unseen"),
            Err(e) => panic!("{e}"),
        }
    }
}

#[test]
fn a_same_size_write_right_after_a_commit_is_not_overwritten() {
    for _ in 0..SAME_TICK_TRIES {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        write(&ws.path().join("a.txt"), "1\n");
        let task = Task::create(home.path(), ws.path()).unwrap();
        write(&task.copy().join("a.txt"), "2\n");
        task.commit().unwrap().unwrap();
        write(&ws.path().join("a.txt"), "3\n");
        assert!(matches!(task.undo(), Err(Error::Conflict(_))));
        write(&task.copy().join("a.txt"), "4\n");
        assert!(matches!(task.commit(), Err(Error::Conflict(_))));
        assert_eq!(fs::read_to_string(ws.path().join("a.txt")).unwrap(), "3\n");
    }
}

#[test]
fn a_same_size_write_right_after_an_undo_is_not_overwritten() {
    for _ in 0..SAME_TICK_TRIES {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        write(&ws.path().join("a.txt"), "1\n");
        let task = Task::create(home.path(), ws.path()).unwrap();
        write(&task.copy().join("a.txt"), "2\n");
        task.commit().unwrap().unwrap();
        write(&task.copy().join("a.txt"), "3\n");
        task.commit().unwrap().unwrap();
        task.undo().unwrap().unwrap();
        write(&ws.path().join("a.txt"), "4\n");
        // A conflict; FAT, which renumbers a file on every write, already
        // reports the copy as out of sync.
        assert!(task.undo().is_err());
        assert_eq!(fs::read_to_string(ws.path().join("a.txt")).unwrap(), "4\n");
    }
}

#[test]
fn a_same_size_write_to_the_copy_right_after_a_rollback_is_seen() {
    for _ in 0..SAME_TICK_TRIES {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        write(&ws.path().join("a.txt"), "1\n");
        let task = Task::create(home.path(), ws.path()).unwrap();
        write(&task.copy().join("a.txt"), "2\n");
        // Rollback keeps the workspace's modification time, so the copy's
        // stamp is fresh only when the workspace's file is.
        write(&ws.path().join("a.txt"), "5\n");
        task.rollback().unwrap();
        write(&task.copy().join("a.txt"), "3\n");
        let paths: Vec<String> = task
            .effects()
            .unwrap()
            .effects
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(paths, ["a.txt"]);
    }
}

#[test]
fn a_held_lock_turns_writers_away() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    // The copy is `<task>/w/<name>`.
    let lock = s.task.copy().ancestors().nth(2).unwrap().join("lock");
    write(&lock, "12345\n");
    assert!(matches!(s.task.commit(), Err(Error::Busy(_))));
    assert!(!s.work().join("a.txt").exists());
    fs::remove_file(&lock).unwrap();
    assert!(s.task.commit().unwrap().is_some());
}

#[test]
fn a_home_inside_the_workspace_is_refused() {
    let ws = TempDir::new().unwrap();
    fixture(ws.path());
    let home = ws.path().join(".kule");
    assert!(matches!(
        Task::create(&home, ws.path()),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn a_task_reopens_from_its_home() {
    let s = Setup::new();
    write(&s.copy().join("src/new.rs"), "// new\n");
    let home = s._home.path();
    let again = Task::open(home, s.task.id()).unwrap();
    assert_eq!(changes(&again), [("src/new.rs".into(), Change::Created)]);
    assert_eq!(Task::list(home).unwrap().len(), 1);
}

#[test]
fn steps_record_what_each_tool_call_changed() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    let first = s.task.step("write a").unwrap();
    assert_eq!(first.id, 1);
    let paths = |step: &kule_engine::Step| -> Vec<(String, Option<Change>)> {
        step.changes
            .iter()
            .map(|c| (c.path.clone(), c.change))
            .collect()
    };
    assert_eq!(paths(&first), [("a.txt".into(), Some(Change::Created))]);

    write(
        &s.copy().join("src/main.rs"),
        "fn main() { /* edited */ }\n",
    );
    write(&s.copy().join("a.txt"), "two\n");
    let second = s.task.step("edit two files").unwrap();
    assert_eq!(
        paths(&second),
        [
            ("a.txt".into(), Some(Change::Created)),
            ("src/main.rs".into(), Some(Change::Modified))
        ]
    );

    // Putting a file back the way the workspace has it shows up as undone.
    write(&s.copy().join("src/main.rs"), "fn main() {}\n");
    let third = s.task.step("revert main").unwrap();
    assert_eq!(paths(&third), [("src/main.rs".into(), None)]);

    let nothing = s.task.step("look around").unwrap();
    assert!(nothing.changes.is_empty());
    assert_eq!(s.task.steps().unwrap().len(), 4);

    s.task.commit().unwrap();
    assert!(s.task.steps().unwrap().is_empty());
    assert_eq!(s.task.step("after commit").unwrap().id, 1);
}

/// Module caches and build directories are often read-only (`chmod 555`);
/// rollback, commit and undo still get through them.
#[test]
fn read_only_directories_do_not_stop_rollback_commit_or_undo() {
    let s = Setup::new();
    let ro = |p: &Path, mode| fs::set_permissions(p, fs::Permissions::from_mode(mode)).unwrap();

    // The agent fills a cache and makes it read-only: rollback still works.
    write(&s.copy().join("cache/x"), "x\n");
    ro(&s.copy().join("cache"), 0o555);
    s.task.rollback().unwrap();
    assert!(!s.copy().join("cache").exists());

    // A read-only directory in the workspace: commit can delete it, undo
    // brings it back, mode included.
    write(&s.work().join("vendor/lib.txt"), "lib\n");
    ro(&s.work().join("vendor"), 0o555);
    let home = TempDir::new().unwrap();
    let task = Task::create(home.path(), &s.work()).unwrap();
    ro(&task.copy().join("vendor"), 0o755);
    fs::remove_dir_all(task.copy().join("vendor")).unwrap();
    task.commit().unwrap().unwrap();
    assert!(!s.work().join("vendor").exists());
    task.undo().unwrap().unwrap();
    assert_eq!(
        fs::read_to_string(s.work().join("vendor/lib.txt")).unwrap(),
        "lib\n"
    );
    let mode = fs::metadata(s.work().join("vendor")).unwrap().mode() & 0o777;
    assert_eq!(mode, 0o555);
    ro(&s.work().join("vendor"), 0o755);
    ro(&task.copy().join("vendor"), 0o755);
}

/// A commit that fails halfway puts the workspace back and leaves no
/// interrupted checkpoint in undo's way.
#[test]
fn a_commit_that_fails_halfway_leaves_nothing_behind() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    let first = s.task.commit().unwrap().unwrap();
    let before = snapshot(&s.work());

    // a.txt goes in first; then zz.txt cannot be read from the copy.
    write(&s.copy().join("a.txt"), "two\n");
    write(&s.copy().join("zz.txt"), "secret\n");
    fs::set_permissions(s.copy().join("zz.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    let result = s.task.commit();
    assert!(result.is_err(), "{result:?}");
    assert_eq!(snapshot(&s.work()), before);

    // Undo still reaches the commit before.
    fs::set_permissions(s.copy().join("zz.txt"), fs::Permissions::from_mode(0o644)).unwrap();
    s.task.rollback().unwrap();
    assert_eq!(s.task.undo().unwrap().unwrap().id, first.id);
}

/// An agent at work, started by any process, holds commit, rollback, undo
/// and remove off; a mark left by a process that died does not.
#[test]
fn an_agent_at_work_holds_off_commit_rollback_undo_and_remove() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    let at_work = s.task.agent_at_work().unwrap();
    assert!(s.task.agent_at_work().is_err());
    let busy = |r: Result<(), Error>| match r {
        Err(Error::Invalid(m)) => assert!(m.contains("at work"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    };
    busy(s.task.commit().map(|_| ()));
    busy(s.task.rollback().map(|_| ()));
    busy(s.task.undo().map(|_| ()));
    busy(s.task.clone().remove());
    drop(at_work);
    assert!(s.task.commit().unwrap().is_some());

    let mark = s.task.copy().ancestors().nth(2).unwrap().join("agent");
    fs::write(&mark, "999999\n").unwrap();
    assert_eq!(s.task.agent_pid(), None);
    write(&s.copy().join("b.txt"), "two\n");
    assert!(s.task.commit().unwrap().is_some());
}

/// A removed task leaves the list at once; its files, read-only
/// directories too, go when the trash is emptied, which may come later.
#[test]
fn a_removed_task_goes_to_the_trash_then_off_the_disk() {
    let s = Setup::new();
    let home = s._home.path().to_path_buf();
    let locked = s.copy().join("src/locked");
    fs::create_dir_all(&locked).unwrap();
    write(&locked.join("f.txt"), "x");
    chmod(&locked, 0o555);
    let id = s.task.id().to_owned();

    s.task.clone().remove().unwrap();
    assert!(Task::list(&home).unwrap().is_empty());
    assert!(Task::open(&home, &id).is_err());
    assert_eq!(fs::read_dir(home.join("trash")).unwrap().count(), 1);

    Task::empty_trash(&home).unwrap();
    assert_eq!(fs::read_dir(home.join("trash")).unwrap().count(), 0);
    // Nothing to do is not an error.
    Task::empty_trash(&home).unwrap();
}

/// A workspace moved away is not rebuilt out of the task's changes.
#[test]
fn commit_and_undo_refuse_when_the_workspace_is_gone() {
    let s = Setup::new();
    write(&s.copy().join("new.txt"), "x");
    let moved = s.work().with_extension("moved");
    fs::rename(s.work(), &moved).unwrap();
    assert!(!s.task.workspace_exists());
    for refused in [s.task.commit().map(|_| ()), s.task.undo().map(|_| ())] {
        match refused {
            Err(Error::Invalid(m)) => assert!(m.contains("is gone"), "{m}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
    assert!(!s.work().exists());
    assert!(s.copy().join("new.txt").exists());
    fs::rename(&moved, s.work()).unwrap();
}

/// A task opens only by an id Kule could have made, and only where its
/// folder holds that task: a folder named otherwise, or holding another
/// task's file, is no task.
#[test]
fn a_task_opens_only_by_the_id_it_was_made_with() {
    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    assert!(Task::open(home.path(), task.id()).is_ok());
    let tasks = home.path().join("tasks");
    let file = tasks.join(task.id()).join("task.json");
    for id in ["x\")(allow default", "..", "a.b", "a-b"] {
        let dir = tasks.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::copy(&file, dir.join("task.json")).unwrap();
        assert!(Task::open(home.path(), id).is_err(), "{id}");
    }
    let other = tasks.join("abc123");
    fs::create_dir_all(&other).unwrap();
    fs::copy(&file, other.join("task.json")).unwrap();
    assert!(Task::open(home.path(), "abc123").is_err());
}

/// A terminal Kule opened on the copy holds off an agent's work, commit and
/// sync, which would end what runs in it; a mark left by a process that
/// died does not. While an agent is at work, no terminal opens.
#[test]
fn an_open_terminal_holds_off_work_commit_and_sync() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    let open = s.task.terminal_opened().unwrap();
    let held = |r: Result<(), Error>, doing: &str| match r {
        Err(Error::Invalid(m)) => {
            assert!(m.contains("terminal is open") && m.contains(doing), "{m}")
        }
        other => panic!("expected a refusal, got {other:?}"),
    };
    held(s.task.agent_at_work().map(|_| ()), "start work");
    held(s.task.commit().map(|_| ()), "commit");
    held(s.task.sync().map(|_| ()), "sync");
    assert_eq!(s.task.terminals_open(), [std::process::id()]);
    drop(open);
    assert!(s.task.terminals_open().is_empty());
    assert!(s.task.commit().unwrap().is_some());

    let marks = s.task.copy().ancestors().nth(2).unwrap().join("terminals");
    fs::write(marks.join("999999-1"), "999999\n").unwrap();
    assert!(s.task.terminals_open().is_empty());
    let at_work = s.task.agent_at_work().unwrap();
    let e = s.task.terminal_opened().map(|_| ()).unwrap_err();
    assert!(e.to_string().contains("agent is at work"), "{e}");
    drop(at_work);
    assert!(s.task.terminal_opened().is_ok());

    // While Kule writes in the copy, under the task's lock, none opens
    // either: it would start after the commit looked for terminals.
    let lock = marks.parent().unwrap().join("lock");
    fs::write(&lock, "1\n").unwrap();
    let e = s.task.terminal_opened().map(|_| ()).unwrap_err();
    assert!(
        e.to_string()
            .contains("Kule is writing in this task's copy"),
        "{e}"
    );
    assert!(s.task.terminals_open().is_empty());
    fs::remove_file(&lock).unwrap();
    assert!(s.task.terminal_opened().is_ok());
}

/// Rolling back, undoing and going back to a step write in the copy, as a
/// sync does: while a terminal Kule opened is open on it, they refuse and
/// leave the copy and the folder as they are.
#[test]
fn an_open_terminal_holds_off_rollback_undo_and_going_back() {
    let s = Setup::new();
    write(&s.copy().join("a.txt"), "one\n");
    assert!(s.task.commit().unwrap().is_some());
    write(&s.copy().join("b.txt"), "two\n");
    s.task.step("b").unwrap();
    let open = s.task.terminal_opened().unwrap();
    let held = |r: Result<(), Error>, doing: &str| match r {
        Err(Error::Invalid(m)) => {
            assert!(m.contains("terminal is open") && m.contains(doing), "{m}")
        }
        other => panic!("expected a refusal, got {other:?}"),
    };
    held(s.task.rollback().map(|_| ()), "roll back");
    // Refused before what changed since the last step is kept as one.
    write(&s.copy().join("c.txt"), "three\n");
    let steps = s.task.steps().unwrap().len();
    held(s.task.back_to_step(0).map(|_| ()), "go back to a step");
    assert_eq!(s.task.steps().unwrap().len(), steps);
    fs::remove_file(s.copy().join("c.txt")).unwrap();
    assert_eq!(fs::read_to_string(s.copy().join("b.txt")).unwrap(), "two\n");
    fs::remove_file(s.copy().join("b.txt")).unwrap();
    held(s.task.undo().map(|_| ()), "undo");
    assert_eq!(fs::read_to_string(s.work().join("a.txt")).unwrap(), "one\n");
    assert!(s.copy().join("a.txt").exists());

    drop(open);
    assert!(s.task.undo().unwrap().is_some());
    assert!(!s.work().join("a.txt").exists());
}

/// Kule cannot see a terminal app or an editor it opened the copy in close:
/// no agent starts work in the task until the person says it is closed.
#[test]
fn work_waits_until_what_was_opened_outside_is_closed() {
    let s = Setup::new();
    s.task.opened_outside("VS Code").unwrap();
    s.task.opened_outside("Terminal").unwrap();
    s.task.opened_outside("VS Code").unwrap();
    let opened: Vec<String> = s
        .task
        .outside()
        .unwrap()
        .into_iter()
        .map(|o| o.what)
        .collect();
    assert_eq!(opened, ["VS Code", "Terminal"]);
    let e = s.task.agent_at_work().map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("VS Code and Terminal"), "{e}");
    assert!(e.contains(&format!("kule closed {}", s.task.id())), "{e}");
    s.task.forget_outside().unwrap();
    assert!(s.task.outside().unwrap().is_empty());
    assert!(s.task.agent_at_work().is_ok());
}

/// A task's id names its processes on the whole computer (its sandbox's
/// mark), so tasks made at the same moment in different Kule homes never
/// share one.
#[test]
fn tasks_made_at_once_in_different_homes_have_different_ids() {
    let ws = TempDir::new().unwrap();
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let homes: Vec<TempDir> = (0..16).map(|_| TempDir::new().unwrap()).collect();
    let mut ids = std::collections::HashSet::new();
    let mut made = 0;
    for _ in 0..20 {
        let together = std::sync::Barrier::new(homes.len());
        let round: Vec<String> = std::thread::scope(|s| {
            let started: Vec<_> = homes
                .iter()
                .map(|home| {
                    let (together, ws) = (&together, ws.path());
                    s.spawn(move || {
                        together.wait();
                        Task::create(home.path(), ws).unwrap().id().to_owned()
                    })
                })
                .collect();
            started.into_iter().map(|t| t.join().unwrap()).collect()
        });
        made += round.len();
        ids.extend(round);
    }
    assert_eq!(ids.len(), made, "tasks made at once shared an id");
}

/// A failed run points to where its agent said why: the log, and the one
/// before it once the log was turned over during that run, never one an
/// earlier run left.
#[test]
fn a_run_error_points_to_the_older_log_too() {
    let (home, folder) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), folder.path()).unwrap();
    let log = task.agent_log();
    let started = std::time::SystemTime::now();
    assert_eq!(
        task.agent_log_hint(started),
        format!("see {}", log.display())
    );
    std::fs::write(log.with_extension("log.1"), "older\n").unwrap();
    assert_eq!(
        task.agent_log_hint(started),
        format!(
            "see {}, and agent.log.1 beside it for what came before",
            log.display()
        )
    );
    // Written before the run started: another run's.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
    assert_eq!(task.agent_log_hint(later), format!("see {}", log.display()));
}

/// A task whose record cannot be read is named apart, with why, for a list
/// a person reads; what counts every task (budgets, say) still fails rather
/// than leave it out.
#[test]
fn a_task_whose_record_cannot_be_read_is_named_apart() {
    let (home, folder) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), folder.path()).unwrap();
    let garbled = home.path().join("tasks/garbled");
    std::fs::create_dir_all(&garbled).unwrap();
    std::fs::write(garbled.join("task.json"), "{ not json").unwrap();
    let (infos, unreadable) = Task::list_told(home.path()).unwrap();
    assert_eq!(
        infos.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        [task.id()]
    );
    assert_eq!(unreadable.len(), 1);
    assert_eq!(unreadable[0].id, "garbled");
    // Why, without the home's path: it reaches extensions too.
    let why = &unreadable[0].why;
    assert!(why.contains("expected") || why.contains("key"), "{why}");
    assert!(!why.contains(home.path().to_str().unwrap()), "{why}");
    // What counts every task fails, naming it and what to do.
    let err = Task::list(home.path()).unwrap_err().to_string();
    assert!(
        err.contains("task garbled's record cannot be read"),
        "{err}"
    );
    assert!(err.contains("`kule rm garbled`"), "{err}");
    // Opened by its id, it says the same, without the home's path.
    let err = Task::open(home.path(), "garbled")
        .err()
        .unwrap()
        .to_string();
    assert!(
        err.contains("task garbled's record cannot be read"),
        "{err}"
    );
    assert!(err.contains("`kule rm garbled`"), "{err}");
    assert!(!err.contains(home.path().to_str().unwrap()), "{err}");
}

/// A task whose record cannot be read can be removed by its id, with its
/// copy; one that can be read is removed as any task is, and none is
/// removed while an agent is at work in it.
#[test]
fn a_task_whose_record_cannot_be_read_can_be_removed() {
    let (home, folder) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), folder.path()).unwrap();
    let id = task.id().to_owned();
    let garbled = home.path().join("tasks/garbled");
    std::fs::create_dir_all(garbled.join("w")).unwrap();
    std::fs::write(garbled.join("task.json"), "{ not json").unwrap();
    assert!(Task::remove_unreadable(home.path(), &id).is_err());
    assert!(Task::remove_unreadable(home.path(), "missing").is_err());
    // A folder there without a record is not a task, and stays.
    let other = home.path().join("tasks/other");
    std::fs::create_dir_all(&other).unwrap();
    assert!(Task::remove_unreadable(home.path(), "other").is_err());
    assert!(other.is_dir());
    std::fs::remove_dir(&other).unwrap();
    // An agent at work in it (this process, alive) holds it.
    std::fs::write(garbled.join("agent"), std::process::id().to_string()).unwrap();
    let err = Task::remove_unreadable(home.path(), "garbled").unwrap_err();
    assert!(err.to_string().contains("an agent is at work"), "{err}");
    std::fs::remove_file(garbled.join("agent")).unwrap();
    Task::remove_unreadable(home.path(), "garbled").unwrap();
    assert!(!garbled.exists());
    // Once it is gone, what counts every task goes on.
    let listed: Vec<String> = Task::list(home.path())
        .unwrap()
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(listed, std::slice::from_ref(&id));
    assert!(folder.path().is_dir());
    assert!(Task::open(home.path(), &id).is_ok());
}
