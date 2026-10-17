//! `kule commit` when another task on the same folder committed first.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

const CODE: &str = "impl Task {\n    fn commit(&self) {\n        1;\n    }\n\n    fn undo(&self) {\n        2;\n    }\n}\n";

#[test]
fn the_second_commit_merges_and_says_so_or_names_what_both_changed() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("task.rs"), CODE).unwrap();
    let new_task = || {
        let out = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
        let id = text(&out).split_whitespace().nth(1).unwrap().to_owned();
        let copy = home.path().join("tasks").join(&id).join("copy");
        (id, copy)
    };
    let (a, copy_a) = new_task();
    let (b, copy_b) = new_task();
    let (c, copy_c) = new_task();
    fs::write(copy_a.join("task.rs"), CODE.replace("1;", "10;")).unwrap();
    fs::write(copy_b.join("task.rs"), CODE.replace("2;", "20;")).unwrap();
    fs::write(copy_c.join("task.rs"), CODE.replace("1;", "11;")).unwrap();

    assert!(kule(home.path(), &["commit", &a]).status.success());
    let out = kule(home.path(), &["commit", &b]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(
        text(&out).contains("merged with what the folder changed meanwhile: task.rs"),
        "{}",
        text(&out)
    );
    let out = kule(home.path(), &["commit", &c]);
    assert!(!out.status.success());
    assert!(
        text(&out).contains("task.rs (both changed method Task::commit)"),
        "{}",
        text(&out)
    );
    assert_eq!(
        fs::read_to_string(ws.path().join("task.rs")).unwrap(),
        CODE.replace("1;", "10;").replace("2;", "20;")
    );
}
