//! macOS: what a sandboxed command can ask the system's services to do for
//! it. Nothing it asks may run, or write, outside the sandbox: no app opened,
//! no background job, no other program's settings, not the pasteboard, not
//! another terminal. Every test checks for the effect itself, outside.
#![cfg(target_os = "macos")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use kule_engine::{Sandbox, Task};
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    outside: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Self {
        let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws = TempDir::new().unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            outside: TempDir::new().unwrap(),
            task,
        }
    }

    fn mark(&self) -> PathBuf {
        fs::canonicalize(self.outside.path()).unwrap().join("ran")
    }

    fn sh(&self, script: &str) -> Output {
        Sandbox::new(&self.task)
            .unwrap()
            .command("sh", &["-c".to_owned(), script.to_owned()])
            .unwrap()
            .output()
            .unwrap()
    }
}

/// Whether `path` shows up within a few seconds.
fn appears(path: &Path) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(4) {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

fn unique(name: &str) -> String {
    format!(
        "com.kule.test.{name}.{}.{}",
        std::process::id(),
        rand_suffix()
    )
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn an_app_opened_from_the_sandbox_does_not_start() {
    let s = Setup::new();
    let app = s.task.copy().join("Probe.app/Contents");
    fs::create_dir_all(app.join("MacOS")).unwrap();
    fs::write(
        app.join("Info.plist"),
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>probe</string>
<key>CFBundleIdentifier</key><string>{}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSBackgroundOnly</key><true/>
</dict></plist>"#,
            unique("app")
        ),
    )
    .unwrap();
    let probe = app.join("MacOS/probe");
    fs::write(
        &probe,
        format!("#!/bin/sh\ntouch '{}'\n", s.mark().display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o755)).unwrap();

    s.sh("open -g -j -n Probe.app");
    assert!(!appears(&s.mark()), "an app opened from the sandbox ran");
}

#[test]
fn a_background_job_submitted_from_the_sandbox_never_runs() {
    let s = Setup::new();
    let label = unique("job");
    let out = s.sh(&format!(
        "launchctl submit -l {label} -- /usr/bin/touch '{}'",
        s.mark().display()
    ));
    let ran = appears(&s.mark());
    let _ = Command::new("launchctl").args(["remove", &label]).output();
    eprintln!("{out:?}");
    assert!(!ran, "a job submitted from the sandbox ran: {out:?}");
}

#[test]
fn other_programs_settings_cannot_be_written() {
    let s = Setup::new();
    let domain = unique("prefs");
    s.sh(&format!("defaults write {domain} written yes"));
    std::thread::sleep(Duration::from_millis(300));
    let read = Command::new("defaults")
        .args(["read", &domain, "written"])
        .output()
        .unwrap();
    let _ = Command::new("defaults").args(["delete", &domain]).output();
    assert!(
        !read.status.success(),
        "a setting written from the sandbox was kept"
    );
}

#[test]
fn the_pasteboard_is_out_of_reach() {
    let s = Setup::new();
    let pbcopy = |text: &str| {
        let mut child = Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(child.stdin.as_mut().unwrap(), text.as_bytes()).unwrap();
        child.wait().unwrap().success()
    };
    let pbpaste = || Command::new("pbpaste").output().unwrap().stdout;
    let kept = pbpaste();
    let secret = format!("kule-test-{}", rand_suffix());
    if !pbcopy(&secret) || pbpaste() != secret.as_bytes() {
        eprintln!("skipped: no pasteboard here");
        return;
    }
    let read = s.sh("pbpaste");
    let written = s.sh("printf replaced | pbcopy");
    let after = pbpaste();
    pbcopy(&String::from_utf8_lossy(&kept));
    assert!(
        !String::from_utf8_lossy(&read.stdout).contains(&secret),
        "the pasteboard was read from the sandbox"
    );
    assert_eq!(
        after,
        secret.as_bytes(),
        "the pasteboard was written: {written:?}"
    );
}

#[test]
fn another_terminal_cannot_be_written_to() {
    let s = Setup::new();
    let (mut leader, mut follower) = (0, 0);
    // SAFETY: openpty writes the two descriptors it is given.
    let made = unsafe {
        libc::openpty(
            &mut leader,
            &mut follower,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(made, 0);
    // SAFETY: follower is an open terminal; ttyname returns a static string.
    let name = unsafe { std::ffi::CStr::from_ptr(libc::ttyname(follower)) }
        .to_string_lossy()
        .into_owned();
    assert!(name.starts_with("/dev/ttys"), "{name}");
    // Read what reaches the terminal, as the person's would show it; a
    // terminal no one reads would hold up the writer's exit.
    let reader = std::thread::spawn(move || {
        let mut seen = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            // SAFETY: reading into a buffer of the length given.
            let n = unsafe { libc::read(leader, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                break;
            }
            seen.extend_from_slice(&buf[..n as usize]);
            if seen.windows(5).any(|w| w == b"hello") {
                break;
            }
        }
        seen
    });
    let out = s.sh(&format!("printf hello > {name}"));
    // SAFETY: closing the descriptor openpty gave; the reader then ends.
    unsafe { libc::close(follower) };
    let seen = reader.join().unwrap();
    // SAFETY: as above.
    unsafe { libc::close(leader) };
    assert!(!out.status.success(), "{name} was opened from the sandbox");
    assert!(
        !seen.windows(5).any(|w| w == b"hello"),
        "{name} was written from the sandbox"
    );
}

#[test]
fn what_is_typed_in_another_terminal_cannot_be_read() {
    let s = Setup::new();
    let (mut leader, mut follower) = (0, 0);
    // SAFETY: openpty writes the two descriptors it is given.
    let made = unsafe {
        libc::openpty(
            &mut leader,
            &mut follower,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(made, 0);
    // SAFETY: follower is an open terminal; ttyname returns a static string.
    let name = unsafe { std::ffi::CStr::from_ptr(libc::ttyname(follower)) }
        .to_string_lossy()
        .into_owned();
    // Keys typed while the sandboxed command waits on the terminal.
    let typing = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        let keys = b"secret\n";
        // SAFETY: writing a buffer of the length given.
        unsafe { libc::write(leader, keys.as_ptr().cast(), keys.len()) };
        leader
    });
    let out = Sandbox::new(&s.task)
        .unwrap()
        .command(
            "sh",
            &[
                "-c".to_owned(),
                format!("head -n 1 < {name} & sleep 2; kill $! 2>/dev/null"),
            ],
        )
        .unwrap()
        .output()
        .unwrap();
    let leader = typing.join().unwrap();
    // SAFETY: closing the descriptors openpty gave.
    unsafe {
        libc::close(follower);
        libc::close(leader);
    }
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("secret"),
        "{name} was read from the sandbox"
    );
}

/// The person's processes: neither signalled nor looked into, their
/// environment least of all (where API keys live).
#[test]
fn processes_outside_the_sandbox_cannot_be_signalled_or_read() {
    let s = Setup::new();
    let secret = format!("kule-test-{}", rand_suffix());
    let mut outside = Command::new("sleep")
        .arg("30")
        .env("KULE_TEST_SECRET", &secret)
        .spawn()
        .unwrap();
    let pid = outside.id();
    let signalled = s.sh(&format!("kill -STOP {pid}"));
    let looked = s.sh(&format!("ps eww -p {pid}"));
    let stopped = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let _ = outside.kill();
    let _ = outside.wait();
    assert!(
        !signalled.status.success(),
        "a process outside was signalled"
    );
    assert!(
        !String::from_utf8_lossy(&stopped.stdout).contains('T'),
        "a process outside was stopped"
    );
    assert!(
        !String::from_utf8_lossy(&looked.stdout).contains(&secret),
        "a process outside had its environment read"
    );
}

/// With its network held to the proxy, a command cannot have the system's
/// certificate services fetch an address for it: a certificate that names
/// its issuer at a listener outside is checked, and the listener hears
/// nothing.
#[test]
fn certificate_checks_fetch_nothing_past_the_proxy() {
    let s = Setup::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let dir = fs::canonicalize(s.task.copy()).unwrap();
    let openssl = |args: &[&str]| {
        let out = Command::new("/usr/bin/openssl")
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    };
    openssl(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "ca.key",
        "-out",
        "ca.pem",
        "-days",
        "2",
        "-subj",
        "/CN=Kule test CA",
    ]);
    openssl(&[
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        "leaf.key",
        "-out",
        "leaf.csr",
        "-subj",
        "/CN=leaf.kule.test",
    ]);
    fs::write(
        dir.join("ext.cnf"),
        format!(
            "authorityInfoAccess=caIssuers;URI:http://127.0.0.1:{port}/issuer.cer,OCSP;URI:http://127.0.0.1:{port}/ocsp\n\
             crlDistributionPoints=URI:http://127.0.0.1:{port}/crl\n"
        ),
    )
    .unwrap();
    openssl(&[
        "x509",
        "-req",
        "-in",
        "leaf.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-out",
        "leaf.pem",
        "-days",
        "2",
        "-extfile",
        "ext.cnf",
    ]);

    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.set_network(kule_engine::Network::Deny);
    let _ = sandbox
        .command("sh", &["-c".to_owned(), "security verify-cert -c leaf.pem -p ssl -s leaf.kule.test; security verify-cert -c leaf.pem -p basic".to_owned()])
        .unwrap()
        .output()
        .unwrap();
    let started = Instant::now();
    let mut heard = false;
    while started.elapsed() < Duration::from_secs(4) {
        if listener.accept().is_ok() {
            heard = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !heard,
        "a certificate check fetched an address past the proxy"
    );
}

/// The tools agents use every day keep working under all of the above.
#[test]
fn everyday_tools_still_work() {
    let s = Setup::new();
    let has = |tool: &str| {
        Command::new("sh")
            .args(["-c", &format!("command -v {tool}")])
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let mut scripts = vec![
        "git init -q r && cd r && git -c user.name=k -c user.email=k@k commit -q --allow-empty -m one && git log --oneline | wc -l",
        "python3 -c 'import json, sqlite3, ssl; print(1)'",
        "python3 -c 'from multiprocessing import shared_memory as s; m=s.SharedMemory(create=True, size=8); m.buf[0]=1; print(m.buf[0]); m.close(); m.unlink()'",
        // PostgreSQL's dynamic shared memory, by the name it gives it.
        "python3 -c 'import _posixshmem, os; n=\"/PostgreSQL.%d\" % os.getpid(); os.close(_posixshmem.shm_open(n, os.O_CREAT | os.O_RDWR, 0o600)); _posixshmem.shm_unlink(n); print(1)'",
    ];
    if has("node") {
        scripts.push("node -e 'require(\"crypto\").randomBytes(4); console.log(1)'");
    }
    if has("xcrun") {
        scripts.push(
            "printf 'int main(void){return 0;}' > m.c && xcrun clang m.c -o m && ./m && echo 1",
        );
    }
    if has("cargo") {
        scripts.push("cargo --version >/dev/null && echo 1");
    }
    for script in scripts {
        let out = s.sh(script);
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).trim().ends_with('1'),
            "{script}: {out:?}"
        );
    }
}

#[test]
fn a_terminal_made_inside_the_sandbox_still_works() {
    let s = Setup::new();
    let out = s.sh(
        "python3 -c \"import os,pty; l,f=pty.openpty(); n=os.ttyname(f); fd=os.open(n, os.O_RDWR); os.write(fd, b'hi'); print(os.read(l, 2).decode())\"",
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
}

/// Deletes a keychain made for a test, whatever the test does.
struct Keychain(PathBuf);

impl Drop for Keychain {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/security")
            .arg("delete-keychain")
            .arg(&self.0)
            .output();
    }
}

/// The keychain's service opens items for its caller, outside the sandbox:
/// a keychain left unlocked outside gives a command nothing, unless the
/// keychain's files were revealed to it (an agent's own login).
#[test]
fn an_unlocked_keychain_opens_only_when_revealed() {
    let s = Setup::new();
    let keychain = Keychain(
        fs::canonicalize(s.outside.path())
            .unwrap()
            .join("probe.keychain-db"),
    );
    let secret = format!("kule-test-{}", rand_suffix());
    let security = |args: &[&str]| {
        let out = Command::new("/usr/bin/security")
            .args(args)
            .arg(&keychain.0)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    };
    security(&["create-keychain", "-p", "kule-test"]);
    security(&[
        "add-generic-password",
        "-s",
        "kule-probe",
        "-a",
        "kule",
        "-w",
        &secret,
    ]);
    let read = format!(
        "security find-generic-password -s kule-probe -w '{}'",
        keychain.0.display()
    );

    let out = s.sh(&read);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains(&secret),
        "an unlocked keychain was opened from the sandbox"
    );

    let home = TempDir::new().unwrap();
    let keychains = home.path().join("Library/Keychains");
    fs::create_dir_all(&keychains).unwrap();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide(&keychains).unwrap();
    sandbox.reveal(&keychains).unwrap();
    let out = sandbox
        .command("sh", &["-c".to_owned(), read])
        .unwrap()
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(&secret),
        "a revealed keychain did not open: {out:?}"
    );
}

/// Shared memory goes by name: a segment a process outside holds cannot be
/// opened from the sandbox (a command's own still can, see
/// `everyday_tools_still_work`).
#[test]
fn shared_memory_held_outside_is_out_of_reach() {
    let s = Setup::new();
    let name = format!("kule_t_{}", rand_suffix() % 1_000_000_000);
    let mut outside = Command::new("python3")
        .args([
            "-I",
            "-c",
            "import sys\n\
             from multiprocessing import shared_memory as s\n\
             m = s.SharedMemory(name=sys.argv[1], create=True, size=64)\n\
             m.buf[:6] = b'kule42'\n\
             print('ready', flush=True)\n\
             sys.stdin.read()\n\
             m.close(); m.unlink()",
            &name,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    std::io::BufRead::read_line(
        &mut std::io::BufReader::new(outside.stdout.as_mut().unwrap()),
        &mut ready,
    )
    .unwrap();
    assert_eq!(ready.trim(), "ready");

    let out = s.sh(&format!(
        "python3 -I -c \"import _posixshmem, mmap, os; fd = _posixshmem.shm_open('/{name}', os.O_RDONLY); print(mmap.mmap(fd, 64, prot=mmap.PROT_READ)[:6])\""
    ));
    drop(outside.stdin.take());
    let _ = outside.wait();
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("kule42"),
        "shared memory held outside was read from the sandbox"
    );
}

/// A command cannot keep the computer awake: the power service that holds
/// such an assertion sits outside the sandbox.
#[test]
fn the_computer_cannot_be_kept_awake() {
    let s = Setup::new();
    let pid_file = fs::canonicalize(s.task.copy())
        .unwrap()
        .join("caffeinate.pid");
    let held = std::thread::scope(|scope| {
        let run = scope.spawn(|| s.sh("caffeinate -i -t 3 & echo $! > caffeinate.pid; wait"));
        let started = Instant::now();
        let mut held = false;
        while !held && started.elapsed() < Duration::from_secs(3) {
            if let Ok(pid) = fs::read_to_string(&pid_file) {
                let assertions = Command::new("pmset")
                    .args(["-g", "assertions"])
                    .output()
                    .unwrap();
                held = String::from_utf8_lossy(&assertions.stdout)
                    .contains(&format!("pid {}(caffeinate)", pid.trim()));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = run.join();
        held
    });
    assert!(pid_file.exists(), "the command did not run");
    assert!(!held, "a command kept the computer awake");
}
