//! Secrets in the folder's settings files: masked in the copy, real in the
//! folder, through every way the copy and the folder sync.

use std::fs;

use kule_engine::{Task, add_secret};
use tempfile::TempDir;

const ENV: &str = "PORT=3000\nSTRIPE_KEY=sk_live_51Hreal\nDB_PASSWORD='hunter2'\n";

struct Setup {
    home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        fs::write(ws.path().join(".env"), ENV).unwrap();
        fs::write(ws.path().join("app.txt"), "app\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup { home, ws, task }
    }

    fn copy_env(&self) -> String {
        fs::read_to_string(self.task.copy().join(".env")).unwrap()
    }

    fn folder_env(&self) -> String {
        fs::read_to_string(self.ws.path().join(".env")).unwrap()
    }
}

/// The copy's `.env` with each placeholder in it, in order.
fn placeholders(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with("kule_secret_"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_copy_holds_placeholders_and_shows_no_change() {
    let s = Setup::new();
    let copy = s.copy_env();
    let p = placeholders(&copy);
    assert_eq!(p.len(), 2, "{copy}");
    assert_eq!(
        copy,
        format!("PORT=3000\nSTRIPE_KEY={}\nDB_PASSWORD='{}'\n", p[0], p[1])
    );
    assert!(!copy.contains("sk_live") && !copy.contains("hunter2"));
    assert_eq!(s.folder_env(), ENV);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_commit_takes_the_real_values_to_the_folder_and_masks_the_copy_again() {
    let s = Setup::new();
    let masked = s.copy_env();
    fs::write(s.task.copy().join(".env"), format!("{masked}NEW_FLAG=1\n")).unwrap();
    fs::write(s.task.copy().join("app.txt"), "app 2\n").unwrap();
    let set = s.task.effects().unwrap();
    assert_eq!(set.paths(), [".env", "app.txt"]);

    s.task.commit().unwrap().unwrap();
    assert_eq!(s.folder_env(), format!("{ENV}NEW_FLAG=1\n"));
    assert_eq!(s.copy_env(), format!("{masked}NEW_FLAG=1\n"));
    assert!(s.task.effects().unwrap().is_empty());

    // Undo brings the folder's file back, and the copy stays masked.
    s.task.undo().unwrap().unwrap();
    assert_eq!(s.folder_env(), ENV);
    assert_eq!(s.copy_env(), masked);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_rollback_masks_what_it_brings_back() {
    let s = Setup::new();
    let masked = s.copy_env();
    fs::write(
        s.task.copy().join(".env"),
        "STRIPE_KEY=typed-by-the-agent\n",
    )
    .unwrap();
    s.task.rollback().unwrap();
    assert_eq!(s.copy_env(), masked);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_change_in_the_folder_merges_with_the_real_values() {
    let s = Setup::new();
    let masked = s.copy_env();
    // The agent adds a line at the end; the person rotates the key.
    fs::write(s.task.copy().join(".env"), format!("{masked}NEW_FLAG=1\n")).unwrap();
    let rotated = ENV.replace("sk_live_51Hreal", "sk_live_51Hnew");
    fs::write(s.ws.path().join(".env"), &rotated).unwrap();
    s.task.commit().unwrap().unwrap();
    assert_eq!(s.folder_env(), format!("{rotated}NEW_FLAG=1\n"));
    assert!(!s.copy_env().contains("sk_live"));
}

#[test]
fn a_kept_secret_in_the_folder_gets_its_own_placeholder() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    add_secret(
        home.path(),
        "STRIPE_KEY",
        "sk_live_51Hreal",
        &["api.stripe.com".into()],
        None,
    )
    .unwrap();
    // Not a name or a value Kule would take for a secret on its own.
    fs::write(ws.path().join(".env"), "PAYMENTS=sk_live_51Hreal\n").unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let env = task.vault().unwrap().env();
    assert_eq!(env.len(), 1);
    let copy = fs::read_to_string(task.copy().join(".env")).unwrap();
    assert_eq!(copy, format!("PAYMENTS={}\n", env[0].1));
}

#[test]
fn the_values_are_kept_where_no_sandbox_reads_them() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    let dir = s.home.path().join("tasks").join(s.task.id());
    let kept = dir.join("secrets.json");
    assert_eq!(
        fs::metadata(&kept).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!kept.starts_with(s.task.copy()));
}

#[test]
fn a_secret_typed_into_the_copy_is_masked_before_an_agent_starts() {
    let s = Setup::new();
    let masked = s.copy_env();
    // A person adds a key in the copy, from a terminal outside the sandbox.
    fs::write(
        s.task.copy().join(".env"),
        format!("{masked}GITHUB_TOKEN=ghp_typedByThePerson12345\n"),
    )
    .unwrap();
    let at_work = s.task.agent_at_work().unwrap();
    let copy = s.copy_env();
    assert!(!copy.contains("ghp_"), "{copy}");
    assert_eq!(placeholders(&copy).len(), 3);
    drop(at_work);
    // Committed, the folder gets the key the person typed.
    s.task.commit().unwrap().unwrap();
    assert_eq!(
        s.folder_env(),
        format!("{ENV}GITHUB_TOKEN=ghp_typedByThePerson12345\n")
    );
}
