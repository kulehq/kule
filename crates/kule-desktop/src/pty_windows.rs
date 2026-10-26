//! Terminals inside the window, on Windows: the person's shell in a task's
//! copy, on a pseudo console that Kule's helper opens inside the task's
//! container, as `kule shell` runs it (see `kule_daemon::shell`). What the
//! agent wrote there runs no less sandboxed for the person's prompt or
//! commands reading it, and no part of the shell starts outside the
//! container. The window draws it with xterm.js; this side starts the
//! shell, passes keys in and output out, and ends it.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use kule_daemon::shell::Sandboxed;
use kule_daemon::shell::TerminalInput;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// More than anyone keeps open at once.
const MAX_OPEN: usize = 16;

#[derive(Default)]
pub struct Terminals {
    next: AtomicU64,
    /// What goes in to each open terminal; dropped, its shell ends.
    open: Mutex<HashMap<u64, TerminalInput>>,
    /// Each shell's end, done once it is (see [`Terminals::end_all`]).
    ending: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

/// What the window hears about a terminal (as `pty.rs` tells it).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Event {
    Output {
        id: u64,
        data: String,
    },
    Exit {
        id: u64,
        code: Option<u32>,
        notes: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lost: Option<String>,
    },
    Preparing {
        key: u64,
        text: String,
    },
}

/// Makes task `task`'s package caches before its shell's sandbox does,
/// telling the tab `key` while that takes a while.
fn prepare(home: &Path, task: &str, key: u64, tell: &impl Fn(Event)) {
    let _ = kule_daemon::shell::prepare_caches(home, task, |text| {
        tell(Event::Preparing {
            key,
            text: text.to_owned(),
        });
    });
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Opened {
    pub id: u64,
    pub shell: String,
    /// What Kule withheld from the shell, for the window to say first.
    pub notes: Vec<String>,
}

/// The name of the shell a terminal starts: `pwsh`, `powershell`, `cmd`.
pub fn shell_name() -> String {
    Path::new(&kule_daemon::osfs::default_shell())
        .file_stem()
        .map_or_else(|| "shell".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// The person's shell, with no banner where it prints one.
fn shell() -> (String, Vec<String>) {
    let shell = kule_daemon::osfs::default_shell()
        .to_string_lossy()
        .into_owned();
    let args = match shell_name().to_ascii_lowercase().as_str() {
        "pwsh" | "powershell" => vec!["-NoLogo".to_owned()],
        _ => Vec::new(),
    };
    (shell, args)
}

impl Terminals {
    /// Starts the person's shell in task `task`'s copy, in its sandbox;
    /// its id. The window hears its output and its end as `terminal`
    /// events. Refused while an agent is at work in the task.
    pub fn open<R: Runtime>(
        &self,
        app: AppHandle<R>,
        home: &Path,
        task: &str,
        cols: u16,
        rows: u16,
        key: u64,
    ) -> Result<(u64, Vec<String>), String> {
        let (program, args) = shell();
        let tell = move |event| {
            let _ = app.emit("terminal", event);
        };
        prepare(home, task, key, &tell);
        let id = self.start(home, task, &program, &args, cols, rows, tell)?;
        let notes = kule_daemon::shell::Task::open(home, task)
            .and_then(|t| t.home_withheld_notes())
            .unwrap_or_default();
        Ok((id, notes))
    }

    /// Starts `program` with `args` on a pseudo console in task `task`'s
    /// sandbox; `tell` hears what it shows and when it ends.
    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        home: &Path,
        task: &str,
        program: &str,
        args: &[String],
        cols: u16,
        rows: u16,
        tell: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<u64, String> {
        if self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
            >= MAX_OPEN
        {
            return Err(format!(
                "at most {MAX_OPEN} terminals at once; close one first"
            ));
        }
        let sandboxed = Sandboxed::new(home, task)?;
        let mut start = sandboxed.terminal(program, args, cols, rows)?;
        start.env("TERM", "xterm-256color");
        start.env("COLORTERM", "truecolor");
        let kule_daemon::shell::Terminal {
            input,
            output,
            errors,
            mut process,
        } = start
            .spawn()
            .map_err(|e| format!("the shell could not start: {e}"))?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id, input);
        let tell = std::sync::Arc::new(tell);
        let (read_out, all_read) = std::sync::mpsc::channel::<()>();
        // What the console shows, and what the helper says should the
        // shell not start, both drawn in the tab.
        for (stream, done) in [(output, Some(read_out)), (errors, None)] {
            let output = std::sync::Arc::clone(&tell);
            std::thread::spawn(move || {
                let mut stream = stream;
                let b64 = base64::engine::general_purpose::STANDARD;
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => output(Event::Output {
                            id,
                            data: b64.encode(&buf[..n]),
                        }),
                    }
                }
                if let Some(done) = done {
                    let _ = done.send(());
                }
            });
        }
        let ending = std::thread::spawn(move || {
            let status = process.wait().ok();
            // What it showed last, before its end is told.
            let _ = all_read.recv_timeout(Duration::from_millis(300));
            let code = status.and_then(|s| s.code()).map(|c| c as u32);
            let end = sandboxed.end();
            let notes = end.lines(code != Some(0));
            tell(Event::Exit {
                id,
                code,
                notes,
                lost: end.lost,
            });
        });
        let mut ends = self
            .ending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ends.retain(|end| !end.is_finished());
        ends.push(ending);
        Ok(id)
    }

    /// Passes what the person typed or pasted to terminal `id`.
    pub fn write(&self, id: u64, data: &str) -> Result<(), String> {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let input = open.get_mut(&id).ok_or("that terminal has ended")?;
        input.write(data.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: u64, cols: u16, rows: u16) -> Result<(), String> {
        let mut open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let input = open.get_mut(&id).ok_or("that terminal has ended")?;
        input.resize(cols, rows).map_err(|e| e.to_string())
    }

    /// Whether a terminal is open, or a closed one's shell is still ending.
    pub fn any(&self) -> bool {
        !self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty()
            || !self
                .ending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
    }

    /// As the app quits: ends every terminal's shell, and waits up to
    /// `within` for each end to be done.
    pub fn end_all(&self, within: Duration) {
        let deadline = std::time::Instant::now() + within;
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let ends = std::mem::take(
            &mut *self
                .ending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for end in ends {
            while !end.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if end.is_finished() {
                let _ = end.join();
            }
        }
    }

    /// Ends terminal `id`'s shell: its input closed, the helper ends it
    /// and everything in its container goes with it.
    pub fn close(&self, id: u64) {
        drop(
            self.open
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    struct Setup {
        home: tempfile::TempDir,
        _ws: tempfile::TempDir,
        outside: tempfile::TempDir,
        task: String,
    }

    impl Setup {
        fn new() -> Setup {
            let home = tempfile::TempDir::new().unwrap();
            let ws = tempfile::TempDir::new().unwrap();
            std::fs::write(ws.path().join("README.md"), "hello\n").unwrap();
            let service = kule_daemon::Service::new(home.path().to_owned());
            let task = service.create(ws.path()).unwrap().id;
            Setup {
                home,
                _ws: ws,
                outside: tempfile::TempDir::new().unwrap(),
                task,
            }
        }

        fn start(&self, terms: &Terminals, script: &str) -> (u64, mpsc::Receiver<Event>) {
            let (tx, rx) = mpsc::channel();
            let tx = Mutex::new(tx);
            let id = terms
                .start(
                    self.home.path(),
                    &self.task,
                    "cmd.exe",
                    &["/d".to_owned(), "/k".to_owned(), script.to_owned()],
                    200,
                    50,
                    move |e| {
                        let _ = tx
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .send(e);
                    },
                )
                .unwrap();
            (id, rx)
        }
    }

    /// Whether the sandbox can be had here (see the engine's
    /// `tests/terminal_windows.rs`).
    fn available() -> bool {
        let refused = kule_daemon::shell::sandbox_refusals();
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

    fn until(rx: &mpsc::Receiver<Event>, seen: &mut String, want: &str) -> bool {
        let b64 = base64::engine::general_purpose::STANDARD;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !seen.contains(want) {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(Event::Output { data, .. }) => {
                    seen.push_str(&String::from_utf8_lossy(&b64.decode(data).unwrap()))
                }
                Ok(Event::Preparing { .. }) => {}
                Ok(Event::Exit { .. }) | Err(_) => return false,
            }
        }
        true
    }

    fn ended(rx: &mpsc::Receiver<Event>) -> Option<Option<u32>> {
        loop {
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(Event::Exit { code, .. }) => break Some(code),
                Ok(_) => continue,
                Err(_) => break None,
            }
        }
    }

    /// The tab's shell runs in the copy, takes what is typed, writes
    /// nowhere outside the copy, and its end is told with its code.
    #[test]
    fn a_terminal_runs_in_the_tasks_sandbox_and_takes_what_is_typed() {
        if !available() {
            return;
        }
        let s = Setup::new();
        let terms = Terminals::default();
        let outside = kule_daemon::osfs::plain(&std::fs::canonicalize(s.outside.path()).unwrap())
            .join("escaped.txt");
        let (id, rx) = s.start(&terms, "set /a 6*7");
        let mut seen = String::new();
        assert!(until(&rx, &mut seen, "42"), "{seen}");
        terms
            .write(id, &format!("echo x> \"{}\"\r", outside.display()))
            .unwrap();
        terms.resize(id, 100, 30).unwrap();
        terms.write(id, "set /a 300+33\r").unwrap();
        assert!(until(&rx, &mut seen, "333"), "{seen}");
        assert!(!outside.exists(), "the shell wrote {}", outside.display());
        terms.write(id, "exit 3\r").unwrap();
        assert_eq!(ended(&rx), Some(Some(3)));
        terms.close(id);
        assert!(terms.write(id, "x").is_err());
    }

    /// Closing the tab ends its shell at once, whatever it runs, and lets
    /// the task go.
    #[test]
    fn closing_a_terminal_ends_its_shell_and_lets_the_task_go() {
        if !available() {
            return;
        }
        let s = Setup::new();
        let terms = Terminals::default();
        // Says it waits, then waits a minute; the word is made of two, so
        // the console's echo of the command does not show it.
        let (id, rx) = s.start(
            &terms,
            "powershell -NoProfile -Command \"'wait'+'ing'; Start-Sleep 60\"",
        );
        let mut seen = String::new();
        assert!(until(&rx, &mut seen, "waiting"), "{seen}");
        let task = kule_daemon::shell::Task::open(s.home.path(), &s.task).unwrap();
        assert!(!task.terminals_open().is_empty());
        let started = Instant::now();
        terms.close(id);
        assert!(ended(&rx).is_some());
        assert!(started.elapsed() < Duration::from_secs(30));
        assert!(task.terminals_open().is_empty());
        terms.end_all(Duration::from_secs(10));
        assert!(!terms.any());
    }
}
