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
