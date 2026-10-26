//! Going back to a step: the copy as it was after it, every later change
//! undone, the going back a step of its own; on the real filesystem.

use std::fs;
use std::path::Path;

use kule_engine::Task;
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        fs::write(ws.path().join("a.txt"), "a0\n").unwrap();
        fs::write(ws.path().join("gone.txt"), "gone\n").unwrap();
        fs::write(ws.path().join(".env"), "API_KEY=sk_live_51Hreal\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            ws,
            task,
        }
    }

    fn copy(&self) -> std::path::PathBuf {
        self.task.copy()
    }

    fn read(&self, rel: &str) -> Option<String> {
        fs::read_to_string(self.copy().join(rel)).ok()
    }
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn the_copy_goes_back_to_any_step_and_forward_again() {
    let s = Setup::new();
    let copy = s.copy();
    // Step 1: a.txt changed, b.txt and a directory created.
    write(&copy, "a.txt", "a1\n");
    write(&copy, "new/b.txt", "b1\n");
    s.task.step("one").unwrap();
    // Step 2: a.txt again, b.txt changed, gone.txt deleted.
    write(&copy, "a.txt", "a2\n");
    write(&copy, "new/b.txt", "b2\n");
    fs::remove_file(copy.join("gone.txt")).unwrap();
    s.task.step("two").unwrap();
    // Step 3: the directory goes, c.txt comes.
    fs::remove_dir_all(copy.join("new")).unwrap();
    write(&copy, "c.txt", "c3\n");
    s.task.step("three").unwrap();

    let back = s.task.back_to_step(1).unwrap();
    assert_eq!(back.label, "Back to step 1");
    assert_eq!(s.read("a.txt").as_deref(), Some("a1\n"));
    assert_eq!(s.read("new/b.txt").as_deref(), Some("b1\n"));
    assert_eq!(s.read("gone.txt").as_deref(), Some("gone\n"));
    assert_eq!(s.read("c.txt"), None);

    // Going back was a step, so step 3 is there to go to again.
    s.task.back_to_step(3).unwrap();
    assert_eq!(s.read("a.txt").as_deref(), Some("a2\n"));
    assert!(!copy.join("new").exists());
    assert_eq!(s.read("gone.txt"), None);
    assert_eq!(s.read("c.txt").as_deref(), Some("c3\n"));

    // Before the first step: the copy holds what the folder holds.
    s.task.back_to_step(0).unwrap();
    assert!(
        s.task.effects().unwrap().files().next().is_none(),
        "{:?}",
        s.task.effects().unwrap()
    );
    assert_eq!(s.read("a.txt").as_deref(), Some("a0\n"));
    // The folder never moved.
    assert_eq!(
        fs::read_to_string(s.ws.path().join("a.txt")).unwrap(),
        "a0\n"
    );
}

#[test]
fn changes_since_the_last_step_are_kept_before_going_back() {
    let s = Setup::new();
    write(&s.copy(), "a.txt", "a1\n");
    s.task.step("one").unwrap();
    // Changed after the last step, by a person's terminal say.
    write(&s.copy(), "a.txt", "unstepped\n");
    s.task.back_to_step(0).unwrap();
    let labels: Vec<String> = s
        .task
        .steps()
        .unwrap()
        .into_iter()
        .map(|s| s.label)
        .collect();
    assert_eq!(
        labels,
        [
            "one",
            "Changes before going back",
            "Back to before the first step"
        ]
    );
    // So the unstepped change can be gone back to.
    s.task.back_to_step(2).unwrap();
    assert_eq!(s.read("a.txt").as_deref(), Some("unstepped\n"));
}

#[test]
fn a_secret_stays_masked_on_the_way_back() {
    let s = Setup::new();
    let masked = s.read(".env").unwrap();
    assert!(!masked.contains("sk_live"), "{masked}");
    write(&s.copy(), ".env", "API_KEY=changed\n");
    s.task.step("one").unwrap();
    s.task.back_to_step(0).unwrap();
    let back = s.read(".env").unwrap();
    assert!(
        !back.contains("sk_live"),
        "the real secret came into the copy: {back}"
    );
    assert_eq!(back, masked);
}

#[test]
fn a_step_that_kept_no_copy_of_a_path_cannot_be_gone_back_to() {
    let s = Setup::new();
    let copy = s.copy();
    // A hard link to a file outside the copy is not kept: its content is
    // not the copy's.
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("secret"), "secret\n").unwrap();
    fs::hard_link(outside.path().join("secret"), copy.join("linked.txt")).unwrap();
    s.task.step("one").unwrap();
    fs::remove_file(copy.join("linked.txt")).unwrap();
    s.task.step("two").unwrap();
    let blockers = s.task.step_blockers().unwrap();
    assert_eq!(blockers[&1].as_deref(), Some("linked.txt"));
    assert_eq!(blockers[&0], None);
    assert_eq!(blockers[&2], None);
    let e = s.task.back_to_step(1).unwrap_err().to_string();
    assert!(e.contains("linked.txt"), "{e}");
    assert!(!copy.join("linked.txt").exists());
    assert!(s.task.back_to_step(9).is_err());
}

#[cfg(unix)]
/// A file swapped for a link to a secret outside the copy is kept as the
/// link it is, never as what it points to.
#[test]
fn a_link_is_kept_as_a_link_and_not_followed() {
    let s = Setup::new();
    let copy = s.copy();
    let outside = TempDir::new().unwrap();
    fs::write(outside.path().join("id_rsa"), "PRIVATE KEY\n").unwrap();
    std::os::unix::fs::symlink(outside.path().join("id_rsa"), copy.join("key")).unwrap();
    s.task.step("one").unwrap();
    fs::remove_file(copy.join("key")).unwrap();
    s.task.step("two").unwrap();
    s.task.back_to_step(1).unwrap();
    let meta = fs::symlink_metadata(copy.join("key")).unwrap();
    assert!(meta.file_type().is_symlink());
    // Nowhere in the task's keeping is the key's content.
    let dir = copy.parent().unwrap();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            let text = fs::read(entry.path()).unwrap_or_default();
            assert!(
                !String::from_utf8_lossy(&text).contains("PRIVATE KEY"),
                "{}",
                entry.path().display()
            );
        }
    }
}

#[test]
fn a_commit_starts_the_steps_over() {
    let s = Setup::new();
    write(&s.copy(), "a.txt", "a1\n");
    s.task.step("one").unwrap();
    s.task.commit().unwrap();
    assert!(s.task.steps().unwrap().is_empty());
    assert!(s.task.back_to_step(1).is_err());
    // The copy is `<task>/w/<name>`.
    let store = s.copy().ancestors().nth(2).unwrap().join("steps-store");
    assert!(!store.exists());
}
