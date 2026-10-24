//! `kule shell`: the person's shell in a task's copy, in the task's sandbox
//! on a terminal of its own; outside it only when asked for, and then only
//! once the changes not committed were seen.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

struct Setup {
    home: TempDir,
    person: TempDir,
    _ws: TempDir,
    outside: TempDir,
    id: String,
    copy: PathBuf,
}

impl Setup {
    fn new() -> Setup {
        // Kule's home and the folder standing for the rest of the computer
        // outside /tmp, which the sandbox lets commands write to.
        let target = env!("CARGO_TARGET_TMPDIR");
        let (home, person, ws, outside) = (
            TempDir::new_in(target).unwrap(),
            TempDir::new_in(target).unwrap(),
            TempDir::new().unwrap(),
            TempDir::new_in(target).unwrap(),
        );
        fs::write(ws.path().join("a.txt"), "a\n").unwrap();
        let mut s = Setup {
            home,
            person,
            _ws: ws,
            outside,
            id: String::new(),
            copy: PathBuf::new(),
        };
        let created = s.ok(&["new", s._ws.path().to_str().unwrap()], "");
        s.id = created.split_whitespace().nth(1).unwrap().to_owned();
        s.copy = fs::canonicalize(s.ok(&["path", &s.id], "").trim()).unwrap();
        s
    }

    /// `kule` with `args`, `typed` as what the person types.
    fn kule(&self, args: &[&str], typed: &str) -> Output {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kule"))
            .args(args)
            .env("KULE_HOME", self.home.path())
            .env("HOME", self.person.path())
            .env("SHELL", "/bin/sh")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(typed.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn ok(&self, args: &[&str], typed: &str) -> String {
        let out = self.kule(args, typed);
        assert!(
            out.status.success(),
            "kule {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn escaped(&self) -> PathBuf {
        fs::canonicalize(self.outside.path())
            .unwrap()
            .join("escaped")
    }
}

fn said(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

/// By default the shell runs in the task's sandbox, in the copy, on a
/// terminal of its own: nothing it runs writes outside the copy.
#[test]
fn the_shell_runs_in_the_tasks_sandbox() {
    let s = Setup::new();
    let out = s.kule(
        &["shell", &s.id],
        &format!(
            "tty -s && echo on-a-terminal\n\
             echo \"at:$(pwd -P) task:$KULE_TASK\"\n\
             echo x > '{}' || echo refused\n\
             echo b > b.txt\n\
             exit 3\n",
            s.escaped().display()
        ),
    );
    let text = said(&out);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(text.contains("on-a-terminal"), "{text}");
    assert!(
        text.contains(&format!("at:{} task:{}", s.copy.display(), s.id)),
        "{text}"
    );
    assert!(text.contains("refused"), "{text}");
    assert!(!s.escaped().exists());
    assert_eq!(fs::read_to_string(s.copy.join("b.txt")).unwrap(), "b\n");
    assert!(text.contains("in its sandbox"), "{text}");
}

/// Outside the sandbox only when asked for; while the copy holds changes
/// not committed, it shows them and opens only once told to.
#[test]
fn outside_the_sandbox_the_changes_are_shown_first() {
    let s = Setup::new();
    fs::write(s.copy.join("b.txt"), "b\n").unwrap();
    let typed = format!("echo x > '{}'\nexit 4\n", s.escaped().display());

    let out = s.kule(&["shell", &s.id, "--outside"], &typed);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains("b.txt") && text.contains("--yes"), "{text}");
    assert!(!s.escaped().exists());

    let out = s.kule(&["shell", &s.id, "--outside", "--yes"], &typed);
    let text = said(&out);
    assert_eq!(out.status.code(), Some(4), "{text}");
    assert!(
        text.contains("b.txt") && text.contains("outside the sandbox"),
        "{text}"
    );
    assert!(s.escaped().exists());
}

/// `kule closed` tells Kule the terminal apps and editors the app opened
/// the copy in are closed: until then no agent starts work in the task.
#[test]
fn closed_lets_work_start_again() {
    let s = Setup::new();
    let task_dir = Path::new(s.home.path()).join("tasks").join(&s.id);
    assert!(task_dir.is_dir(), "{}", task_dir.display());
    fs::write(
        task_dir.join("outside.json"),
        r#"[{"what":"VS Code","at_unix":1}]"#,
    )
    .unwrap();
    let typed = "exit 0\n";
    let out = s.kule(&["exec", &s.id, "--", "true"], typed);
    let text = said(&out);
    assert!(!out.status.success(), "{text}");
    assert!(text.contains(&format!("kule closed {}", s.id)), "{text}");
    s.ok(&["closed", &s.id], "");
    s.ok(&["exec", &s.id, "--", "true"], "");
}

/// What a command leaves running ends with `kule exec`, however it left
/// the command's process group and parent.
#[test]
fn what_exec_leaves_running_ends_with_it() {
    if !Command::new("python3")
        .args(["-c", "pass"])
        .status()
        .is_ok_and(|s| s.success())
    {
        eprintln!("skipped: no python3");
        return;
    }
    let s = Setup::new();
    // The command ends once its stray has beaten.
    let script = "import os, time\nif os.fork():\n  end=time.time()+10\n  while time.time()<end and not os.path.exists('beat'): time.sleep(0.01)\n  os._exit(0)\nos.setsid()\nif os.fork(): os._exit(0)\nfor fd in (0, 1, 2): os.close(fd)\nopen('pid','w').write(str(os.getpid()))\nwhile True:\n  open('beat','a').write('.')\n  time.sleep(0.02)";
    let out = s.kule(
        &["exec", &s.id, "--no-network", "--", "python3", "-c", script],
        "",
    );
    assert!(out.status.success(), "{}", said(&out));
    let told = String::from_utf8_lossy(&out.stderr).to_lowercase();
    let (beat, pid) = (s.copy.join("beat"), s.copy.join("pid"));
    let len = || fs::metadata(&beat).map_or(0, |m| m.len());
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut stopped = false;
    while !stopped && std::time::Instant::now() < until {
        let before = len();
        std::thread::sleep(std::time::Duration::from_millis(300));
        stopped = before > 0 && len() == before;
    }
    // In a PID namespace of its own (Linux) it ended with the command, at
    // once, and its pid is the namespace's.
    if kule_engine::own_processes() {
        assert!(stopped || len() == 0, "a process outlived kule exec");
        return;
    }
    if let Some(pid) = fs::read_to_string(&pid)
        .ok()
        .and_then(|p| p.trim().parse::<i32>().ok())
    {
        // SAFETY: kill only sends a signal.
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
    assert!(len() > 0, "it never beat");
    assert!(stopped, "a process outlived kule exec");
    // And the person is told what was ended, by name.
    assert!(told.contains("kule: ended python"), "{told}");
}

/// What the shell left running is ended as it ends, and the person is told
/// so by name (macOS); a shell that failed says, on Linux, that refusals
/// are not reported.
#[test]
fn what_the_shell_left_running_is_told_as_it_ends() {
    let s = Setup::new();
    let out = s.kule(
        &["shell", &s.id],
        "nohup sleep 30 </dev/null >/dev/null 2>&1 &\nsleep 1\nexit 3\n",
    );
    let text = said(&out);
    assert_eq!(out.status.code(), Some(3), "{text}");
    if cfg!(target_os = "macos") {
        assert!(text.contains("kule: Kule ended sleep,"), "{text}");
    } else {
        assert!(text.contains(kule_engine::UNREPORTED), "{text}");
    }
}

/// On macOS, where a task's limits let its servers listen, `kule exec`
/// keeps them off the network while the command runs: Kule holds their
/// port on this computer's network addresses.
#[cfg(target_os = "macos")]
#[test]
fn exec_shields_its_servers_while_it_runs() {
    let outward = {
        let probe = std::net::UdpSocket::bind("0.0.0.0:0").unwrap();
        probe
            .connect("192.0.2.1:9")
            .ok()
            .and_then(|()| probe.local_addr().ok())
            .map(|a| a.ip())
            .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
    };
    let Some(outward) = outward else {
        eprintln!("no network address to try from; skipped");
        return;
    };
    let s = Setup::new();
    s.ok(&["limits", "--task", &s.id, "servers=on"], "");
    // Listening servers in proxy mode are weaker isolation, taken knowingly.
    s.ok(&["isolation", "--accept-weaker", "on"], "");
    // Listens on every address until told it is done, a minute at most.
    let script = "import os, socket, time\ns=socket.socket()\ns.bind(('0.0.0.0',0))\ns.listen()\nopen('port.tmp','w').write(str(s.getsockname()[1]))\nos.rename('port.tmp','port')\nend=time.time()+60\nwhile time.time()<end and not os.path.exists('done'): time.sleep(0.05)";
    let exec = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(["exec", &s.id, "--", "python3", "-I", "-c", script])
        .env("KULE_HOME", s.home.path())
        .env("HOME", s.person.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let port_file = s.copy.join("port");
    let mut port = None;
    // Starting takes its time on a busy machine.
    for _ in 0..3000 {
        if let Ok(text) = fs::read_to_string(&port_file) {
            port = text.trim().parse::<u16>().ok();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let port = port.expect("the server never listened");
    let mut held = false;
    for _ in 0..100 {
        if std::net::TcpListener::bind((outward, port)).is_err() {
            held = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    fs::write(s.copy.join("done"), "").unwrap();
    // Ended either way: when this test's own look held the port in the
    // moment Kule went for it, Kule stops the server instead.
    let _ = exec.wait_with_output();
    assert!(held, "the port was never held");
}

/// A copy Kule cannot read back after the shell (a folder made unreadable
/// in it) hides nothing else: why the changes cannot be shown is said, the
/// shell's own exit code comes back, and a shell that exited well fails,
/// since part of what it did went untold.
#[test]
fn what_cannot_be_shown_after_the_shell_hides_nothing_else() {
    struct Unlock(PathBuf);
    impl Drop for Unlock {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
        }
    }
    let s = Setup::new();
    let _unlock = Unlock(s.copy.join("locked"));
    for (code, expected) in [(3, 3), (0, 1)] {
        let out = s.kule(
            &["shell", &s.id],
            &format!("mkdir -p locked && chmod 000 locked\nexit {code}\n"),
        );
        let text = said(&out);
        assert_eq!(out.status.code(), Some(expected), "{text}");
        assert!(
            text.contains("kule: what the shell changed cannot be shown"),
            "{text}"
        );
    }
}
