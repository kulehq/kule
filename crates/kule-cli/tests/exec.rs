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

/// A copy Kule cannot read back after the command (a folder the command
/// made unreadable) hides nothing else: what the sandbox refused is still
/// told and recorded, why the changes cannot be shown is said, and the
/// command's own exit code comes back; a command that succeeded fails.
#[test]
fn what_cannot_be_shown_after_a_command_hides_nothing_else() {
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
    let copy = String::from_utf8_lossy(&kule(&["path", &id]).stdout)
        .trim()
        .to_owned();
    // So the folder can be removed, however the test ends.
    struct Unlock(std::path::PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let _unlock = Unlock(std::path::Path::new(&copy).join("locked"));
    let target = fs::canonicalize(outside.path()).unwrap().join("x");
    for (code, expected) in [(3, 3), (0, 1)] {
        let script = format!(
            "mkdir -p locked && chmod 000 locked; echo x > {}; exit {code}",
            target.display()
        );
        let out = kule(&["exec", &id, "--", "sh", "-c", &script]);
        let (said, err) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert_eq!(out.status.code(), Some(expected), "{said}{err}");
        assert!(
            err.contains("kule: what the command changed cannot be shown"),
            "{err}"
        );
        if cfg!(target_os = "macos") {
            assert!(
                said.contains(&format!(
                    "sandbox: refused file-write-create {}",
                    target.display()
                )),
                "{said}{err}"
            );
            let task = kule_engine::Task::open(home.path(), &id).unwrap();
            assert!(
                kule_engine::denials(&task)
                    .unwrap()
                    .iter()
                    .any(|d| std::path::Path::new(&d.path) == target),
                "{said}{err}"
            );
        } else if code != 0 {
            assert!(err.contains(kule_engine::UNREPORTED), "{err}");
        }
    }
}

/// `kule list` lists every task it can read, and names one whose record
/// cannot be read, with why, rather than failing whole.
#[test]
fn a_task_whose_record_cannot_be_read_does_not_hide_the_others() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
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
    let garbled = home.path().join("tasks/garbled");
    fs::create_dir_all(&garbled).unwrap();
    fs::write(garbled.join("task.json"), "{ not json").unwrap();
    let out = kule(&["list"]);
    let (said, err) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "{err}");
    assert!(said.contains(&id), "{said}");
    assert!(err.contains("kule: task garbled cannot be read"), "{err}");
    // No command starts in any task while it is there, and the person is
    // told which one and what to do, each cause once.
    let out = kule(&["exec", &id, "--", "true"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(
        err.contains("task garbled's record cannot be read"),
        "{err}"
    );
    assert!(err.contains("`kule rm garbled`"), "{err}");
    assert_eq!(err.matches("key must be a string").count(), 1, "{err}");
    // It can be removed, and then the others work again.
    let out = kule(&["rm", "garbled"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!garbled.exists());
    let out = kule(&["exec", &id, "--", "true"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // An error that names its cause says it once.
    let settings = home.path().join("tasks").join(&id).join("settings.json");
    fs::write(&settings, "{").unwrap();
    let out = kule(&["status", &id]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert_eq!(err.matches("EOF while parsing").count(), 1, "{err}");
}

/// A command's probes of terminals other than its own are one line after
/// it, counted, as a shell's are where it ends.
#[test]
fn terminal_probes_are_one_line_after_a_command() {
    if !cfg!(target_os = "macos") {
        // Linux does not tell Kule what the sandbox refused.
        return;
    }
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
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
    let script = "ls /dev/ttys5 /dev/ttys6 /dev/ttys7 2>/dev/null; true";
    let out = kule(&["exec", &id, "--", "sh", "-c", script]);
    let (said, err) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(out.status.success(), "{err}");
    let probes: Vec<&str> = said.lines().filter(|l| l.contains("/dev/ttys")).collect();
    assert_eq!(probes.len(), 1, "{said}");
    assert!(probes[0].contains("/dev/ttys* (ls) ×"), "{said}");
}

/// The first command in a task says it copies the person's package caches
/// (macOS, where it clones them), once; the next says nothing of them.
#[test]
fn the_first_command_says_it_makes_the_caches() {
    let (home, ws, person) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    fs::create_dir_all(person.path().join(".npm/_cacache")).unwrap();
    let kule = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kule"))
            .args(args)
            .env("KULE_HOME", home.path())
            .env("HOME", person.path())
            .env_remove("CARGO_HOME")
            .output()
            .unwrap()
    };
    let out = kule(&["new", ws.path().to_str().unwrap()]);
    let id = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .nth(1)
        .unwrap()
        .to_owned();
    let told = |out: &std::process::Output| {
        String::from_utf8_lossy(&out.stderr)
            .matches("preparing this task's package caches")
            .count()
    };
    let first = kule(&["exec", &id, "--", "true"]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(told(&first), usize::from(cfg!(target_os = "macos")));
    let next = kule(&["exec", &id, "--", "true"]);
    assert!(
        next.status.success(),
        "{}",
        String::from_utf8_lossy(&next.stderr)
    );
    assert_eq!(told(&next), 0);
}
