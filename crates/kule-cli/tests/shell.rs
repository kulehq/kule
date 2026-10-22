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
