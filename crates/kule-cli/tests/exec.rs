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

/// What Kule cannot record of what the sandbox refused is a `kule:` line,
/// and the command's own exit code still comes back; a command that
/// succeeded fails, since part of what it did went untold.
#[test]
fn an_unrecorded_refusal_is_said_and_the_exit_code_kept() {
    if !cfg!(target_os = "macos") {
        // Linux does not tell Kule what the sandbox refused.
        return;
    }
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let outside = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let kule = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kule"))
            .args(args)
            .env("KULE_HOME", home.path())
            .output()
            .unwrap()
    };
    let out = kule(&["new", ws.path().to_str().unwrap()]);
    let id = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    // Where the refusals are kept cannot be written.
    fs::create_dir(home.path().join("tasks").join(&id).join("sandbox.jsonl")).unwrap();
    let target = fs::canonicalize(outside.path()).unwrap().join("x");
    for (code, expected) in [(3, 3), (0, 1)] {
        let script = format!("echo x > {}; exit {code}", target.display());
        let out = kule(&["exec", &id, "--", "sh", "-c", &script]);
        let (said, err) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert_eq!(out.status.code(), Some(expected), "{said}{err}");
        assert!(
            said.contains(&format!(
                "sandbox: refused file-write-create {}",
                target.display()
            )),
            "{said}"
        );
        assert!(
            err.contains("kule: what the sandbox refused could not be recorded"),
            "{err}"
        );
    }
}
