//! `kule path` and `kule open`: a task's copy, or a file of it, reached
//! from outside Kule. Nothing here opens an app: `kule open --dry-run`
//! checks as `kule open` does and prints instead.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .output()
        .unwrap()
}

fn ok(home: &Path, args: &[&str]) -> String {
    let out = kule(home, args);
    assert!(
        out.status.success(),
        "kule {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn refused(home: &Path, args: &[&str]) -> String {
    let out = kule(home, args);
    assert!(!out.status.success(), "kule {args:?} went through");
    assert!(out.stdout.is_empty(), "kule {args:?} printed a path");
    String::from_utf8(out.stderr).unwrap()
}

/// A task on a folder; its id and copy.
fn task(home: &Path, ws: &Path) -> (String, PathBuf) {
    let created = ok(home, &["new", ws.to_str().unwrap()]);
    let id = created.split_whitespace().nth(1).unwrap().to_owned();
    let copy = PathBuf::from(ok(home, &["path", &id]).trim());
    (id, copy)
}

#[test]
fn path_names_the_copy_or_a_file_in_it_and_nothing_outside() {
    let (home, ws, outside) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    fs::create_dir_all(ws.path().join("docs")).unwrap();
    fs::write(ws.path().join("docs/plan.md"), "# Plan\n").unwrap();
    let (id, copy) = task(home.path(), ws.path());
    assert!(copy.join("docs/plan.md").is_file());
    assert_eq!(
        ok(home.path(), &["path", &id, "docs/plan.md"]).trim(),
        copy.join("docs/plan.md").to_str().unwrap()
    );
    assert_eq!(
        ok(home.path(), &["path", &id, "docs"]).trim(),
        copy.join("docs").to_str().unwrap()
    );

    std::os::unix::fs::symlink(outside.path(), copy.join("away")).unwrap();
    for file in ["away", "away/x", "../x", "/etc/hosts", "missing.md"] {
        refused(home.path(), &["path", &id, file]);
    }
}

#[test]
fn path_warns_when_git_in_the_copy_would_run_what_the_task_changed() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let (id, copy) = task(home.path(), ws.path());
    let quiet = kule(home.path(), &["path", &id]);
    assert!(
        quiet.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&quiet.stderr)
    );

    // A repository the task put in the copy: git there reads its config.
    fs::create_dir_all(copy.join("sub/.git")).unwrap();
    fs::write(copy.join("sub/.git/config"), "[core]\n\tfsmonitor = ./x\n").unwrap();
    let warned = kule(home.path(), &["path", &id]);
    assert!(warned.status.success());
    assert_eq!(
        String::from_utf8_lossy(&warned.stdout).trim(),
        copy.to_str().unwrap()
    );
    let said = String::from_utf8_lossy(&warned.stderr);
    assert!(said.contains("careful") && said.contains(".git"), "{said}");
}

#[test]
fn open_lets_through_documents_that_are_what_their_name_says() {
    let (home, ws, outside) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let (id, copy) = task(home.path(), ws.path());
    fs::write(copy.join("summary.pdf"), b"%PDF-1.7\n").unwrap();
    fs::write(copy.join("march.csv"), "day,sum\n1,3\n").unwrap();
    for file in ["summary.pdf", "march.csv"] {
        // Kule's own copy of it, never the agent's file.
        let at = PathBuf::from(ok(home.path(), &["open", "--dry-run", &id, file]).trim());
        assert!(
            at.starts_with(home.path().join("opened")),
            "{}",
            at.display()
        );
        assert_eq!(fs::read(&at).unwrap(), fs::read(copy.join(file)).unwrap());
    }

    fs::write(copy.join("rename.command"), "#!/bin/sh\n").unwrap();
    fs::write(copy.join("notes.pdf"), "#!/bin/sh\n").unwrap();
    fs::write(
        copy.join("sums.csv"),
        "a\n=IMAGE(\"https://example.com/p\")\n",
    )
    .unwrap();
    fs::write(outside.path().join("real.pdf"), b"%PDF-1.7").unwrap();
    std::os::unix::fs::symlink(outside.path().join("real.pdf"), copy.join("linked.pdf")).unwrap();
    for file in [
        "rename.command",
        "notes.pdf",
        "sums.csv",
        "linked.pdf",
        "../a.pdf",
        "docs",
    ] {
        refused(home.path(), &["open", "--dry-run", &id, file]);
    }
    assert!(
        refused(home.path(), &["open", "--dry-run", &id, "rename.command"])
            .contains("outside the sandbox")
    );
}

#[test]
fn open_waits_while_an_agent_is_at_work() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let (id, copy) = task(home.path(), ws.path());
    fs::write(copy.join("summary.pdf"), b"%PDF-1.7\n").unwrap();
    // As `kule run` or the app marks it: this test's process is alive.
    let marker = home.path().join("tasks").join(&id).join("agent");
    fs::write(&marker, format!("{}\n", std::process::id())).unwrap();
    assert!(refused(home.path(), &["open", "--dry-run", &id, "summary.pdf"]).contains("at work"));
    fs::remove_file(&marker).unwrap();
    ok(home.path(), &["open", "--dry-run", &id, "summary.pdf"]);
}
