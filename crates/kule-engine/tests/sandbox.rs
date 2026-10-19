//! M1, step 2: the operating system's sandbox (Seatbelt on macOS, Landlock
//! on Linux). Every test runs real commands through `sh` and checks what
//! actually reached the disk.

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

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

/// The folder cannot be read either: the copy is what a command works on,
/// and what it masks (the secrets in `.env` files) is not in the folder's
/// files masked.
#[test]
fn the_real_workspace_cannot_be_read() {
    let s = Setup::new();
    fs::write(s.task.workspace().join(".env"), "API_TOKEN=real-value\n").unwrap();
    let ws = fs::canonicalize(s.task.workspace()).unwrap();
    let out = sh(
        &s.sandbox(),
        &format!(
            "cat '{0}/README.md'; cat '{0}/.env'; ls '{0}'; cat README.md",
            ws.display()
        ),
    );
    let said = text(&out);
    assert_eq!(said, "hello", "{out:?}");
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

/// What the person typed, their browsers' and mail's stores, and the
/// credentials tools keep in the home directory are out of a command's
/// sight; the rest of the home stays readable.
#[test]
fn histories_browsers_and_credentials_in_the_home_are_hidden() {
    let s = Setup::new();
    // Outside /tmp, as a person's home is.
    let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let hidden = [
        ".zsh_history",
        ".bash_history",
        ".python_history",
        ".node_repl_history",
        ".psql_history",
        ".mysql_history",
        ".lesshst",
        ".viminfo",
        "Library/Cookies/Cookies.binarycookies",
        "Library/Application Support/Google/Chrome/Default/Cookies",
        "Library/Application Support/Firefox/Profiles/a/cookies.sqlite",
        "Library/Safari/History.db",
        "Library/Mail/V10/x",
        "Library/Messages/chat.db",
        ".config/google-chrome/Default/Cookies",
        ".mozilla/firefox/a/cookies.sqlite",
        ".thunderbird/a/prefs.js",
        ".config/rclone/rclone.conf",
        ".m2/settings.xml",
        ".gradle/gradle.properties",
        ".gem/credentials",
        ".pgpass",
        ".my.cnf",
        ".oci/config",
        ".databrickscfg",
        ".cache/huggingface/token",
        ".config/Code/User/globalStorage/state.vscdb",
        "Library/Application Support/Code/User/globalStorage/state.vscdb",
    ];
    for path in hidden.iter().chain(&["notes/plain.txt"]) {
        let full = home.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, "secret\n").unwrap();
    }
    let sources = CacheSources {
        home: fs::canonicalize(home.path()).unwrap(),
        cargo_home: home.path().join(".cargo"),
    };
    let sandbox = Sandbox::for_task_with(&s.task, Some(sources.clone())).unwrap();
    for path in hidden {
        let out = sh(
            &sandbox,
            &format!("cat '{}'", sources.home.join(path).display()),
        );
        assert!(!ok(&out), "{path} was readable");
    }
    let out = sh(
        &sandbox,
        &format!("cat '{}'", sources.home.join("notes/plain.txt").display()),
    );
    assert!(ok(&out), "{out:?}");
}

/// Linux: a task's commands have a /tmp of their own, so tasks cannot leave
/// files there for each other, or for the person's tools.
#[cfg(target_os = "linux")]
#[test]
fn each_task_has_a_tmp_of_its_own() {
    let (a, b) = (Setup::new(), Setup::new());
    let name = format!("kule-own-tmp-{}", std::process::id());
    // A home outside /tmp, as a person's is.
    let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let sources = || kule_engine::CacheSources {
        home: home.path().to_owned(),
        cargo_home: home.path().join(".cargo"),
    };
    let one = Sandbox::for_task_with(&a.task, Some(sources())).unwrap();
    let two = Sandbox::for_task_with(&b.task, Some(sources())).unwrap();
    let wrote = sh(&one, &format!("echo x > /tmp/{name} && cat /tmp/{name}"));
    assert!(ok(&wrote), "{wrote:?}");
    assert!(
        !Path::new("/tmp").join(&name).exists(),
        "it reached the system's /tmp"
    );
    let seen = sh(&two, &format!("cat /tmp/{name}"));
    assert!(!ok(&seen), "another task saw it");
}

/// Where this computer gives a command less isolation than Kule means to,
/// the command runs only once the person accepted that. CI runs this a
/// second time with unprivileged user namespaces refused.
#[test]
fn weaker_isolation_runs_only_once_accepted() {
    let s = Setup::new();
    let network = Network::Proxy(9);
    if kule_engine::isolation_gaps(network).is_empty() {
        // CI's second run says it took user namespaces away.
        assert!(
            std::env::var_os("KULE_TEST_EXPECT_GAPS").is_none(),
            "no gap where one was expected"
        );
        eprintln!("skipped: this computer gives a command all of its isolation");
        return;
    }
    let mut refused = s.sandbox();
    refused.set_network(network);
    let error = refused.command("true", &[]).expect_err("ran less isolated");
    assert!(error.to_string().contains("weaker isolation"), "{error}");

    kule_engine::set_isolation(
        s._home.path(),
        &kule_engine::Isolation {
            accept_weaker: true,
        },
    )
    .unwrap();
    let mut accepted = s.sandbox();
    accepted.set_network(network);
    let out = accepted.command("true", &[]).unwrap().output().unwrap();
    assert!(ok(&out), "{out:?}");
}

/// A process that left its session and its parent still lives in the
/// sandbox; ending the sandbox's strays ends it, and nothing else.
#[test]
fn processes_left_behind_end_with_the_sandbox() {
    let s = Setup::new();
    let sandbox = s.sandbox();
    let script = "import os, time\n\
                  if os.fork(): os._exit(0)\n\
                  os.setsid()\n\
                  if os.fork(): os._exit(0)\n\
                  fd = os.open('/dev/null', os.O_RDWR)\n\
                  for i in (0, 1, 2): os.dup2(fd, i)\n\
                  open('left.pid', 'w').write(str(os.getpid()))\n\
                  time.sleep(60)\n";
    assert!(ok(&sandbox_python(&sandbox, script)));
    let pid_file = s.task.copy().join("left.pid");
    let started = std::time::Instant::now();
    while fs::read_to_string(&pid_file).map_or(true, |t| t.is_empty())
        && started.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let pid: i32 = fs::read_to_string(&pid_file).unwrap().parse().unwrap();
    // SAFETY: signal 0 only asks whether the process is there.
    let alive = |pid: i32| unsafe { libc::kill(pid, 0) } == 0;
    assert!(alive(pid));
    // A process of the person's own in the copy, outside the sandbox, stays.
    let mut own = Command::new("sleep")
        .arg("30")
        .current_dir(s.task.copy())
        .spawn()
        .unwrap();

    sandbox.end_strays();
    let started = std::time::Instant::now();
    while alive(pid) && started.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let own_alive = alive(own.id() as i32);
    let _ = own.kill();
    let _ = own.wait();
    // SAFETY: as above; a stray the test could not end is ended here.
    let left = alive(pid);
    unsafe { libc::kill(pid, libc::SIGKILL) };
    assert!(!left, "a process left in the sandbox outlived it");
    assert!(own_alive, "a process outside the sandbox was ended");
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

/// Unix sockets lead to the user's daemons (Kule's own, Docker, ssh-agent,
/// the session bus); only sockets in the copy and tmp are reachable.
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
/// An agent's plugins can run to hundreds of megabytes, more than Kule
/// notes to put back. Where they cannot be kept read-only in place, they
/// are hidden from the command instead: still never changed by it.
#[test]
fn a_protected_directory_too_large_to_note_is_still_never_changed() {
    let s = Setup::new();
    let state = s.outside().join("agent-state");
    let plugins = state.join("plugins");
    fs::create_dir_all(&plugins).unwrap();
    // Sparse: larger than Kule notes, without the disk.
    fs::File::create(plugins.join("bundle.js"))
        .unwrap()
        .set_len(65 << 20)
        .unwrap();
    // Where the plugins are hidden (Linux without user namespaces),
    // Landlock opens the state by what is in it, so the agent writes in its
    // directories, not new files next to them.
    fs::create_dir_all(state.join("sessions")).unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&state).unwrap();
    sandbox.protect(&plugins).unwrap();
    let kept = sandbox.keep_protected().unwrap();
    let out = sh(
        &sandbox,
        &format!(
            "echo evil > '{0}/plugins/hook.js'; echo session > '{0}/sessions/1.json'",
            state.display()
        ),
    );
    kept.restore().unwrap();
    assert!(!plugins.join("hook.js").exists(), "{out:?}");
    assert_eq!(
        fs::read_to_string(state.join("sessions/1.json")).unwrap(),
        "session\n"
    );
}

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

/// A server that asks for every address listens on loopback, in each of the
/// languages Kule hooks, through a shell as an agent's commands go.
#[test]
fn a_server_asking_for_every_address_listens_on_loopback() {
    let s = Setup::new();
    let sandbox = s.sandbox();
    let have = |tool: &str| {
        Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let mut cases = vec![
        (
            "python socket",
            "python3 -c \"import socket; s=socket.socket(); s.bind(('0.0.0.0', 0)); print(s.getsockname()[0])\"",
            "127.0.0.1",
        ),
        (
            "python http.server",
            "python3 -c \"import http.server as h, socket; s=h.ThreadingHTTPServer(('', 0), h.SimpleHTTPRequestHandler); print(s.socket.getsockname()[0])\"",
            "127.0.0.1",
        ),
        (
            "python dual stack",
            "python3 -c \"import socket; s=socket.socket(socket.AF_INET6); s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0); s.bind(('::', 0)); print(s.getsockname()[0])\"",
            "::ffff:127.0.0.1",
        ),
    ];
    if have("node") {
        cases.push((
            "node listen(port)",
            "node -e \"const s=require('http').createServer(); s.listen(0, () => { console.log(s.address().address); s.close(); })\"",
            "127.0.0.1",
        ));
        cases.push((
            "node listen(port, ::)",
            "node -e \"const s=require('net').createServer(); s.listen({ port: 0, host: '::' }, () => { console.log(s.address().address); s.close(); })\"",
            "127.0.0.1",
        ));
    }
    if have("ruby") {
        cases.push((
            "ruby TCPServer",
            "ruby -e \"puts TCPServer.new('0.0.0.0', 0).addr[3]\"",
            "127.0.0.1",
        ));
        cases.push((
            "ruby tcp_server_sockets",
            "ruby -e \"puts Socket.tcp_server_sockets(0).map { |s| s.local_address.ip_address }.uniq\"",
            "127.0.0.1",
        ));
    }
    for (what, script, wanted) in cases {
        let out = sh(&sandbox, script);
        assert!(ok(&out), "{what}: {out:?}");
        assert_eq!(text(&out).trim(), wanted, "{what}");
    }
    // A server given an address of its own keeps it.
    let own = sh(
        &sandbox,
        "python3 -c \"import socket; s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[0])\"",
    );
    assert_eq!(text(&own).trim(), "127.0.0.1");
}

/// The address this computer reaches its network with (nothing is sent),
/// when it has one.
#[cfg(target_os = "macos")]
fn network_ip() -> Option<std::net::IpAddr> {
    let probe = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("192.0.2.1:9").ok()?;
    let ip = probe.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

/// Asks the server at `ip`:`port` and says whether it answered.
#[cfg(target_os = "macos")]
fn answers(ip: std::net::IpAddr, port: u16) -> bool {
    use std::io::{Read, Write};
    std::net::TcpStream::connect_timeout(&(ip, port).into(), std::time::Duration::from_secs(3))
        .and_then(|mut c| {
            c.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;
            c.write_all(b"ping")?;
            let mut buf = [0u8; 2];
            c.read_exact(&mut buf)?;
            Ok(buf == *b"hi")
        })
        .unwrap_or(false)
}

/// A sandboxed server, `python3 -I` so no hook reaches it, on `host`; returns
/// it with its port.
#[cfg(target_os = "macos")]
fn raw_server(sandbox: &Sandbox, host: &str) -> (std::process::Child, u16) {
    use std::io::{BufRead, BufReader};
    let code = format!(
        "import socket\ns=socket.socket()\ns.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)\ns.bind(('{host}',0))\ns.listen()\nprint(s.getsockname()[1],flush=True)\nwhile True:\n c,_=s.accept()\n c.settimeout(3)\n try:\n  if c.recv(4)==b'ping': c.sendall(b'hi')\n except Exception: pass\n c.close()"
    );
    let mut child = sandbox
        .command("python3", &["-I".to_owned(), "-c".to_owned(), code])
        .unwrap()
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    (child, line.trim().parse().unwrap())
}

/// On macOS, a server no hook reaches that listens on every address answers
/// this computer and not the network, once Kule shields it; one that takes
/// a network address of its own is stopped.
#[cfg(target_os = "macos")]
#[test]
fn the_shield_keeps_an_unhooked_server_off_the_network() {
    use kule_engine::shield::{Shield, descendants};
    let Some(outward) = network_ip() else {
        eprintln!("no network address to try from; skipped");
        return;
    };
    let s = Setup::new();
    let sandbox = s.sandbox();
    let (mut server, port) = raw_server(&sandbox, "0.0.0.0");
    let loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    assert!(
        answers(outward, port),
        "the test server should be on the network before the shield"
    );

    let mut shield = Shield::new();
    let guarded = shield.guard(&descendants());
    assert!(guarded.shielded.contains(&port), "{guarded:?}");
    assert!(
        answers(loopback, port),
        "the shield took this computer's own way in"
    );
    assert!(
        !answers(outward, port),
        "the network still reaches the server"
    );

    // A server on a network address of its own is stopped.
    let (mut own, own_port) = raw_server(&sandbox, &outward.to_string());
    let guarded = shield.guard(&descendants());
    assert!(
        guarded.stopped.iter().any(|s| s.port == own_port),
        "{guarded:?}"
    );
    assert!(own.wait().is_ok());

    shield.release();
    assert!(
        answers(outward, port),
        "released, the port is the server's again"
    );
    let _ = server.kill();
    let _ = server.wait();
}

/// On Linux, without a network of its own, any program's server is held to
/// loopback by Kule doing its binds, hooks or not (`python3 -I` ignores
/// them): every address becomes loopback, a listen without a bind listens
/// on loopback, an address on the network is refused, and Unix sockets and
/// UDP clients go on as asked.
#[cfg(target_os = "linux")]
#[test]
fn any_program_s_server_is_held_to_loopback_on_linux() {
    let s = Setup::new();
    // Network allowed: no namespace of its own, on every Linux.
    let sandbox = s.sandbox();
    let py = |code: &str| {
        let out = sandbox_python_isolated(&sandbox, code);
        (
            ok(&out),
            text(&out),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let cases = [
        (
            "every address",
            "import socket; s=socket.socket(); s.bind(('0.0.0.0', 0)); print(s.getsockname()[0])",
            "127.0.0.1",
        ),
        (
            "listen unbound",
            "import socket; s=socket.socket(); s.listen(); print(s.getsockname()[0])",
            "127.0.0.1",
        ),
        (
            "dual stack",
            "import socket; s=socket.socket(socket.AF_INET6); s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 0); s.bind(('::', 0)); print(s.getsockname()[0])",
            "::ffff:127.0.0.1",
        ),
        (
            "v6 only",
            "import socket; s=socket.socket(socket.AF_INET6); s.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1); s.bind(('::', 0)); print(s.getsockname()[0])",
            "::1",
        ),
        (
            "loopback asked",
            "import socket; s=socket.socket(); s.bind(('127.0.0.1', 0)); print(s.getsockname()[0])",
            "127.0.0.1",
        ),
        (
            "udp client",
            "import socket; s=socket.socket(socket.AF_INET, socket.SOCK_DGRAM); s.bind(('0.0.0.0', 0)); print(s.getsockname()[0])",
            "0.0.0.0",
        ),
        (
            "unix socket",
            "import socket, os; p=os.path.join(os.environ['TMPDIR'], 'u.sock'); s=socket.socket(socket.AF_UNIX); s.bind(p); print('bound')",
            "bound",
        ),
    ];
    for (what, code, wanted) in cases {
        let (worked, said, err) = py(code);
        assert!(worked, "{what}: {err}");
        assert_eq!(said, wanted, "{what}");
    }
    // An address on the network is refused; Go, Rust and the rest get the
    // same, since the kernel asks Kule, not the language.
    let (worked, _, err) = py("import socket; s=socket.socket(); s.bind(('10.1.2.3', 0))");
    assert!(!worked, "a bind to a network address went through");
    assert!(
        err.contains("Permission denied") || err.contains("Errno 13"),
        "{err}"
    );
}

#[cfg(target_os = "linux")]
fn sandbox_python_isolated(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .command(
            "python3",
            &["-I".to_owned(), "-c".to_owned(), script.to_owned()],
        )
        .unwrap()
        .output()
        .unwrap()
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

#[test]
fn a_command_cannot_write_a_file_past_the_size_limit() {
    let s = Setup::new();
    let mut sandbox = s.sandbox();
    sandbox.limit_file_size(1 << 20);
    let out = sh(&sandbox, "head -c 3000000 /dev/zero > big; echo $?");
    assert_ne!(text(&out), "0", "the write went through");
    let size = fs::metadata(s.task.copy().join("big")).unwrap().len();
    assert!(size <= 1 << 20, "{size}");
    // Without the limit it goes through.
    let out = sh(&s.sandbox(), "head -c 3000000 /dev/zero > big2; echo $?");
    assert_eq!(text(&out), "0");
}

#[test]
fn the_task_s_limits_reach_its_sandbox() {
    let s = Setup::new();
    s.task
        .set_limits(&kule_engine::TaskLimits {
            disk_mb: Some(Some(1)),
            ..Default::default()
        })
        .unwrap();
    let sandbox = Sandbox::for_task_with(&s.task, None).unwrap();
    let out = sh(&sandbox, "head -c 3000000 /dev/zero > big; echo $?");
    assert_ne!(text(&out), "0", "the write went through");
}

/// Metal, through a small Swift program: whether it gets a device and runs
/// a command on it. `None` where it cannot be told: no Swift compiler, or
/// no GPU (a virtual machine).
#[cfg(target_os = "macos")]
fn metal_works(sandbox: Option<&Sandbox>, probe: &Path) -> Option<bool> {
    let out = match sandbox {
        Some(sandbox) => sandbox
            .command(probe.to_str().unwrap(), &[])
            .unwrap()
            .output(),
        None => Command::new(probe).output(),
    }
    .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).starts_with("gpu ok"))
}

#[cfg(target_os = "macos")]
#[test]
fn a_command_denied_the_gpu_gets_no_metal_device() {
    let s = Setup::new();
    let dir = s.outside();
    let source = dir.join("probe.swift");
    fs::write(
        &source,
        "import Metal\n\
         guard let d = MTLCreateSystemDefaultDevice(), let q = d.makeCommandQueue(), let b = d.makeBuffer(length: 4096), let c = q.makeCommandBuffer(), let e = c.makeBlitCommandEncoder() else { print(\"no device\"); exit(2) }\n\
         e.fill(buffer: b, range: 0..<4096, value: 7); e.endEncoding(); c.commit(); c.waitUntilCompleted()\n\
         print(c.status == .completed ? \"gpu ok\" : \"gpu failed\")\n",
    )
    .unwrap();
    let probe = dir.join("probe");
    let built = Command::new("swiftc")
        .arg(&source)
        .arg("-o")
        .arg(&probe)
        .status()
        .is_ok_and(|s| s.success());
    if !built || metal_works(None, &probe) != Some(true) {
        eprintln!("skipped: no Swift compiler or no GPU here");
        return;
    }
    assert_eq!(metal_works(Some(&s.sandbox()), &probe), Some(true));
    let mut denied = s.sandbox();
    denied.deny_gpu();
    assert_eq!(metal_works(Some(&denied), &probe), Some(false));
}
