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
        Err(Error::Conflict(paths)) => assert_eq!(paths, ["src/main.rs"]),
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
        Err(Error::Conflict(paths)) => assert_eq!(paths, ["busy.txt"]),
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
        let writer = {
            let (tasks, a) = (home.path().join("tasks"), a.clone());
            std::thread::spawn(move || {
                loop {
                    let cloned = fs::read_dir(&tasks)
                        .into_iter()
                        .flatten()
                        .flatten()
                        .any(|t| t.path().join("copy/a.txt").exists());
                    if cloned {
                        fs::write(&a, "2\n").unwrap();
                        return;
                    }
                    std::thread::yield_now();
                }
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
    let lock = s.task.copy().parent().unwrap().join("lock");
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
