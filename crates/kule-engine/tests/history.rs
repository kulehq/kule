//! M3: what a commit replaced lives in the task's Lonca store, and every
//! commit and undo is an entry in a signed chain.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;

use kule_engine::{HistoryEvent, Sandbox, Task};
use tempfile::TempDir;

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

struct Setup {
    home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        write(&ws.path().join("a.txt"), "a\n");
        write(&ws.path().join("bin/run.sh"), "#!/bin/sh\n");
        fs::set_permissions(
            ws.path().join("bin/run.sh"),
            fs::Permissions::from_mode(0o750),
        )
        .unwrap();
        write(&ws.path().join("ro.txt"), "read only\n");
        fs::set_permissions(ws.path().join("ro.txt"), fs::Permissions::from_mode(0o444)).unwrap();
        symlink("a.txt", ws.path().join("link")).unwrap();
        let init = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(ws.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap();
        assert!(init.success());
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup { home, ws, task }
    }

    fn work(&self, rel: &str) -> std::path::PathBuf {
        self.ws.path().join(rel)
    }

    fn copy(&self, rel: &str) -> std::path::PathBuf {
        self.task.copy().join(rel)
    }
}

#[test]
fn undo_brings_back_modes_symlinks_and_odd_paths_from_the_store() {
    let s = Setup::new();
    let stamp = |p: &Path| {
        let m = fs::symlink_metadata(p).unwrap();
        (m.permissions().mode() & 0o7777, m.modified().unwrap())
    };
    let before = [stamp(&s.work("bin/run.sh")), stamp(&s.work("ro.txt"))];
    fs::write(s.copy("bin/run.sh"), "#!/bin/sh\necho changed\n").unwrap();
    fs::remove_file(s.copy("ro.txt")).unwrap();
    fs::remove_file(s.copy("link")).unwrap();
    symlink("elsewhere", s.copy("link")).unwrap();
    write(&s.copy(".git/HEAD"), "ref: refs/heads/other\n");
    let checkpoint = s.task.commit().unwrap().unwrap();
    assert!(checkpoint.snapshot.is_some());
    // Nothing of it sits in a plain directory any more.
    let dir = s
        .home
        .path()
        .join("tasks")
        .join(s.task.id())
        .join("checkpoints");
    assert!(
        !dir.join(format!("{:06}", checkpoint.id))
            .join("before")
            .exists()
    );
    assert!(dir.join("store.git").is_dir());

    s.task.undo().unwrap().unwrap();
    assert_eq!(
        fs::read_to_string(s.work("bin/run.sh")).unwrap(),
        "#!/bin/sh\n"
    );
    assert_eq!(fs::read_to_string(s.work("ro.txt")).unwrap(), "read only\n");
    assert_eq!(
        [stamp(&s.work("bin/run.sh")), stamp(&s.work("ro.txt"))],
        before
    );
    assert_eq!(fs::read_link(s.work("link")).unwrap(), Path::new("a.txt"));
    assert_eq!(
        fs::read_to_string(s.work(".git/HEAD")).unwrap(),
        "ref: refs/heads/main\n"
    );
    // The laid-out directory is gone once undo is done with it.
    assert!(
        !dir.join(format!("{:06}", checkpoint.id))
            .join("before")
            .exists()
    );
}

#[test]
fn the_history_records_commits_and_undos_in_order() {
    let s = Setup::new();
    assert!(s.task.history().unwrap().is_empty());
    write(&s.copy("b.txt"), "b\n");
    let first = s.task.commit().unwrap().unwrap();
    write(&s.copy("c.txt"), "c\n");
    s.task.commit().unwrap().unwrap();
    s.task.undo().unwrap().unwrap();

    let history = s.task.history().unwrap();
    let events: Vec<_> = history.iter().map(|e| (e.seq, e.event.clone())).collect();
    assert_eq!(events.len(), 3);
    assert!(matches!(
        &events[0],
        (0, HistoryEvent::Commit { checkpoint, changes: 1, snapshot, .. })
            if *checkpoint == first.id && Some(snapshot) == first.snapshot.as_ref()
    ));
    assert!(matches!(
        events[1],
        (1, HistoryEvent::Commit { checkpoint: 2, .. })
    ));
    assert!(matches!(
        events[2],
        (2, HistoryEvent::Undo { checkpoint: 2 })
    ));
    assert_eq!(history[1].prev.len(), 64);
}

/// Rewrites the newest entry's file in the store with `git`, the way someone
/// with write access but without the key would.
fn forge(store: &Path, edit: impl Fn(&str) -> String) {
    let git = |args: &[&str], input: Option<&str>| -> String {
        use std::io::Write;
        let mut child = Command::new("git")
            .args(["--git-dir", store.to_str().unwrap()])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "x")
            .env("GIT_AUTHOR_EMAIL", "x@x")
            .env("GIT_COMMITTER_NAME", "x")
            .env("GIT_COMMITTER_EMAIL", "x@x")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(input) = input {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let head = git(&["rev-parse", "refs/kule/log"], None);
    let parent = git(&["rev-parse", &format!("{head}^")], None);
    let entry = git(&["cat-file", "blob", &format!("{head}:entry.json")], None);
    let blob = git(&["hash-object", "-w", "--stdin"], Some(&edit(&entry)));
    let tree = git(
        &["mktree"],
        Some(&format!("100644 blob {blob}\tentry.json\n")),
    );
    let commit = git(&["commit-tree", &tree, "-p", &parent, "-m", "forged"], None);
    git(&["update-ref", "refs/kule/log", &commit], None);
}

#[test]
fn a_changed_entry_breaks_the_history() {
    let s = Setup::new();
    write(&s.copy("b.txt"), "b\n");
    s.task.commit().unwrap().unwrap();
    write(&s.copy("c.txt"), "c\n");
    s.task.commit().unwrap().unwrap();
    assert_eq!(s.task.history().unwrap().len(), 2);

    let store = s
        .home
        .path()
        .join("tasks")
        .join(s.task.id())
        .join("checkpoints/store.git");
    forge(&store, |entry| {
        entry.replace("\"changes\":1", "\"changes\":7")
    });
    let e = s.task.history().unwrap_err().to_string();
    assert!(e.contains("does not verify"), "{e}");
}

#[test]
fn the_signing_key_is_hidden_from_what_a_task_runs() {
    let s = Setup::new();
    write(&s.copy("b.txt"), "b\n");
    s.task.commit().unwrap().unwrap();
    let key = s.home.path().join("kule.key");
    assert!(key.is_file());

    let out = Sandbox::new(&s.task)
        .unwrap()
        .command("cat", &[key.to_string_lossy().into_owned()])
        .unwrap()
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}
