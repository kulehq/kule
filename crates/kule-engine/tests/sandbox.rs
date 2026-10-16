//! M1, step 2: the operating system's sandbox (Seatbelt on macOS, Landlock
//! on Linux). Every test runs real commands through `sh` and checks what
//! actually reached the disk.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Output;

use kule_engine::{Network, Sandbox, Task};
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    /// A directory outside the task, standing for the rest of the machine.
    outside: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("README.md"), "hello\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            outside: TempDir::new().unwrap(),
            task,
        }
    }

    fn outside(&self) -> PathBuf {
        fs::canonicalize(self.outside.path()).unwrap()
    }

    fn sandbox(&self) -> Sandbox {
        Sandbox::new(&self.task).unwrap()
    }
}

fn sh(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .command("sh", &["-c".to_owned(), script.to_owned()])
        .unwrap()
        .output()
        .unwrap()
}

fn ok(out: &Output) -> bool {
    out.status.success()
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn work_inside_the_copy_is_allowed() {
    let s = Setup::new();
    let out = sh(
        &s.sandbox(),
        "echo hi > out.txt && mkdir d && mv out.txt d/ && cat d/out.txt",
    );
    assert!(ok(&out), "{out:?}");
    assert_eq!(text(&out), "hi");
    assert_eq!(
        fs::read_to_string(s.task.copy().join("d/out.txt")).unwrap(),
        "hi\n"
    );
}

#[test]
fn writes_outside_the_copy_are_refused() {
    let s = Setup::new();
    let evil = s.outside().join("evil.txt");
    let out = sh(&s.sandbox(), &format!("echo x > '{}'", evil.display()));
    assert!(!ok(&out));
    assert!(!evil.exists());
}

#[test]
fn the_real_workspace_cannot_be_touched() {
    let s = Setup::new();
    let readme = s.task.workspace().join("README.md");
    let out = sh(
        &s.sandbox(),
        &format!("rm -f '{0}'; echo gone > '{0}'", readme.display()),
    );
    assert!(!ok(&out));
    assert_eq!(fs::read_to_string(&readme).unwrap(), "hello\n");
}

#[test]
fn kule_state_next_to_the_copy_cannot_be_touched() {
    let s = Setup::new();
    let baseline = s.task.copy().parent().unwrap().join("baseline.json");
    let before = fs::read(&baseline).unwrap();
    let out = sh(&s.sandbox(), "echo '{}' > ../baseline.json");
    assert!(!ok(&out));
    assert_eq!(fs::read(&baseline).unwrap(), before);
    assert!(s.task.effects().is_ok());
}

#[test]
fn subprocesses_inherit_the_sandbox() {
    let s = Setup::new();
    let evil = s.outside().join("evil.txt");
    let out = sh(
        &s.sandbox(),
        &format!("sh -c \"sh -c 'echo x > {}'\"", evil.display()),
    );
    assert!(!ok(&out));
    assert!(!evil.exists());
}

#[test]
fn a_symlink_in_the_copy_does_not_lead_outside() {
    let s = Setup::new();
    std::os::unix::fs::symlink(s.outside(), s.task.copy().join("escape")).unwrap();
    let out = sh(&s.sandbox(), "echo x > escape/evil.txt");
    assert!(!ok(&out));
    assert!(!s.outside().join("evil.txt").exists());
}

#[test]
fn the_task_tmp_is_tmpdir_and_writable() {
    let s = Setup::new();
    let out = sh(&s.sandbox(), "echo x > \"$TMPDIR/t\" && echo \"$TMPDIR\"");
    assert!(ok(&out), "{out:?}");
    assert_eq!(
        Path::new(&text(&out)),
        fs::canonicalize(s.task.tmp().unwrap()).unwrap()
    );
}

#[test]
fn devices_and_pipes_still_work() {
    let s = Setup::new();
    let out = sh(&s.sandbox(), "echo x > /dev/null && echo ok | cat");
    assert!(ok(&out), "{out:?}");
    assert_eq!(text(&out), "ok");
}

#[test]
fn an_allowed_path_becomes_writable() {
    let s = Setup::new();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&s.outside()).unwrap();
    let out = sh(
        &sandbox,
        &format!("echo x > '{}/cache.bin'", s.outside().display()),
    );
    assert!(ok(&out), "{out:?}");
    assert!(s.outside().join("cache.bin").exists());
}

#[test]
fn hidden_paths_cannot_be_read_or_listed() {
    let s = Setup::new();
    let secrets = s.outside().join("secrets");
    fs::create_dir(&secrets).unwrap();
    fs::write(secrets.join("id_ed25519"), "PRIVATE KEY\n").unwrap();
    fs::write(s.outside().join("public.txt"), "public\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.hide(&secrets).unwrap();

    let out = sh(&sandbox, &format!("cat '{}/id_ed25519'", secrets.display()));
    assert!(!ok(&out));
    assert!(!text(&out).contains("PRIVATE"));
    assert!(!ok(&sh(&sandbox, &format!("ls '{}'", secrets.display()))));
    // Everything around the hidden path stays readable.
    let out = sh(
        &sandbox,
        &format!("cat '{}/public.txt'", s.outside().display()),
    );
    assert!(ok(&out), "{out:?}");
    assert_eq!(text(&out), "public");
}

#[test]
fn a_hidden_path_wins_over_an_allowed_one() {
    let s = Setup::new();
    let secrets = s.outside().join("secrets");
    fs::create_dir(&secrets).unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&s.outside()).unwrap();
    sandbox.hide(&secrets).unwrap();
    let out = sh(
        &sandbox,
        &format!("echo x > '{}/planted'", secrets.display()),
    );
    assert!(!ok(&out));
    assert!(!secrets.join("planted").exists());
}

/// Connects to a listener the test owns, so no outside network is needed.
fn connect_script(port: u16) -> String {
    format!(
        "python3 -c 'import socket; socket.create_connection((\"127.0.0.1\", {port}), timeout=5)'"
    )
}

#[test]
fn network_can_be_shut() {
    let s = Setup::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let open = sh(&s.sandbox(), &connect_script(port));
    assert!(ok(&open), "allowed connection failed: {open:?}");

    let mut shut = s.sandbox();
    shut.set_network(Network::Deny);
    let out = sh(&shut, &connect_script(port));
    assert!(!ok(&out), "denied connection went through");
}

/// macOS reports each refusal to the system log, tagged per sandbox; Linux
/// has no unprivileged way to read Landlock's refusals and says so.
#[test]
fn refusals_are_reported_where_the_platform_allows() {
    let s = Setup::new();
    let secrets = s.outside().join("secrets");
    fs::create_dir(&secrets).unwrap();
    fs::write(secrets.join("token"), "t\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.hide(&secrets).unwrap();
    let bystander = s.sandbox();

    let evil = s.outside().join("evil.txt");
    let script = format!(
        "echo x > '{}'; cat '{}/token'",
        evil.display(),
        secrets.display()
    );
    assert!(!ok(&sh(&sandbox, &script)));

    let window = std::time::Duration::from_secs(60);
    if !cfg!(target_os = "macos") {
        assert_eq!(sandbox.denials(window).unwrap(), None);
        return;
    }
    let wanted = |d: &kule_engine::Denial, op: &str, path: &Path| {
        d.operation.starts_with(op) && Path::new(&d.path) == path
    };
    let token = secrets.join("token");
    let mut denials = Vec::new();
    for _ in 0..20 {
        denials = sandbox.denials(window).unwrap().unwrap();
        if denials.iter().any(|d| wanted(d, "file-write", &evil))
            && denials.iter().any(|d| wanted(d, "file-read", &token))
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    assert!(
        denials.iter().any(|d| wanted(d, "file-write", &evil)),
        "{denials:?}"
    );
    assert!(
        denials.iter().any(|d| wanted(d, "file-read", &token)),
        "{denials:?}"
    );
    assert_eq!(bystander.denials(window).unwrap(), Some(Vec::new()));
}

/// Shells and tools write to /tmp by hard-coded path; the default sandbox
/// lets them, the bare one does not.
#[test]
fn the_default_sandbox_lets_commands_use_tmp() {
    let s = Setup::new();
    let name = format!("kule-tmp-test-{}", std::process::id());
    let script = format!("echo x > /tmp/{name} && cat /tmp/{name} && rm /tmp/{name}");

    let bare = sh(&s.sandbox(), &script);
    assert!(!ok(&bare), "the bare sandbox let /tmp through");

    let default = sh(&Sandbox::for_task(&s.task).unwrap(), &script);
    assert!(ok(&default), "{default:?}");
    assert_eq!(text(&default), "x");
}
