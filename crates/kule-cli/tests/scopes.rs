//! Budgets and limits for every task, a group's tasks and one task, as
//! `kule limits`, `kule group budget` and `kule budget` say them.

use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .output()
        .unwrap()
}

fn kule(home: &Path, args: &[&str]) -> String {
    let out = run(home, args);
    assert!(
        out.status.success(),
        "kule {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn refused(home: &Path, args: &[&str]) -> String {
    let out = run(home, args);
    assert!(!out.status.success(), "kule {args:?} went through");
    String::from_utf8(out.stderr).unwrap()
}

/// The line about `name` in a limits listing.
fn line<'a>(listing: &'a str, name: &str) -> &'a str {
    listing
        .lines()
        .find(|l| l.trim_start().starts_with(name))
        .unwrap_or_else(|| panic!("no {name} in:\n{listing}"))
}

/// A task in group 1, "clients".
fn task_in_a_group(home: &Path, ws: &Path) -> String {
    let created = kule(home, &["new", ws.to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap().to_owned();
    kule(home, &["group", "new", "clients"]);
    kule(home, &["group", "put", &id, "1"]);
    id
}

#[test]
fn each_limit_says_which_scope_it_comes_from() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let h = home.path();
    let id = task_in_a_group(h, ws.path());

    let every = kule(h, &["limits", "memory=8", "cpu=4"]);
    assert!(every.starts_with("limits for EVERY TASK"), "{every}");
    assert!(line(&every, "memory").contains("8 GB"), "{every}");

    let group = kule(
        h,
        &["limits", "--group", "1", "memory=2", "gpu=off", "cpu=none"],
    );
    assert!(
        group.starts_with("limits for EACH TASK IN GROUP 1 \"clients\" (1 task)"),
        "{group}"
    );
    assert!(
        line(&group, "memory").ends_with("2 GB (the group's)"),
        "{group}"
    );
    assert!(
        line(&group, "processor").ends_with("no limit (the group's)"),
        "{group}"
    );
    assert!(
        line(&group, "one run").ends_with("(every task's)"),
        "{group}"
    );

    // A move says what it changes of the task's limits, loosened or not.
    let out = kule(h, &["group", "put", &id]);
    assert!(
        out.contains(
            "its limits: processor no limit → 4 cores, memory 2 GB → 8 GB, GPU off → may be used"
        ),
        "{out}"
    );
    let back = kule(h, &["group", "put", &id, "1"]);
    assert!(back.contains("memory 8 GB → 2 GB"), "{back}");

    let task = kule(h, &["limits", "--task", &id, "memory=1"]);
    assert!(
        task.starts_with(&format!(
            "limits for TASK {id} only (in group 1 \"clients\")"
        )),
        "{task}"
    );
    assert!(line(&task, "memory").ends_with("1 GB (its own)"), "{task}");
    assert!(
        line(&task, "GPU").ends_with("off (group \"clients\")"),
        "{task}"
    );
    assert!(line(&task, "one run").ends_with("(every task's)"), "{task}");
    // Back to the group's.
    let task = kule(h, &["limits", "--task", &id, "memory=inherit"]);
    assert!(
        line(&task, "memory").ends_with("2 GB (group \"clients\")"),
        "{task}"
    );
    // And to every task's, once the group's is gone too.
    kule(h, &["limits", "--group", "1", "memory=inherit"]);
    let task = kule(h, &["limits", "--task", &id]);
    assert!(
        line(&task, "memory").ends_with("8 GB (every task's)"),
        "{task}"
    );

    // Every task's limits have nothing above them; one scope at a time; a
    // value out of its range or a group there is not changes nothing.
    assert!(refused(h, &["limits", "memory=inherit"]).contains("nothing above them"));
    refused(h, &["limits", "--group", "1", "--task", &id, "memory=1"]);
    refused(h, &["limits", "--group", "1", "memory=0.01"]);
    refused(h, &["limits", "--group", "2", "memory=1"]);
    refused(h, &["limits", "--task", &id, "speed=9"]);
    assert!(line(&kule(h, &["limits", "--group", "1"]), "GPU").ends_with("off (the group's)"));
}

#[test]
fn a_group_s_budget_holds_its_tasks_and_says_so() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let h = home.path();
    let id = task_in_a_group(h, ws.path());

    let shown = kule(h, &["group", "budget", "1", "20"]);
    assert!(
        shown.starts_with("budgets for GROUP 1 \"clients\": its 1 task together"),
        "{shown}"
    );
    assert!(
        line(&shown, "all their runs").contains("20.00 USD, 0.00 USD spent"),
        "{shown}"
    );
    assert!(
        line(&shown, "any 24 hours").contains("no budget"),
        "{shown}"
    );
    let shown = kule(h, &["group", "budget", "1", "5", "--daily"]);
    // Setting one keeps the other.
    assert!(
        line(&shown, "all their runs").contains("20.00 USD"),
        "{shown}"
    );
    assert!(line(&shown, "any 24 hours").contains("5.00 USD"), "{shown}");
    assert!(kule(h, &["group"]).contains("budget 20.00 USD, 5.00 USD a day"));
    refused(h, &["group", "budget", "1", "0"]);
    refused(h, &["group", "budget", "9", "5"]);

    let out = kule(h, &["group", "put", &id]);
    assert!(out.contains("no group budget holds it any more"), "{out}");
    let out = kule(h, &["group", "put", &id, "1"]);
    assert!(
        out.contains("its group's budgets: 20.00 USD in all (\"clients\"), 5.00 USD a day"),
        "{out}"
    );

    let budgets = kule(h, &["budget", &id]);
    assert!(
        line(&budgets, "group \"clients\", all its runs").contains("20.00 USD left"),
        "{budgets}"
    );
    assert!(
        budgets.contains("the next run may cost at most 5.00 USD"),
        "{budgets}"
    );

    // Its tasks spent it: the run is refused before any agent starts, and
    // the refusal names the group.
    let usage = h.join("tasks").join(&id).join("usage.jsonl");
    std::fs::write(
        &usage,
        "{\"agent\":\"claude-code\",\"started_unix\":1,\"cost\":{\"amount\":25.0,\"currency\":\"USD\"},\"group\":1}\n",
    )
    .unwrap();
    let e = refused(h, &["run", &id, "hello"]);
    assert!(e.contains("group \"clients\" has spent its budget"), "{e}");
    assert!(kule(h, &["budget", &id]).contains("no run starts: group \"clients\""));

    kule(h, &["group", "budget", "1", "off"]);
    let shown = kule(h, &["group", "budget", "1"]);
    assert!(
        line(&shown, "all their runs").contains("no budget (25.00 USD spent)"),
        "{shown}"
    );
}
