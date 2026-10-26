//! The terminal on Windows: the person's shell on a pseudo console that
//! Kule's helper opens inside the command's container, with real keys in
//! and the real console's output out. The helper (`kule-pty-helper.exe`)
//! must be built beside the test programs first (`cargo build -p
//! kule-pty-helper`). Where no container can be made the tests say so and
//! return, unless `KULE_TEST_REQUIRE_SANDBOX` is set (see
//! `sandbox_windows.rs`).
#![cfg(windows)]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kule_engine::{Process, Sandbox, Task, TerminalInput};
use tempfile::TempDir;

struct Setup {
    home: TempDir,
    _ws: TempDir,
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
            home,
            _ws: ws,
            outside: TempDir::new().unwrap(),
            task,
        }
    }

    fn sandbox(&self) -> Sandbox {
        Sandbox::new(&self.task).unwrap()
    }
}

fn plain(path: &Path) -> PathBuf {
    kule_engine::osfs::plain(&fs::canonicalize(path).unwrap())
}

fn available() -> bool {
    let refused = kule_engine::sandbox_refusals();
    if refused.is_empty() {
        return true;
    }
    assert!(
        std::env::var_os("KULE_TEST_REQUIRE_SANDBOX").is_none(),
        "the sandbox is required here, but: {}",
        refused.join("; ")
    );
    eprintln!("skipped: {}", refused.join("; "));
    false
}

/// The helper Kule starts the terminal with: the one beside the test
/// programs.
fn helper() -> PathBuf {
    kule_engine::windows::beside_me("kule-pty-helper.exe")
        .expect("build kule-pty-helper first: cargo build -p kule-pty-helper")
}

/// A shell on a terminal, and all its console showed so far, escape
/// sequences left out.
struct Shell {
    input: Option<TerminalInput>,
    process: Process,
    shown: Arc<Mutex<String>>,
}

impl Shell {
    fn start(sandbox: &Sandbox) -> Shell {
        let start = sandbox
            .terminal_start("cmd.exe", &["/d".to_owned()], 200, 50)
            .unwrap();
        let terminal = start.spawn().unwrap();
        let shown = Arc::new(Mutex::new(String::new()));
        let mut output = terminal.output;
        let into = Arc::clone(&shown);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = output.read(&mut buf) {
                if n == 0 {
                    break;
                }
                into.lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        });
        Shell {
            input: Some(terminal.input),
            process: terminal.process,
            shown,
        }
    }

    fn typed(&mut self, keys: &str) {
        self.input.as_mut().unwrap().write(keys.as_bytes()).unwrap();
    }

    /// What the console showed once `want` shows, or panics after `secs`.
    fn until(&self, want: &str, secs: u64) -> String {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let seen = without_escapes(&self.shown.lock().unwrap());
            if seen.contains(want) {
                return seen;
            }
            assert!(
                Instant::now() < deadline,
                "{want:?} never showed; the console showed:\n{seen}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the shell shows its prompt again (the console's text
    /// ends with `>`), or panics after `secs`: what is typed while a
    /// command still runs may go to that command, or be dropped as it ends.
    fn until_prompt(&self, secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let seen = without_escapes(&self.shown.lock().unwrap());
            if seen.trim_end().ends_with('>') {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the prompt never came back; the console showed:\n{seen}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for a line that starts with `label` and ends with `value`
    /// (as `mode con` shows a size).
    fn until_line(&self, label: &str, value: &str, secs: u64) {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let seen = without_escapes(&self.shown.lock().unwrap());
            if seen.lines().any(|l| {
                let l = l.trim();
                l.starts_with(label) && l.trim_start_matches(label).trim() == value
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "no {label} {value} line; the console showed:\n{seen}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits up to `secs` for the shell to end; its exit code.
    fn ended(&mut self, secs: u64) -> Option<i32> {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if let Some(status) = self.process.try_wait().unwrap() {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

/// A command that says it waits, waits a minute, then says it woke: each
/// word made of two, so the console's echo of the command shows neither.
const WAITS: &str = "powershell -NoProfile -Command \"'wait'+'ing'; Start-Sleep 60; 'wo'+'ke'\"";

/// The console's text with its escape sequences (CSI and OSC) left out.
fn without_escapes(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The shell starts in the copy on a console of the size asked, takes
/// what is typed, follows a resize, and its exit code is the terminal's.
#[test]
fn a_terminal_runs_in_the_copy_and_takes_what_is_typed() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let mut shell = Shell::start(&s.sandbox());
    let copy = plain(&s.task.copy());
    // Arithmetic, so what shows is the shell's answer, not the keys the
    // console echoes.
    shell.typed("cd\r");
    shell.until(&copy.display().to_string(), 30);
    shell.typed("set /a 6*7\r");
    shell.until("42", 10);
    shell.typed("mode con\r");
    shell.until_line("Columns:", "200", 10);
    shell.until_line("Lines:", "50", 10);
    shell.input.as_mut().unwrap().resize(120, 40).unwrap();
    shell.typed("mode con\r");
    shell.until_line("Columns:", "120", 10);
    shell.until_line("Lines:", "40", 10);
    shell.typed("exit 3\r");
    assert_eq!(shell.ended(10), Some(3));
}

/// What the shell runs writes in the copy and nowhere else; the folder of
/// the helper it runs under is not written either.
#[test]
fn a_terminal_writes_only_in_the_copy() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let mut shell = Shell::start(&s.sandbox());
    let outside = plain(s.outside.path()).join("escaped.txt");
    let planted = plain(helper().parent().unwrap()).join("planted.txt");
    shell.typed("echo in> in.txt\r");
    shell.typed(&format!("echo x> \"{}\"\r", outside.display()));
    shell.typed(&format!("copy /y in.txt \"{}\"\r", planted.display()));
    shell.typed("set /a 300+33\r");
    shell.until("333", 30);
    shell.typed("exit\r");
    assert!(shell.ended(10).is_some());
    assert_eq!(
        fs::read_to_string(s.task.copy().join("in.txt"))
            .unwrap()
            .trim(),
        "in"
    );
    assert!(!outside.exists(), "the shell wrote {}", outside.display());
    let wrote = planted.exists();
    let _ = fs::remove_file(&planted);
    assert!(!wrote, "the shell wrote beside the helper");
}

/// Ctrl-C reaches what the shell runs, which ends, and the shell goes on.
#[test]
fn ctrl_c_ends_what_the_shell_runs_and_not_the_shell() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let mut shell = Shell::start(&s.sandbox());
    // Only Ctrl-C ends it before its minute (cmd's own `timeout` reads the
    // console in a way a pseudo console on a hosted runner did not give it).
    shell.typed(&format!("{WAITS}\r"));
    shell.until("waiting", 30);
    shell.typed("\x03");
    shell.until_prompt(15);
    shell.typed("set /a 600+66\r");
    let seen = shell.until("666", 15);
    assert!(
        !seen.contains("woke"),
        "Ctrl-C did not end the wait: {seen}"
    );
    shell.typed("exit 0\r");
    assert_eq!(shell.ended(10), Some(0));
}

/// Kule closing a terminal (its input ends) ends the shell, even while
/// what it runs reads no input.
#[test]
fn a_closed_terminal_ends_its_shell() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let mut shell = Shell::start(&s.sandbox());
    shell.typed(&format!("{WAITS}\r"));
    shell.until("waiting", 30);
    drop(shell.input.take());
    assert!(shell.ended(15).is_some(), "the shell outlived its terminal");
}

/// A helper in a folder the command could write to is refused, not
/// started. Breaks when the helper's folder is not checked against the
/// grants.
#[test]
fn a_helper_the_command_could_write_is_refused() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let mut sandbox = s.sandbox();
    let dir = plain(helper().parent().unwrap());
    sandbox.allow_write(&dir).unwrap();
    let e = sandbox
        .terminal_start("cmd.exe", &[], 80, 24)
        .unwrap_err()
        .to_string();
    assert!(e.contains("starts no terminal from there"), "{e}");
}

/// Where this computer gives no sandbox, no terminal opens either,
/// whatever the person accepted.
#[test]
fn no_sandbox_means_no_terminal() {
    let s = Setup::new();
    kule_engine::set_isolation(
        s.home.path(),
        &kule_engine::Isolation {
            accept_weaker: true,
        },
    )
    .unwrap();
    let mut sandbox = s.sandbox();
    sandbox.refuse_for_test("this is a test");
    let e = sandbox
        .terminal_start("cmd.exe", &[], 80, 24)
        .unwrap_err()
        .to_string();
    assert!(e.contains("this is a test"), "{e}");
}
