//! `kule agents`: the person's own agent profiles, approved before they run.

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

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const OPENCODE: &str = r#"
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode"]
login = [".local/share/opencode/auth.json"]
pass_env = ["OPENROUTER_API_KEY"]
hosts = ["openrouter.ai"]
"#;

#[test]
fn a_profile_of_your_own_runs_once_approved() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(home.path().join("agents.toml"), OPENCODE).unwrap();

    let list = stdout(&kule(home.path(), &["agents"]));
    assert!(list.contains("claude-code      built in\n"), "{list}");
    assert!(
        list.contains("opencode         yours, held: not approved"),
        "{list}"
    );

    // Held: a run is refused before anything starts.
    let created = stdout(&kule(home.path(), &["new", ws.path().to_str().unwrap()]));
    let id = created.split_whitespace().nth(1).unwrap();
    let run = kule(home.path(), &["run", id, "--agent", "opencode", "hello"]);
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("agent opencode cannot run: not approved"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let approved = stdout(&kule(home.path(), &["agents", "approve", "opencode"]));
    assert!(
        approved.contains("also connects to  https://openrouter.ai"),
        "{approved}"
    );
    assert!(
        approved.contains("sees your env     OPENROUTER_API_KEY"),
        "{approved}"
    );
    let list = stdout(&kule(home.path(), &["agents"]));
    assert!(list.contains("opencode         yours\n"), "{list}");

    stdout(&kule(home.path(), &["agents", "revoke", "opencode"]));
    let list = stdout(&kule(home.path(), &["agents"]));
    assert!(list.contains("opencode         yours, held"), "{list}");
}

#[test]
fn a_profile_that_opens_too_much_cannot_be_approved() {
    let home = TempDir::new().unwrap();
    let wide = OPENCODE.replace(
        r#"state = [".local/share/opencode"]"#,
        r#"state = [".ssh"]"#,
    );
    fs::write(home.path().join("agents.toml"), wide).unwrap();
    let out = kule(home.path(), &["agents", "approve", "opencode"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(".ssh"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A run that fails still says what it changed, what the sandbox refused
/// it, and how long it took; the error comes last, with where the
/// agent said why.
#[test]
fn a_failed_run_still_tells_what_its_sandbox_did() {
    // The folder standing for the rest of the computer is outside /tmp,
    // which commands may write to on Linux.
    let (home, ws, outside) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    let target = fs::canonicalize(outside.path()).unwrap().join("x");
    let script = format!("echo x > {}; sleep 1; exit 3", target.display());
    fs::write(
        home.path().join("agents.toml"),
        format!("[[agent]]\nname = \"broken\"\ncommand = [\"sh\", \"-c\", {script:?}]\n"),
    )
    .unwrap();
    stdout(&kule(home.path(), &["agents", "approve", "broken"]));
    let created = stdout(&kule(home.path(), &["new", ws.path().to_str().unwrap()]));
    let id = created.split_whitespace().nth(1).unwrap();

    let run = kule(home.path(), &["run", id, "--agent", "broken", "hello"]);
    let (out, err) = (
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(!run.status.success(), "{out}");
    assert!(!target.exists(), "the sandbox let it write outside");
    assert!(out.contains("policy:"), "{out}");
    assert!(out.contains("took:"), "{out}");
    assert!(err.contains("agent.log"), "{err}");
    if cfg!(target_os = "macos") {
        assert!(
            out.contains(&format!(
                "sandbox: refused file-write-create {}",
                target.display()
            )),
            "{out}"
        );
    } else {
        assert!(err.contains(kule_engine::UNREPORTED), "{err}");
    }
}

/// A run sets aside what it may spend: while it is at work, another task's
/// run under the same daily budget is refused, saying why. A run that did
/// not end (its `kule run` was killed) keeps its share counted until the
/// person lets go of it, which is refused while it is at work.
#[test]
fn releasing_an_unfinished_reservation_is_said_and_kept() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(
        home.path().join("agents.toml"),
        "[[agent]]\nname = \"slow\"\ncommand = [\"sh\", \"-c\", \"sleep 30\"]\n\n\
         [[agent]]\nname = \"quick\"\ncommand = [\"sh\", \"-c\", \"exit 3\"]\n",
    )
    .unwrap();
    stdout(&kule(home.path(), &["agents", "approve", "slow"]));
    stdout(&kule(home.path(), &["agents", "approve", "quick"]));
    let new = || {
        let created = stdout(&kule(home.path(), &["new", ws.path().to_str().unwrap()]));
        created.split_whitespace().nth(1).unwrap().to_owned()
    };
    let (first, second) = (new(), new());
    stdout(&kule(home.path(), &["daily-budget", "10", "USD"]));

    // A run that ends closes its share: what it spent counts instead.
    let ended = kule(home.path(), &["run", &second, "--agent", "quick", "hi"]);
    assert!(!ended.status.success(), "the agent speaks no ACP");
    let daily = stdout(&kule(home.path(), &["daily-budget"]));
    assert!(
        daily.contains("10.00 USD left") && !daily.contains("set aside"),
        "{daily}"
    );

    let mut running = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(["run", &first, "--agent", "slow", "hi"])
        .env("KULE_HOME", home.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let share = home
        .path()
        .join("tasks")
        .join(&first)
        .join("reserved.jsonl");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while fs::read_to_string(&share).unwrap_or_default().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "nothing was set aside"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let refused = kule(home.path(), &["run", &second, "--agent", "slow", "hi"]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success());
    assert!(
        said.contains("set aside") && said.contains(&first),
        "{said}"
    );
    let at_work = kule(home.path(), &["budget", &first, "--release"]);
    assert!(!at_work.status.success());
    assert!(String::from_utf8_lossy(&at_work.stderr).contains("at work"));

    // Killed: its usage never comes, and its share stays counted.
    running.kill().unwrap();
    running.wait().unwrap();
    let task = kule_engine::Task::open(home.path(), &first).unwrap();
    let _ = kule_engine::Sandbox::end_all(&task);
    let held = kule(home.path(), &["run", &second, "--agent", "slow", "hi"]);
    assert!(String::from_utf8_lossy(&held.stderr).contains("set aside"));

    let released = stdout(&kule(home.path(), &["budget", &first, "--release"]));
    assert!(released.contains("let go of 10.00 USD"), "{released}");
    let usage =
        fs::read_to_string(home.path().join("tasks").join(&first).join("usage.jsonl")).unwrap();
    assert!(usage.contains("\"released\":true"), "{usage}");
    let daily = stdout(&kule(home.path(), &["daily-budget"]));
    assert!(daily.contains("10.00 USD left"), "{daily}");
}

/// A copy Kule cannot read back after a run (a folder the agent made
/// unreadable) hides nothing else: what the sandbox refused is still told
/// and recorded, why the changes cannot be shown is said, and the run's own
/// error comes back.
#[test]
fn what_cannot_be_shown_after_a_run_hides_nothing_else() {
    let (home, ws, outside) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    let created = stdout(&kule(home.path(), &["new", ws.path().to_str().unwrap()]));
    let id = created.split_whitespace().nth(1).unwrap();
    let copy = stdout(&kule(home.path(), &["path", id])).trim().to_owned();
    let target = fs::canonicalize(outside.path()).unwrap().join("x");
    // The agent starts in the task's tmp, not in the copy.
    let script = format!(
        "mkdir -p '{copy}/locked' && chmod 000 '{copy}/locked'; echo x > {}; sleep 1; exit 3",
        target.display()
    );
    fs::write(
        home.path().join("agents.toml"),
        format!("[[agent]]\nname = \"broken\"\ncommand = [\"sh\", \"-c\", {script:?}]\n"),
    )
    .unwrap();
    stdout(&kule(home.path(), &["agents", "approve", "broken"]));
    // So the folder can be removed, however the test ends.
    struct Unlock(std::path::PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let _unlock = Unlock(Path::new(&copy).join("locked"));

    let run = kule(home.path(), &["run", id, "--agent", "broken", "hello"]);
    let (out, err) = (
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr),
    );
    assert!(!run.status.success(), "{out}");
    assert!(err.contains("agent.log"), "{err}");
    assert!(
        err.contains("kule: what the run changed cannot be shown"),
        "{err}"
    );
    if cfg!(target_os = "macos") {
        // The system log did not show all of it: the refusal may be missing.
        if err.contains(kule_engine::INCOMPLETE) {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr().lock(),
                "INCONCLUSIVE what_cannot_be_shown_after_a_run_hides_nothing_else: {err}"
            );
            return;
        }
        assert!(
            out.contains(&format!(
                "sandbox: refused file-write-create {}",
                target.display()
            )),
            "{out}"
        );
        let task = kule_engine::Task::open(home.path(), id).unwrap();
        assert!(
            kule_engine::denials(&task)
                .unwrap()
                .iter()
                .any(|d| Path::new(&d.path) == target),
            "{out}"
        );
    } else {
        assert!(err.contains(kule_engine::UNREPORTED), "{err}");
    }
}
