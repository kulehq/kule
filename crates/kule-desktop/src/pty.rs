//! Terminals inside the window: the person's shell in a task's copy, in a
//! pseudo-terminal, inside the task's sandbox, as `kule exec` runs a
//! command (see `kule_daemon::shell`). What the agent wrote there runs no
//! less sandboxed for the person's prompt or commands reading it, and what
//! the shell changes in the copy is reviewed like the agent's work. The
//! window draws it with xterm.js; this side starts the shell, passes keys
//! in and output out, and ends it.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use base64::Engine;
use kule_daemon::shell::Sandboxed;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// More than anyone keeps open at once.
const MAX_OPEN: usize = 16;

struct Term {
    /// The terminal's leader end: what the person types goes in here, and
    /// its size is set here.
    leader: File,
    /// The process Kule started, which leads the shell's process group.
    pid: i32,
}

#[derive(Default)]
pub struct Terminals {
    next: AtomicU64,
    open: Mutex<HashMap<u64, Term>>,
    /// Each shell's end, done once it is: what its sandbox refused it is
    /// recorded there (see [`Terminals::end_all`]).
    ending: Mutex<Vec<std::thread::JoinHandle<()>>>,
}

/// What the window hears about a terminal: output, as base64 so a cut
/// inside a character survives, or that its shell ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Event {
    Output {
        id: u64,
        data: String,
    },
    /// `notes`: what the sandbox refused the shell and what Kule ended or
    /// could not do as it ended, a line each (see `ShellEnd::lines`).
    /// `lost`: when the system log dropped the kernel's messages while
    /// the shell ran (see `ShellEnd::lost`), told apart and quietly.
    Exit {
        id: u64,
        code: Option<u32>,
        notes: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        lost: Option<String>,
    },
    /// What Kule does before the shell starts and that takes a while, for
    /// the tab the window opened as `key`: it knows no id yet.
    Preparing {
        key: u64,
        text: String,
    },
}

/// Makes task `task`'s package caches before its shell's sandbox does,
/// telling the tab `key` while that takes a while. What fails here is
/// tried again, and refused, as the sandbox is made.
fn prepare(home: &Path, task: &str, key: u64, tell: &impl Fn(Event)) {
    let _ = kule_daemon::shell::prepare_caches(home, task, |text| {
        tell(Event::Preparing {
            key,
            text: text.to_owned(),
        });
    });
}

fn size(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows.clamp(2, 1000),
        ws_col: cols.clamp(2, 1000),
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// A new pseudo-terminal of `cols` × `rows`: its leader and follower ends.
fn openpty(cols: u16, rows: u16) -> Result<(OwnedFd, OwnedFd), String> {
    let (mut leader, mut follower) = (0, 0);
    let mut ws = size(cols, rows);
    // SAFETY: openpty fills the two descriptors; no name is asked for,
    // the modes are the system's, the size is read from `ws`.
    let made = unsafe {
        libc::openpty(
            &mut leader,
            &mut follower,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            // Mutable on macOS, read-only on Linux.
            std::ptr::addr_of_mut!(ws),
        )
    };
    if made != 0 {
        return Err(format!(
            "a terminal could not open: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: openpty made both, and nothing else owns them.
    let (leader, follower) =
        unsafe { (OwnedFd::from_raw_fd(leader), OwnedFd::from_raw_fd(follower)) };
    // Neither end reaches what the app starts later.
    for fd in [&leader, &follower] {
        // SAFETY: fcntl on a descriptor this function owns.
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    Ok((leader, follower))
}

/// What a terminal is, for its tab: its id and the shell's name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Opened {
    pub id: u64,
    pub shell: String,
    /// What Kule withheld from the shell, for the window to say first.
    pub notes: Vec<String>,
}

/// The name of the shell [`shell`] starts: `zsh`, `bash`.
pub fn shell_name() -> String {
    Path::new(&shell().0)
        .file_stem()
        .map_or_else(|| "shell".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// The person's shell, started as a login shell so it sets up what their
/// terminal app would.
fn shell() -> (String, Vec<String>) {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| Path::new(s).is_absolute())
        .unwrap_or_else(|| "/bin/sh".to_owned());
    (shell, vec!["-l".to_owned()])
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
        // Handed back with the id rather than sent as output: what is sent
        // before the window knows the id is never shown.
        let notes = kule_daemon::shell::Task::open(home, task)
            .and_then(|t| t.start_notes())
            .unwrap_or_default();
        Ok((id, notes))
    }

    /// Starts `program` with `args` in a pseudo-terminal, in task `task`'s
    /// sandbox; `tell` hears what it writes and when it ends.
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
        let (leader, follower) = openpty(cols, rows)?;
        let mut cmd = sandboxed.command(program, args, follower.as_fd())?;
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // macOS's ls colours only with CLICOLOR; a value of the person's
        // own stands.
        if std::env::var_os("CLICOLOR").is_none() {
            cmd.env("CLICOLOR", "1");
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("the shell could not start: {e}"))?;
        // Only the shell holds the follower end now, so its leader end
        // reads an end once the shell and what it left are gone.
        drop(cmd);
        drop(follower);
        let leader = File::from(leader);
        let mut reader = leader.try_clone().map_err(|e| e.to_string())?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                id,
                Term {
                    leader,
                    pid: child.id() as i32,
                },
            );
        let tell = std::sync::Arc::new(tell);
        let (read_out, all_read) = std::sync::mpsc::channel::<()>();
        let output = std::sync::Arc::clone(&tell);
        std::thread::spawn(move || {
            let b64 = base64::engine::general_purpose::STANDARD;
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => output(Event::Output {
                        id,
                        data: b64.encode(&buf[..n]),
                    }),
                }
            }
            let _ = read_out.send(());
        });
        // The shell's end, not its terminal's: what it left running in the
        // background may hold the terminal on, but the task's work goes on
        // once the shell is gone (a commit ends what was left).
        let ending = std::thread::spawn(move || {
            let status = child.wait().ok();
            // What it wrote last, before its end is told.
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
        let term = open.get_mut(&id).ok_or("that terminal has ended")?;
        term.leader
            .write_all(data.as_bytes())
            .and_then(|_| term.leader.flush())
            .map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: u64, cols: u16, rows: u16) -> Result<(), String> {
        let open = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let term = open.get(&id).ok_or("that terminal has ended")?;
        let ws = size(cols, rows);
        // SAFETY: TIOCSWINSZ reads the size from `ws`.
        if unsafe { libc::ioctl(term.leader.as_raw_fd(), libc::TIOCSWINSZ, &ws) } == -1 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
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
    /// `within` for each end to be done, so what the sandbox refused them is
    /// recorded and their tasks are let go of.
    pub fn end_all(&self, within: Duration) {
        let deadline = std::time::Instant::now() + within;
        let open: Vec<u64> = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .copied()
            .collect();
        for id in open {
            self.close(id);
        }
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

    /// Ends terminal `id`'s shell, and what it runs in front.
    pub fn close(&self, id: u64) {
        if let Some(term) = self
            .open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id)
        {
            // SAFETY: kill only sends a signal; a negative pid names the
            // group the process Kule started leads.
            unsafe {
                libc::kill(-term.pid, libc::SIGKILL);
            }
            drop(term.leader);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

    /// Kule's home with one task, on a folder of its own.
    struct Setup {
        home: tempfile::TempDir,
        _ws: tempfile::TempDir,
        outside: tempfile::TempDir,
        task: String,
    }

    impl Setup {
        fn new() -> Setup {
            // Kule's home is hidden from the sandbox, and on Linux hiding
            // it inside /tmp would hide /tmp itself; the folder standing
            // for the rest of the computer is outside /tmp too, which
            // commands may write to.
            let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target");
            std::fs::create_dir_all(base).unwrap();
            let home = tempfile::TempDir::new_in(base).unwrap();
            let ws = tempfile::TempDir::new().unwrap();
            std::fs::write(ws.path().join("README.md"), "hello\n").unwrap();
            let service = kule_daemon::Service::new(home.path().to_owned());
            let task = service.create(ws.path()).unwrap().id;
            Setup {
                home,
                _ws: ws,
                outside: tempfile::TempDir::new_in(base).unwrap(),
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
                    "/bin/sh",
                    &["-c".to_owned(), script.to_owned()],
                    80,
                    24,
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

    /// Everything terminal output said until `want` shows, decoded.
    fn until(rx: &mpsc::Receiver<Event>, want: &str) -> String {
        let b64 = base64::engine::general_purpose::STANDARD;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = String::new();
        while !seen.contains(want) {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(Event::Output { data, .. }) => {
                    seen.push_str(&String::from_utf8_lossy(&b64.decode(data).unwrap()))
                }
                Ok(Event::Preparing { .. }) => {}
                Ok(Event::Exit { .. }) | Err(_) => break,
            }
        }
        seen
    }

    /// Whether the shell's end is told, waiting as long as its end may
    /// take: on macOS what the sandbox refused is read from the system log
    /// first, for up to its wait and then some on a busy machine.
    fn ended(rx: &mpsc::Receiver<Event>) -> bool {
        loop {
            match rx.recv_timeout(Duration::from_secs(40)) {
                Ok(Event::Exit { .. }) => break true,
                Ok(_) => continue,
                Err(_) => break false,
            }
        }
    }

    #[test]
    fn a_terminal_runs_in_the_copy_and_takes_what_is_typed() {
        let s = Setup::new();
        let terms = Terminals::default();
        let (id, rx) = s.start(
            &terms,
            "printf 'in:%s task:%s colour:%s\\n' \"$(pwd -P)\" \"$KULE_TASK\" \"$CLICOLOR\"; read line; printf 'got:%s\\n' \"$line\"; stty size",
        );
        let task = kule_daemon::Service::new(s.home.path().to_owned());
        let copy = std::fs::canonicalize(task.copy_of(&s.task).unwrap()).unwrap();
        // The person's CLICOLOR stands; without one, ls is told to colour.
        let colour = std::env::var("CLICOLOR").unwrap_or_else(|_| "1".to_owned());
        let out = until(&rx, &format!("colour:{colour}"));
        assert!(
            out.contains(&format!(
                "in:{} task:{} colour:{colour}",
                copy.display(),
                s.task
            )),
            "{out:?}"
        );

        terms.resize(id, 100, 30).unwrap();
        terms.write(id, "hello\n").unwrap();
        let out = until(&rx, "30 100");
        assert!(out.contains("got:hello"), "{out:?}");
        assert!(out.contains("30 100"), "{out:?}");

        // Its end is told, and it can no longer be written to once closed.
        assert!(ended(&rx));
        terms.close(id);
        assert!(terms.write(id, "x").is_err());
    }

    /// The shell runs in the task's sandbox: nothing it runs writes outside
    /// the copy, whatever the agent left there for it to run.
    #[test]
    fn a_terminal_runs_in_the_tasks_sandbox() {
        let s = Setup::new();
        let terms = Terminals::default();
        let outside = std::fs::canonicalize(s.outside.path()).unwrap();
        let (_, rx) = s.start(
            &terms,
            &format!(
                "if echo x > '{}/escaped' 2>/dev/null; then echo wrote; else echo refused; fi; echo done",
                outside.display()
            ),
        );
        let out = until(&rx, "done");
        assert!(out.contains("refused"), "{out:?}");
        assert!(!outside.join("escaped").exists());
    }

    /// The notes the shell's end came with, waiting a while for it.
    fn end_notes(rx: &mpsc::Receiver<Event>) -> Vec<String> {
        loop {
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(Event::Exit { notes, .. }) => break notes,
                Ok(_) => continue,
                Err(_) => panic!("the shell never ended"),
            }
        }
    }

    /// As the shell ends, its terminal says what the sandbox refused it and
    /// what Kule ended that it left running (macOS), or that the platform
    /// does not report refusals when the shell failed (Linux).
    #[test]
    fn a_shell_s_end_tells_what_the_sandbox_did() {
        let s = Setup::new();
        let terms = Terminals::default();
        let outside = std::fs::canonicalize(s.outside.path()).unwrap();
        let target = outside.join("escaped");
        let (_, rx) = s.start(
            &terms,
            &format!(
                "echo x > '{}' 2>/dev/null; nohup sleep 30 </dev/null >/dev/null 2>&1 & sleep 1; exit 3",
                target.display()
            ),
        );
        let notes = end_notes(&rx);
        assert!(!target.exists());
        if cfg!(target_os = "macos") {
            assert!(
                notes
                    .iter()
                    .any(|n| n.starts_with("sandbox: refused")
                        && n.contains(&*target.to_string_lossy())),
                "{notes:?}"
            );
            assert!(
                notes.iter().any(|n| n.starts_with("Kule ended sleep,")),
                "{notes:?}"
            );
        } else {
            assert!(
                notes
                    .iter()
                    .any(|n| n.starts_with("Linux does not tell Kule")),
                "{notes:?}"
            );
        }
    }

    /// While a terminal is open on the copy, no agent starts work in the
    /// task and nothing is committed; once its shell ends, both may.
    #[test]
    fn an_open_terminal_holds_the_task_until_its_shell_ends() {
        let s = Setup::new();
        let terms = Terminals::default();
        let (id, rx) = s.start(&terms, "printf up; sleep 60");
        until(&rx, "up");
        let task = kule_engine_task(&s);
        assert!(!task.terminals_open().is_empty());
        let started = Instant::now();
        terms.close(id);
        // Ended at once, not when its sleep would have: what is left to
        // wait for is the system log (see `ended`).
        assert!(ended(&rx) && started.elapsed() < Duration::from_secs(40));
        assert!(task.terminals_open().is_empty());
    }

    /// No terminal opens while an agent is at work in the task.
    #[test]
    fn no_terminal_while_an_agent_is_at_work() {
        let s = Setup::new();
        let at_work = kule_engine_task(&s).agent_at_work().unwrap();
        let e = Terminals::default()
            .start(s.home.path(), &s.task, "/bin/sh", &[], 80, 24, |_| {})
            .unwrap_err();
        assert!(e.contains("agent is at work"), "{e}");
        drop(at_work);
    }

    /// As the app quits with a terminal open, its shell is ended and what
    /// the sandbox refused it is still recorded (macOS): the task's next
    /// run and its Timeline hear of it, and the task is let go of.
    #[test]
    fn quitting_with_a_terminal_open_still_records_its_refusals() {
        let s = Setup::new();
        let terms = Terminals::default();
        let target = std::fs::canonicalize(s.outside.path())
            .unwrap()
            .join("escaped");
        let (_, rx) = s.start(
            &terms,
            &format!(
                "echo x > '{}' 2>/dev/null; printf up; sleep 60",
                target.display()
            ),
        );
        until(&rx, "up");
        // What keeps the app from going until its end is recorded.
        assert!(terms.any());
        let started = Instant::now();
        terms.end_all(Duration::from_secs(30));
        assert!(!terms.any());
        let task = kule_engine_task(&s);
        assert!(task.terminals_open().is_empty());
        assert!(started.elapsed() < Duration::from_secs(30));
        if cfg!(target_os = "macos") {
            let recorded = kule_daemon::Service::new(s.home.path().to_owned())
                .timeline(&s.task)
                .unwrap()
                .denials;
            assert!(
                recorded
                    .iter()
                    .any(|d| std::path::Path::new(&d.path) == target),
                "{recorded:?}"
            );
        }
    }

    /// A tab whose task's caches are still being made says so, by its
    /// key, before its shell starts.
    #[test]
    fn a_tab_says_it_waits_for_its_tasks_caches() {
        let s = Setup::new();
        let lock = s
            .home
            .path()
            .join("tasks")
            .join(&s.task)
            .join("caches.lock");
        let held = File::create(&lock).unwrap();
        // As Kule's own does: flock, which goes with the file.
        // SAFETY: `held` is an open descriptor for the call.
        assert_eq!(
            unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&held), libc::LOCK_EX) },
            0
        );
        let (tx, rx) = mpsc::channel();
        let (home, task) = (s.home.path().to_owned(), s.task.clone());
        let waiting = std::thread::spawn(move || {
            prepare(&home, &task, 7, &move |e| {
                let _ = tx.send(e);
            });
        });
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Event::Preparing { key, text } => {
                assert_eq!(key, 7);
                assert!(
                    text.contains("still preparing this task's package caches"),
                    "{text}"
                );
            }
            other => panic!("{other:?}"),
        }
        drop(held);
        waiting.join().unwrap();
    }

    fn kule_engine_task(s: &Setup) -> kule_daemon::shell::Task {
        kule_daemon::shell::Task::open(s.home.path(), &s.task).unwrap()
    }
}
