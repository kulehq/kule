//! `kule exec`: what opens the sandbox wider is said where the command runs.

use std::fs;
use std::process::Command;

use tempfile::TempDir;

/// `--allow-write`, `--reveal` and `--pass-env` each say what they open,
/// as `--no-sandbox` does.
#[test]
fn what_opens_the_sandbox_wider_is_said() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    // Outside /tmp, which commands may write to on Linux anyway.
    let outside = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let kule = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kule"))
            .args(args)
            .env("KULE_HOME", home.path())
            .env("KULE_TEST_SECRET", "x")
            .output()
            .unwrap()
    };
    let out = kule(&["new", ws.path().to_str().unwrap()]);
    let id = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let dir = outside.path().to_str().unwrap();
    let out = kule(&[
        "exec",
        &id,
        "--allow-write",
        dir,
        "--reveal",
        dir,
        "--pass-env",
        "KULE_TEST_SECRET",
        "--",
        "true",
    ]);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{said}");
    for line in [
        format!("kule: the command may write {dir}, outside the task's copy"),
        format!("kule: the command may read {dir}, which the sandbox hides"),
        "kule: the command gets KULE_TEST_SECRET from your environment".to_owned(),
    ] {
        assert!(said.contains(&line), "{line} in {said}");
    }
}
