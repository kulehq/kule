//! `kule status` as a person reads it.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

fn kule(home: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "kule {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn status_lists_what_changed_inside_each_file() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(
        ws.path().join("task.rs"),
        "impl Task {\n    fn commit(&self) {}\n}\n",
    )
    .unwrap();
    fs::write(ws.path().join("notes.md"), "# Plan\nFirst.\n").unwrap();
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    let copy = home.path().join("tasks").join(id).join("copy");
    fs::write(
        copy.join("task.rs"),
        "impl Task {\n    fn commit(&self) { 1; }\n    fn check(&self) {}\n}\n",
    )
    .unwrap();
    fs::write(
        copy.join("notes.md"),
        "# Plan\nFirst.\n\n## Later\nSecond.\n",
    )
    .unwrap();

    let status = kule(home.path(), &["status", id]);
    assert!(
        status.contains(
            "  ~ notes.md\n      + section Plan › Later\n  ~ task.rs\n      ~ method Task::commit\n      + method Task::check\n"
        ),
        "{status}"
    );
}

#[test]
fn status_tells_where_another_task_changes_the_same() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let code = "impl Task {\n    fn commit(&self) {}\n    fn undo(&self) {}\n}\n";
    fs::write(ws.path().join("task.rs"), code).unwrap();
    let new_task = || {
        let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
        let id = created.split_whitespace().nth(1).unwrap().to_owned();
        let copy = home.path().join("tasks").join(&id).join("copy");
        (id, copy)
    };
    let (a, copy_a) = new_task();
    let (b, copy_b) = new_task();
    let (_, copy_c) = new_task();
    let commit = code.replace("commit(&self) {}", "commit(&self) { 1; }");
    fs::write(copy_a.join("task.rs"), &commit).unwrap();
    fs::write(copy_b.join("task.rs"), commit.replace("1;", "2;")).unwrap();
    fs::write(
        copy_c.join("task.rs"),
        code.replace("undo(&self) {}", "undo(&self) { 3; }"),
    )
    .unwrap();

    let status = kule(home.path(), &["status", &a]);
    let expected = format!("task {b} changes them too:\n  task.rs · method Task::commit\n");
    assert!(status.ends_with(&expected), "{status}");
}

#[test]
fn status_shows_moves_as_moves() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::create_dir(ws.path().join("photos")).unwrap();
    for i in 0..3 {
        fs::write(ws.path().join(format!("photos/{i}.jpg")), [i; 8]).unwrap();
    }
    fs::write(ws.path().join("notes.md"), "# Notes\n").unwrap();
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    let copy = home.path().join("tasks").join(id).join("copy");
    fs::rename(copy.join("photos"), copy.join("2026")).unwrap();
    fs::create_dir(copy.join("docs")).unwrap();
    fs::rename(copy.join("notes.md"), copy.join("docs/notes.md")).unwrap();

    let status = kule(home.path(), &["status", id]);
    assert!(
        status.starts_with(
            "6 file changes: +1 ~0 -0 »5\n  » photos/ → 2026/\n  + docs/\n  » notes.md → docs/notes.md\n"
        ),
        "{status}"
    );
    assert!(status.contains("passes on its own"), "{status}");
}

#[test]
fn status_tells_what_the_runs_spent_and_what_is_left() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    assert!(!kule(home.path(), &["status", id]).contains("spent"));

    let usage = home.path().join("tasks").join(id).join("usage.jsonl");
    fs::write(
        &usage,
        concat!(
            r#"{"agent":"claude-code","started_unix":1,"input_tokens":1000,"output_tokens":200,"cost":{"amount":0.4,"currency":"USD"}}"#,
            "\n",
            r#"{"agent":"codex","started_unix":2,"input_tokens":300}"#,
            "\n",
        ),
    )
    .unwrap();
    let set = kule(home.path(), &["budget", id, "1.5"]);
    assert_eq!(
        set,
        "spent: 0.40 USD, 1500 tokens in 2 runs (1 without a cost); budget 1.50 USD, 1.10 USD left\n"
    );
    assert!(kule(home.path(), &["status", id]).ends_with(&set));

    let off = kule(home.path(), &["budget", id, "off"]);
    assert_eq!(
        off,
        "spent: 0.40 USD, 1500 tokens in 2 runs (1 without a cost)\n"
    );
}

#[test]
fn list_shows_a_tasks_name_and_colour() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    kule(home.path(), &["name", id, "march invoices"]);
    kule(home.path(), &["color", id, "teal"]);
    let list = kule(home.path(), &["list"]);
    assert!(list.contains(" · march invoices (teal)"), "{list}");

    let refused = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(["color", id, "#ff0000"])
        .env("KULE_HOME", home.path())
        .output()
        .unwrap();
    assert!(!refused.status.success());

    kule(home.path(), &["name", id]);
    kule(home.path(), &["color", id]);
    let list = kule(home.path(), &["list"]);
    assert!(!list.contains('·') && !list.contains('('), "{list}");
}

#[test]
fn policy_prints_the_one_that_holds_and_puts_kules_in_place() {
    let home = TempDir::new().unwrap();
    assert!(!kule(home.path(), &["policy"]).contains("@id(\"careful\")"));
    assert_eq!(
        kule(home.path(), &["policy", "careful"]),
        "policy: careful\n"
    );
    assert!(kule(home.path(), &["policy"]).contains("@id(\"careful\")"));
    kule(home.path(), &["policy", "balanced"]);
    assert!(!home.path().join("policy.cedar").exists());

    fs::write(home.path().join("policy.cedar"), "// mine\n").unwrap();
    assert_eq!(kule(home.path(), &["policy"]), "// mine\n");
    let refused = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(["policy", "careful"])
        .env("KULE_HOME", home.path())
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(
        fs::read_to_string(home.path().join("policy.cedar")).unwrap(),
        "// mine\n"
    );
}

#[test]
fn a_group_puts_its_policy_over_its_tasks_commits() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("notes.md"), "# Plan\n").unwrap();
    let created = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap();
    let made = kule(home.path(), &["group", "new", "clients", "--color", "blue"]);
    assert!(made.contains("group 1 made: clients"), "{made}");
    kule(home.path(), &["group", "policy", "1", "careful"]);
    kule(home.path(), &["group", "put", id, "1"]);
    let list = kule(home.path(), &["list"]);
    assert!(list.contains(" [clients]"), "{list}");
    let groups = kule(home.path(), &["group"]);
    assert!(
        groups.contains("1  clients · careful (blue)") && groups.contains(id),
        "{groups}"
    );
    assert!(kule(home.path(), &["group", "policy", "1"]).contains("@id(\"careful\")"));

    // The careful group holds the edit that the policy for every task lets
    // through.
    let copy = home.path().join("tasks").join(id).join("copy");
    fs::write(copy.join("notes.md"), "# Plan\nMore.\n").unwrap();
    let commit = |home: &Path| {
        Command::new(env!("CARGO_BIN_EXE_kule"))
            .args(["commit", id])
            .env("KULE_HOME", home)
            .output()
            .unwrap()
    };
    assert!(!commit(home.path()).status.success());
    assert_eq!(
        fs::read_to_string(ws.path().join("notes.md")).unwrap(),
        "# Plan\n"
    );

    // A policy of its own is checked before it is kept.
    let own = home.path().join("own.cedar");
    fs::write(&own, "permit (").unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args([
            "group",
            "policy",
            "1",
            "own",
            "--file",
            own.to_str().unwrap(),
        ])
        .env("KULE_HOME", home.path())
        .output()
        .unwrap();
    assert!(!refused.status.success());

    kule(home.path(), &["group", "delete", "1"]);
    assert!(!kule(home.path(), &["list"]).contains('['));
    assert!(commit(home.path()).status.success());
    assert_eq!(
        fs::read_to_string(ws.path().join("notes.md")).unwrap(),
        "# Plan\nMore.\n"
    );
}
