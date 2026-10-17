//! M1, step 2: the operating system's sandbox (Seatbelt on macOS, Landlock
//! on Linux). Every test runs real commands through `sh` and checks what
//! actually reached the disk.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Output;

use kule_engine::{CacheSources, Network, Sandbox, Task, protects_in_place};
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
        // Kule's home is hidden from the sandbox; on Linux hiding it inside
        // /tmp would take /tmp itself away, so it lives elsewhere, as
        // ~/.kule does.
        let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
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
fn kules_home_cannot_be_read_but_the_copy_can() {
    let s = Setup::new();
    let task_dir = s.task.copy().parent().unwrap().to_owned();
    let home = task_dir.parent().unwrap().parent().unwrap().to_owned();
    fs::write(home.join("policy.cedar"), "// the person's policy\n").unwrap();
    let other = home.join("tasks/other/copy");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("secret.rs"), "another project\n").unwrap();

    for path in [
        home.join("policy.cedar"),
        other.join("secret.rs"),
        task_dir.join("baseline.json"),
    ] {
        let out = sh(&s.sandbox(), &format!("cat '{}'", path.display()));
        assert!(!ok(&out), "{} was readable", path.display());
    }
    let out = sh(
        &s.sandbox(),
        "cat README.md && echo more >> README.md && ls \"$TMPDIR\"",
    );
    assert!(ok(&out), "{out:?}");
    assert!(text(&out).starts_with("hello"));
}

/// Unix sockets lead to the user's daemons (Kule's own, Docker, ssh-agent);
/// only sockets in the copy and tmp are reachable. Linux has no Landlock
/// rule for it; there the daemon also wants a token the sandbox cannot read.
#[cfg(target_os = "macos")]
#[test]
fn unix_sockets_outside_the_copy_are_out_of_reach() {
    use std::os::unix::net::UnixListener;
    let s = Setup::new();
    let outside = s.outside().join("daemon.sock");
    let _daemon = UnixListener::bind(&outside).unwrap();
    let inside = s.task.tmp().unwrap().join("test.sock");
    let _test_server = UnixListener::bind(&inside).unwrap();
    let connect = |path: &Path| {
        let script = format!(
            "import socket; s = socket.socket(socket.AF_UNIX); s.connect({:?}); print('connected')",
            path.to_str().unwrap()
        );
        sandbox_python(&s.sandbox(), &script)
    };
    assert!(!ok(&connect(&outside)));
    let reached = connect(&inside);
    assert!(ok(&reached), "{reached:?}");
}

fn sandbox_python(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .command("python3", &["-c".to_owned(), script.to_owned()])
        .unwrap()
        .output()
        .unwrap()
}

/// An agent's own settings sit inside its writable state; where they name
/// hooks or MCP servers that run outside the sandbox later, they stay
/// readable but not writable. Landlock cannot refuse the write without
/// refusing new files next to them too, so on Linux they are mounted
/// read-only for the command, or, where the kernel refuses the namespace,
/// put back after the run.
#[test]
fn a_protected_file_inside_a_writable_directory_stays_read_only() {
    let s = Setup::new();
    let state = s.outside().join("agent-state");
    fs::create_dir_all(&state).unwrap();
    fs::write(state.join("settings.json"), "{}\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&state).unwrap();
    sandbox.protect(&state.join("settings.json")).unwrap();
    let kept = sandbox.keep_protected().unwrap();

    let out = sh(
        &sandbox,
        &format!(
            "cat '{0}/settings.json' && echo session > '{0}/session.json'",
            state.display()
        ),
    );
    assert!(ok(&out), "{out:?}");
    assert_eq!(text(&out), "{}");
    let out = sh(
        &sandbox,
        &format!(
            "echo '{{\"hooks\":1}}' > '{}/settings.json'",
            state.display()
        ),
    );
    let restored = kept.restore().unwrap();
    if protects_in_place() {
        assert!(!ok(&out), "{out:?}");
        assert!(restored.is_empty());
    } else {
        let paths: Vec<_> = restored.iter().map(|p| p.path.clone()).collect();
        assert_eq!(paths, [state.join("settings.json")]);
    }
    assert_eq!(
        fs::read_to_string(state.join("settings.json")).unwrap(),
        "{}\n"
    );
    assert_eq!(
        fs::read_to_string(state.join("session.json")).unwrap(),
        "session\n"
    );
}

/// The person may change an agent's settings while a run goes, from the
/// agent itself outside Kule. Kept read-only in place, the change simply
/// stays; put back, it is moved aside, never lost.
#[test]
fn a_change_made_outside_the_sandbox_during_a_run_is_not_lost() {
    let s = Setup::new();
    let state = s.outside().join("agent-state");
    fs::create_dir_all(&state).unwrap();
    let settings = state.join("settings.json");
    fs::write(&settings, "{}\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&state).unwrap();
    sandbox.protect(&settings).unwrap();
    let kept = sandbox.keep_protected().unwrap();
    let mut command = sandbox
        .command("sh", &["-c".to_owned(), "read line".to_owned()])
        .unwrap();
    let mut running = command.stdin(std::process::Stdio::piped()).spawn().unwrap();
    // The person's own session, while the command still runs.
    fs::write(&settings, "{\"theme\":\"mine\"}\n").unwrap();
    drop(running.stdin.take());
    running.wait().unwrap();

    let restored = kept.restore().unwrap();
    if protects_in_place() {
        assert!(restored.is_empty());
        assert_eq!(
            fs::read_to_string(&settings).unwrap(),
            "{\"theme\":\"mine\"}\n"
        );
    } else {
        assert_eq!(restored.len(), 1);
        let saved = restored[0].saved.as_ref().unwrap();
        assert_eq!(fs::read_to_string(saved).unwrap(), "{\"theme\":\"mine\"}\n");
    }
}

/// Settings linked in from elsewhere (a dotfiles repository) stay linked:
/// the command can neither write through the link nor swap it for a file.
#[test]
fn a_protected_link_cannot_be_swapped() {
    let s = Setup::new();
    let (state, dotfiles) = (
        s.outside().join("agent-state"),
        s.outside().join("dotfiles"),
    );
    fs::create_dir_all(&state).unwrap();
    fs::create_dir_all(&dotfiles).unwrap();
    fs::write(dotfiles.join("settings.json"), "{}\n").unwrap();
    let settings = state.join("settings.json");
    std::os::unix::fs::symlink(dotfiles.join("settings.json"), &settings).unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&state).unwrap();
    sandbox.protect(&settings).unwrap();
    let kept = sandbox.keep_protected().unwrap();

    let through = sh(
        &sandbox,
        &format!("echo '{{\"hooks\":1}}' > '{}'", settings.display()),
    );
    assert!(!ok(&through), "{through:?}");
    let _ = sh(
        &sandbox,
        &format!(
            "rm -f '{0}' && echo '{{\"hooks\":1}}' > '{0}'",
            settings.display()
        ),
    );
    kept.restore().unwrap();
    assert_eq!(
        fs::read_link(&settings).unwrap(),
        dotfiles.join("settings.json")
    );
    assert_eq!(
        fs::read_to_string(dotfiles.join("settings.json")).unwrap(),
        "{}\n"
    );
}

/// A protected path the agent's state names on its own, like a settings
/// file in the home directory, is never writable.
#[test]
fn a_protected_writable_root_stays_read_only() {
    let s = Setup::new();
    let settings = s.outside().join("agent.json");
    fs::write(&settings, "{}\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&settings).unwrap();
    sandbox.protect(&settings).unwrap();
    let kept = sandbox.keep_protected().unwrap();

    let out = sh(
        &sandbox,
        &format!("echo '{{}}{{}}' > '{}'", settings.display()),
    );
    assert!(!ok(&out), "{out:?}");
    assert!(kept.restore().unwrap().is_empty());
    assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
}

/// Tokens and agent sockets in the person's environment stay out; what
/// tools need to find themselves goes in, and what is passed on purpose.
#[test]
fn only_the_environment_tools_need_reaches_a_command() {
    let s = Setup::new();
    let (secret, passed) = (
        format!("KULE_TEST_SECRET_{}", std::process::id()),
        format!("KULE_TEST_PASSED_{}", std::process::id()),
    );
    // SAFETY: the names are this test's own; no other thread reads them.
    unsafe {
        std::env::set_var(&secret, "hunter2");
        std::env::set_var(&passed, "on purpose");
    }
    let mut sandbox = s.sandbox();
    sandbox.pass_env(&passed);
    let out = sh(&sandbox, "env");
    let env = text(&out);
    assert!(ok(&out), "{out:?}");
    assert!(!env.contains("hunter2"), "{env}");
    assert!(env.contains(&format!("{passed}=on purpose")), "{env}");
    for name in ["PATH=", "HOME="] {
        assert!(
            env.lines().any(|l| l.starts_with(name)),
            "{name} missing: {env}"
        );
    }
}

/// No controlling terminal: nothing in the sandbox can push keystrokes into
/// the person's terminal, and its process group can be stopped as one.
#[test]
fn commands_run_in_a_session_of_their_own() {
    let s = Setup::new();
    let out = sandbox_python(
        &s.sandbox(),
        "import os; print(os.getsid(0) == os.getpid(), os.getpgrp() == os.getpid())",
    );
    assert!(ok(&out), "{out:?}");
    assert_eq!(text(&out), "True True");
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

    let default = sh(&Sandbox::for_task_with(&s.task, None).unwrap(), &script);
    assert!(ok(&default), "{default:?}");
    assert_eq!(text(&default), "x");
}

/// What a package manager finds in the person's caches it builds or
/// installs later without checking again; a task gets caches of its own.
#[test]
fn package_managers_write_to_the_tasks_own_caches() {
    let s = Setup::new();
    let person = s.outside();
    let registry = person.join(".cargo/registry/src/serde-1.0.0");
    fs::create_dir_all(&registry).unwrap();
    fs::write(registry.join("lib.rs"), "real\n").unwrap();
    fs::write(person.join(".cargo/credentials.toml"), "token = \"t\"\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox
        .use_task_caches(&CacheSources {
            home: person.clone(),
            cargo_home: person.join(".cargo"),
        })
        .unwrap();

    let out = sh(
        &sandbox,
        "mkdir -p \"$CARGO_HOME/registry/src/serde-1.0.0\" \
         && echo poisoned > \"$CARGO_HOME/registry/src/serde-1.0.0/lib.rs\" \
         && echo x > \"$XDG_CACHE_HOME/pip\" && echo \"$CARGO_HOME\"",
    );
    assert!(ok(&out), "{out:?}");
    assert!(Path::new(&text(&out)).starts_with(fs::canonicalize(s.task.tmp().unwrap()).unwrap()));
    assert_eq!(
        fs::read_to_string(registry.join("lib.rs")).unwrap(),
        "real\n"
    );

    let out = sh(
        &sandbox,
        &format!("cat '{}/.cargo/credentials.toml'", person.display()),
    );
    assert!(!ok(&out), "{out:?}");
}

/// A task's defaults leave the person's caches as they are: readable, and
/// nothing can be left in them.
#[test]
fn the_persons_package_caches_are_read_only() {
    let s = Setup::new();
    // Not under /tmp, which the defaults leave writable.
    let person = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = fs::canonicalize(person.path()).unwrap();
    let caches = [
        ".cargo/registry",
        ".cargo/git",
        ".npm",
        ".nuget/packages",
        ".cache",
        "Library/Caches",
    ];
    for cache in caches {
        fs::create_dir_all(home.join(cache)).unwrap();
        fs::write(home.join(cache).join("cached"), "real\n").unwrap();
    }
    let sources = CacheSources {
        cargo_home: home.join(".cargo"),
        home: home.clone(),
    };
    let sandbox = Sandbox::for_task_with(&s.task, Some(sources)).unwrap();
    for cache in caches {
        let dir = home.join(cache);
        let out = sh(
            &sandbox,
            &format!("cat '{0}/cached' && echo x > '{0}/new'", dir.display()),
        );
        assert_eq!(text(&out), "real", "{cache} is not readable");
        assert!(!ok(&out), "{cache} is writable");
        assert!(!dir.join("new").exists());
    }
}

/// A hidden directory stays hidden whole when a path inside it was hidden
/// first (Landlock builds the readable set by carving them out in turn).
#[test]
fn a_hidden_directory_stays_hidden_after_a_path_inside_it() {
    let s = Setup::new();
    let state = s.outside().join("agent");
    fs::create_dir_all(state.join("projects")).unwrap();
    fs::write(state.join("login.json"), "secret\n").unwrap();
    fs::write(state.join("projects/session.jsonl"), "secret\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.hide(&state.join("login.json")).unwrap();
    sandbox.hide(&state).unwrap();
    for path in ["login.json", "projects/session.jsonl"] {
        let out = sh(&sandbox, &format!("cat '{}'", state.join(path).display()));
        assert!(!ok(&out), "{path}: {out:?}");
    }
}
