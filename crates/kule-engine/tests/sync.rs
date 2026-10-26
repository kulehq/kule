//! The folder's own changes brought into a task's copy: what only the
//! folder changed comes in, what both changed is merged, and when anything
//! does not merge, nothing comes in.

use std::fs;
use std::path::Path;
use std::process::Command;

use kule_engine::{Error, Synced, Task};
use tempfile::TempDir;

const CODE: &str = "impl Task {\n    fn commit(&self) {\n        1;\n    }\n\n    fn undo(&self) {\n        2;\n    }\n}\n";

struct Setup {
    _home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        Setup::with(|_| {})
    }

    fn with(prepare: impl FnOnce(&Path)) -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        write(&ws.path().join("task.rs"), CODE);
        write(&ws.path().join("notes.txt"), "a\nb\nc\n");
        write(&ws.path().join("docs/guide.md"), "# Guide\n");
        prepare(ws.path());
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            ws,
            task,
        }
    }

    fn folder(&self, file: &str) -> String {
        fs::read_to_string(self.ws.path().join(file)).unwrap()
    }

    fn copy(&self, file: &str) -> String {
        fs::read_to_string(self.task.copy().join(file)).unwrap()
    }

    fn in_folder(&self, file: &str, text: &str) {
        write(&self.ws.path().join(file), text);
    }

    fn in_copy(&self, file: &str, text: &str) {
        write(&self.task.copy().join(file), text);
    }
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn both_changed(result: kule_engine::Result<Synced>) -> Vec<String> {
    match result {
        Err(Error::BothChanged(paths)) => paths,
        other => panic!("not refused: {other:?}"),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn what_only_the_folder_changed_comes_into_the_copy() {
    let s = Setup::new();
    s.in_folder("new.txt", "new\n");
    s.in_folder("notes.txt", "a\nb\nC\n");
    fs::remove_dir_all(s.ws.path().join("docs")).unwrap();

    let behind = s.task.behind().unwrap();
    assert_eq!(
        behind.changed,
        ["docs", "docs/guide.md", "new.txt", "notes.txt"]
    );
    assert!(behind.both.is_empty());

    let synced = s.task.sync().unwrap();
    assert_eq!(
        synced.brought,
        ["docs", "docs/guide.md", "new.txt", "notes.txt"]
    );
    assert_eq!(s.copy("new.txt"), "new\n");
    assert_eq!(s.copy("notes.txt"), "a\nb\nC\n");
    assert!(!s.task.copy().join("docs").exists());
    // The person's work is not the task's: nothing to commit, nothing behind.
    assert!(s.task.effects().unwrap().is_empty());
    assert!(s.task.behind().unwrap().is_empty());
    assert_eq!(s.task.sync().unwrap(), Synced::default());
}

#[test]
fn the_tasks_later_change_to_a_brought_file_keeps_the_persons() {
    let s = Setup::new();
    s.in_folder("notes.txt", "a\nb\nC\n");
    s.task.sync().unwrap();
    s.in_copy("notes.txt", "A\nb\nC\n");
    s.task.commit().unwrap().unwrap();
    assert_eq!(s.folder("notes.txt"), "A\nb\nC\n");
}

#[test]
fn a_file_both_changed_apart_is_merged_and_stays_the_tasks_change() {
    let s = Setup::new();
    s.in_copy("task.rs", &CODE.replace("1;", "10;"));
    s.in_folder("task.rs", &CODE.replace("2;", "20;"));

    let behind = s.task.behind().unwrap();
    assert_eq!(behind.both, ["task.rs"]);

    let synced = s.task.sync().unwrap();
    assert_eq!(synced.merged, ["task.rs"]);
    let both = CODE.replace("1;", "10;").replace("2;", "20;");
    assert_eq!(s.copy("task.rs"), both);
    assert_eq!(s.folder("task.rs"), CODE.replace("2;", "20;"));
    assert_eq!(s.task.effects().unwrap().paths(), ["task.rs"]);

    let checkpoint = s.task.commit().unwrap().unwrap();
    assert!(checkpoint.merged.is_empty(), "{:?}", checkpoint.merged);
    assert_eq!(s.folder("task.rs"), both);
}

#[test]
fn what_both_changed_in_one_method_brings_nothing_in() {
    let s = Setup::new();
    s.in_copy("task.rs", &CODE.replace("1;", "10;"));
    s.in_folder("task.rs", &CODE.replace("1;", "11;"));
    s.in_folder("new.txt", "new\n");

    assert_eq!(s.task.behind().unwrap().both, ["task.rs"]);
    let paths = both_changed(s.task.sync());
    assert_eq!(paths, ["task.rs (both changed method Task::commit)"]);
    assert_eq!(s.copy("task.rs"), CODE.replace("1;", "10;"));
    assert!(!s.task.copy().join("new.txt").exists());
    assert_eq!(s.folder("task.rs"), CODE.replace("1;", "11;"));
}

#[test]
fn a_folder_the_task_deleted_does_not_come_back_with_a_new_file() {
    let s = Setup::new();
    fs::remove_dir_all(s.task.copy().join("docs")).unwrap();
    s.in_folder("docs/more.md", "more\n");

    assert_eq!(both_changed(s.task.sync()), ["docs/more.md"]);
    assert!(!s.task.copy().join("docs").exists());
}

#[test]
fn a_folder_deleted_in_the_folder_keeps_the_tasks_new_file() {
    let s = Setup::new();
    s.in_copy("docs/more.md", "more\n");
    fs::remove_dir_all(s.ws.path().join("docs")).unwrap();

    let paths = both_changed(s.task.sync());
    assert!(paths.contains(&"docs".to_owned()), "{paths:?}");
    assert_eq!(s.copy("docs/more.md"), "more\n");
}

#[test]
fn the_same_change_on_both_sides_is_no_change_of_the_tasks() {
    let s = Setup::new();
    s.in_copy("notes.txt", "a\nb\nC\n");
    s.in_folder("notes.txt", "a\nb\nC\n");
    s.task.sync().unwrap();
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_folder_file_touched_but_not_changed_is_not_behind() {
    let s = Setup::new();
    let file = s.ws.path().join("notes.txt");
    let later = filetime::FileTime::from_unix_time(4_000_000_000, 0);
    filetime::set_file_mtime(&file, later).unwrap();
    assert!(s.task.behind().unwrap().is_empty());
    s.task.sync().unwrap();
    assert!(s.task.behind().unwrap().is_empty());
}

#[test]
fn settings_files_from_the_folder_come_in_masked() {
    let s = Setup::with(|ws| write(&ws.join(".env"), "PORT=3000\n"));
    s.in_folder(".env", "PORT=3000\nSTRIPE_KEY=sk_live_51Hreal\n");
    s.in_folder("config/.env.local", "DB_PASSWORD=hunter2\n");

    let synced = s.task.sync().unwrap();
    assert_eq!(synced.brought, [".env", "config", "config/.env.local"]);
    let (env, local) = (s.copy(".env"), s.copy("config/.env.local"));
    assert!(
        !env.contains("sk_live") && env.contains("kule_secret_"),
        "{env}"
    );
    assert!(
        !local.contains("hunter2") && local.contains("kule_secret_"),
        "{local}"
    );
    assert!(s.task.effects().unwrap().is_empty());

    // The task's change takes the real values to the folder.
    s.in_copy(".env", &format!("{env}DEBUG=1\n"));
    s.task.commit().unwrap().unwrap();
    assert_eq!(
        s.folder(".env"),
        "PORT=3000\nSTRIPE_KEY=sk_live_51Hreal\nDEBUG=1\n"
    );
}

#[test]
fn a_settings_file_both_changed_merges_with_the_real_values() {
    let s = Setup::with(|ws| write(&ws.join(".env"), "PORT=3000\nSTRIPE_KEY=sk_live_51Hreal\n"));
    let masked = s.copy(".env");
    s.in_copy(".env", &format!("{masked}DEBUG=1\n"));
    s.in_folder(".env", "PORT=4000\nSTRIPE_KEY=sk_live_51Hreal\n");

    assert_eq!(s.task.sync().unwrap().merged, [".env"]);
    let env = s.copy(".env");
    assert!(
        env.starts_with("PORT=4000\n") && env.ends_with("DEBUG=1\n"),
        "{env}"
    );
    assert!(!env.contains("sk_live"), "{env}");
    s.task.commit().unwrap().unwrap();
    assert_eq!(
        s.folder(".env"),
        "PORT=4000\nSTRIPE_KEY=sk_live_51Hreal\nDEBUG=1\n"
    );
}

#[test]
fn the_agent_hears_once_what_came_in() {
    let s = Setup::new();
    s.in_folder("new.txt", "new\n");
    s.in_copy("task.rs", &CODE.replace("1;", "10;"));
    s.in_folder("task.rs", &CODE.replace("2;", "20;"));
    s.task.sync().unwrap();

    let note = s.task.take_sync_note().unwrap().unwrap();
    assert!(note.contains("`new.txt`"), "{note}");
    assert!(
        note.contains("merged") && note.contains("`task.rs`"),
        "{note}"
    );
    assert_eq!(s.task.take_sync_note().unwrap(), None);
}

#[cfg(unix)]
#[test]
fn a_path_in_the_note_cannot_end_its_code_span_or_line() {
    let s = Setup::new();
    s.in_folder("a`b\nc.txt", "x\n");
    s.task.sync().unwrap();
    let note = s.task.take_sync_note().unwrap().unwrap();
    assert!(note.contains("`a?b?c.txt`"), "{note}");
    assert!(!note.contains('\n'), "{note}");
}

#[test]
fn steps_start_over_after_a_sync() {
    let s = Setup::new();
    s.in_copy("notes.txt", "A\nb\nc\n");
    s.task.step("one").unwrap();
    assert_eq!(s.task.steps().unwrap().len(), 1);
    s.in_folder("new.txt", "new\n");
    s.task.sync().unwrap();
    assert!(s.task.steps().unwrap().is_empty());
    assert_eq!(s.task.effects().unwrap().paths(), ["notes.txt"]);
}

#[test]
fn a_commit_in_the_folder_reaches_the_copys_repository() {
    let s = Setup::with(|ws| {
        git(ws, &["init", "-q"]);
        git(ws, &["add", "-A"]);
        git(ws, &["commit", "-qm", "initial"]);
    });
    s.in_folder("notes.txt", "a\nb\nC\n");
    git(s.ws.path(), &["commit", "-qam", "person"]);
    // The task looked at its repository meanwhile.
    git(&s.task.copy(), &["status", "--short"]);

    let behind = s.task.behind().unwrap();
    assert!(behind.repository);
    assert_eq!(behind.changed, ["notes.txt"]);

    let synced = s.task.sync().unwrap();
    assert!(synced.repository);
    let set = s.task.effects().unwrap();
    assert!(set.is_empty(), "{:?}", set.paths());
    let head = git(s.ws.path(), &["rev-parse", "HEAD"]);
    assert_eq!(git(&s.task.copy(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&s.task.copy(), &["status", "--short"]), "");
}

#[test]
fn a_branch_both_moved_brings_nothing_in() {
    let s = Setup::with(|ws| {
        git(ws, &["init", "-q"]);
        git(ws, &["add", "-A"]);
        git(ws, &["commit", "-qm", "initial"]);
    });
    s.in_folder("notes.txt", "a\nb\nC\n");
    git(s.ws.path(), &["commit", "-qam", "person"]);
    s.in_copy("task.rs", &CODE.replace("1;", "10;"));
    git(&s.task.copy(), &["commit", "-qam", "task"]);

    let paths = both_changed(s.task.sync());
    assert!(
        paths.contains(&".git/refs/heads/main".to_owned()),
        "{paths:?}"
    );
    assert_ne!(
        git(&s.task.copy(), &["rev-parse", "HEAD"]),
        git(s.ws.path(), &["rev-parse", "HEAD"])
    );
}

#[test]
fn output_the_folder_ignores_stays_the_tasks_when_both_changed_it() {
    let s = Setup::with(|ws| {
        git(ws, &["init", "-q"]);
        write(&ws.join(".gitignore"), "build/\n");
        write(&ws.join("build/out.bin"), "old\n");
        git(ws, &["add", "-A"]);
        git(ws, &["commit", "-qm", "initial"]);
    });
    s.in_copy("build/out.bin", "\0task\n");
    s.in_folder("build/out.bin", "\0person\n");
    s.in_folder("new.txt", "new\n");

    let synced = s.task.sync().unwrap();
    assert_eq!(synced.kept, ["build/out.bin"]);
    assert_eq!(synced.brought, ["new.txt"]);
    assert_eq!(s.copy("build/out.bin"), "\0task\n");
}

#[test]
fn a_task_on_a_folder_that_is_gone_brings_nothing_in() {
    let s = Setup::new();
    let moved = TempDir::new().unwrap();
    fs::rename(s.ws.path(), moved.path().join("ws")).unwrap();
    let err = s.task.sync().unwrap_err().to_string();
    assert!(err.contains("is gone"), "{err}");
    assert_eq!(s.copy("notes.txt"), "a\nb\nc\n");
    fs::rename(moved.path().join("ws"), s.ws.path()).unwrap();
}
