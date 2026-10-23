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
        let copy = std::path::PathBuf::from(
            text(&out)
                .split(" · copy at ")
                .nth(1)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        );
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

/// What still runs in the task's sandbox is ended before Kule puts the
/// copy back, and the person is told what, by name, where they asked for
/// it (macOS, where such a process outlives its command).
#[cfg(target_os = "macos")]
#[test]
fn rolling_back_tells_what_it_ended() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let out = kule(home.path(), &["new", ws.path().to_str().unwrap()]);
    let id = text(&out).split_whitespace().nth(1).unwrap().to_owned();
    let task = kule_engine::Task::open(home.path(), &id).unwrap();
    let code = "import os, time\nif os.fork(): os._exit(0)\nos.setsid()\nif os.fork(): os._exit(0)\ntime.sleep(60)";
    let status = kule_engine::Sandbox::new(&task)
        .unwrap()
        .command(
            "python3",
            &["-I".to_owned(), "-c".to_owned(), code.to_owned()],
        )
        .unwrap()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while kule_engine::Sandbox::processes_of(&id).is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the stray never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let out = kule(home.path(), &["rollback", &id]);
    let said = text(&out);
    assert!(out.status.success(), "{said}");
    let told = String::from_utf8_lossy(&out.stderr).to_lowercase();
    assert!(
        told.contains("kule: kule ended python") && told.contains("to roll back"),
        "{said}"
    );
}
