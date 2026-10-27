//! Values Kule masks that the folder's git repository holds anyway: found
//! wherever git keeps them, and nothing reads the copy until the person
//! accepts that for the folder. Repositories are made by the real `git`.

use std::fs;
use std::path::Path;
use std::process::Command;

use kule_engine::{Task, forget_committed};
use tempfile::TempDir;

const TOKEN: &str = "ghp_K1x9cQ7mTz4Lr2Wv8Nb5Hy3Ds6Fp0Ja1Ue";

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

/// A folder that is a repository with one commit of a plain file.
fn repository() -> TempDir {
    let ws = TempDir::new().unwrap();
    git(ws.path(), &["init", "-q"]);
    fs::write(ws.path().join("app.txt"), "app\n").unwrap();
    git(ws.path(), &["add", "app.txt"]);
    git(ws.path(), &["commit", "-qm", "app"]);
    ws
}

fn env_with_token(dir: &Path) {
    fs::write(dir.join(".env"), format!("API_TOKEN={TOKEN}\n")).unwrap();
}

/// The `.env` git no longer tracks, as a person leaves it after taking it
/// out of the repository: ignored, the value still in the folder.
fn untrack_env(dir: &Path) {
    git(dir, &["rm", "-q", "--cached", ".env"]);
    fs::write(dir.join(".gitignore"), ".env\n").unwrap();
    git(dir, &["add", ".gitignore"]);
}

/// Whether the task lets nothing read its copy, saying it is git.
fn held(task: &Task) -> bool {
    match task.agent_at_work() {
        Ok(_) => false,
        Err(e) => {
            let e = e.to_string();
            assert!(e.contains("git"), "refused for another reason: {e}");
            assert!(!e.contains(TOKEN), "the refusal shows the value: {e}");
            true
        }
    }
}

/// Escape attempt (#46): a value Kule masks in the copy's `.env`, read
/// through git from the commit that holds it. Outside Kule the copy hands
/// it over; in the task nothing starts (no agent, no terminal) until the
/// person accepts it for the folder, and then each run is told. Breaks when
/// the copy's repository is not looked into before anything reads it.
#[test]
fn escape_secrets_a_committed_value_through_git() {
    let home = TempDir::new().unwrap();
    let ws = repository();
    env_with_token(ws.path());
    git(ws.path(), &["add", ".env"]);
    git(ws.path(), &["commit", "-qm", "env"]);
    let task = Task::create(home.path(), ws.path()).unwrap();
    let copy = fs::read_to_string(task.copy().join(".env")).unwrap();
    assert!(!copy.contains(TOKEN), "the copy is not masked: {copy}");
    assert!(
        git(&task.copy(), &["show", "HEAD:.env"]).contains(TOKEN),
        "the attempt does not reach the value even outside Kule"
    );

    assert!(held(&task), "an agent started with the value in git");
    let e = task.terminal_opened().err().map(|e| e.to_string());
    assert!(
        e.as_deref().is_some_and(|e| e.contains("git")),
        "a terminal opened with the value in git: {e:?}"
    );
    assert!(task.start_notes().unwrap().is_empty());

    let look = task.accept_committed().unwrap();
    assert!(look.found.iter().any(|f| f.path == ".env"), "{look:?}");
    assert!(look.found.iter().all(|f| !f.value.contains(TOKEN)));
    let at_work = task.agent_at_work().unwrap();
    let notes = task.start_notes().unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.contains(".env") && n.contains("git")),
        "the acceptance is not told: {notes:?}"
    );
    drop(at_work);

    // Taken back, and the task holds again.
    assert!(forget_committed(home.path(), ws.path()).unwrap());
    assert!(held(&task));
}

/// The common way out of a committed `.env`: untracked and ignored, its
/// value unchanged. The commit that added it still holds it, in a clone
/// too, where no log names it and every object is packed.
#[test]
fn a_value_left_in_an_older_commit_is_found() {
    let home = TempDir::new().unwrap();
    let origin = repository();
    env_with_token(origin.path());
    git(origin.path(), &["add", ".env"]);
    git(origin.path(), &["commit", "-qm", "env"]);
    untrack_env(origin.path());
    git(origin.path(), &["commit", "-qm", "untrack"]);
    let parent = TempDir::new().unwrap();
    let from = origin.path().to_str().unwrap();
    git(parent.path(), &["clone", "-q", "--no-local", from, "ws"]);
    let ws = parent.path().join("ws");
    env_with_token(&ws);
    let task = Task::create(home.path(), &ws).unwrap();
    assert!(held(&task));
    let look = task.committed().unwrap();
    assert!(
        look.found
            .iter()
            .any(|f| f.path == ".env" && f.at.starts_with("commit ")),
        "{look:?}"
    );
}

/// A commit amended away is in no branch, but the reference's log keeps
/// it, and git shows it.
#[test]
fn a_value_amended_away_is_found_in_the_log() {
    let home = TempDir::new().unwrap();
    let ws = repository();
    env_with_token(ws.path());
    git(ws.path(), &["add", ".env"]);
    git(ws.path(), &["commit", "-qm", "env"]);
    untrack_env(ws.path());
    git(ws.path(), &["commit", "-q", "--amend", "-m", "no env"]);
    assert!(
        git(ws.path(), &["log", "--all", "--", ".env"]).is_empty(),
        "a branch still holds it"
    );
    let task = Task::create(home.path(), ws.path()).unwrap();
    assert!(held(&task));
    let look = task.committed().unwrap();
    assert!(
        look.found
            .iter()
            .any(|f| f.path == ".env" && f.at.starts_with("commit ")),
        "{look:?}"
    );
}

/// A value only added to the index, never committed, is in the index.
#[test]
fn a_value_only_staged_is_found() {
    let home = TempDir::new().unwrap();
    let ws = repository();
    env_with_token(ws.path());
    git(ws.path(), &["add", ".env"]);
    let task = Task::create(home.path(), ws.path()).unwrap();
    assert!(held(&task));
    let look = task.committed().unwrap();
    assert!(look.found.iter().any(|f| f.at == "the index"), "{look:?}");
}

/// A value stashed with the untracked files is in the stash's commits.
#[test]
fn a_value_in_a_stash_is_found() {
    let home = TempDir::new().unwrap();
    let ws = repository();
    env_with_token(ws.path());
    git(ws.path(), &["stash", "-q", "-u"]);
    env_with_token(ws.path());
    fs::write(ws.path().join(".gitignore"), ".env\n").unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    assert!(held(&task));
}

/// What git keeps outside every commit and log, loose or in a cruft pack,
/// has no name: it is looked into whole.
#[test]
fn a_value_in_an_object_outside_the_history_is_found() {
    for cruft in [false, true] {
        let home = TempDir::new().unwrap();
        let ws = repository();
        env_with_token(ws.path());
        fs::write(ws.path().join(".gitignore"), ".env\n").unwrap();
        git(ws.path(), &["hash-object", "-w", ".env"]);
        if cruft {
            git(ws.path(), &["repack", "-q", "-d", "--cruft"]);
        }
        let task = Task::create(home.path(), ws.path()).unwrap();
        assert!(held(&task), "cruft: {cruft}");
        let look = task.committed().unwrap();
        assert!(
            look.found
                .iter()
                .any(|f| f.at == "an object outside the history"),
            "cruft: {cruft}: {look:?}"
        );
    }
}

/// A value never committed, beside a committed `.env` that holds another
/// one (the shared example a team commits), changes nothing: commands
/// start, and nothing is told.
#[test]
fn a_value_never_committed_changes_nothing() {
    let home = TempDir::new().unwrap();
    let ws = repository();
    fs::write(
        ws.path().join(".env"),
        "API_TOKEN=change-me-in-your-own-copy\n",
    )
    .unwrap();
    git(ws.path(), &["add", ".env"]);
    git(ws.path(), &["commit", "-qm", "example"]);
    env_with_token(ws.path());
    let task = Task::create(home.path(), ws.path()).unwrap();
    let at_work = task.agent_at_work().unwrap();
    assert!(task.start_notes().unwrap().is_empty());
    drop(at_work);
    assert!(task.committed().unwrap().clean());
}

/// What was accepted is the values found then: a value committed since
/// asks again.
#[test]
fn a_value_found_after_the_acceptance_asks_again() {
    let home = TempDir::new().unwrap();
    let ws = repository();
    env_with_token(ws.path());
    git(ws.path(), &["add", ".env"]);
    git(ws.path(), &["commit", "-qm", "env"]);
    let task = Task::create(home.path(), ws.path()).unwrap();
    task.accept_committed().unwrap();
    drop(task.agent_at_work().unwrap());

    fs::write(
        ws.path().join(".env"),
        format!("API_TOKEN={TOKEN}\nDEPLOY_KEY=sk_live_9fQ2xLm7Rt4Vb8Nc1Zp6\n"),
    )
    .unwrap();
    git(ws.path(), &["commit", "-qam", "deploy key"]);
    task.sync().unwrap();
    assert!(held(&task), "a value committed since ran unasked");
}
