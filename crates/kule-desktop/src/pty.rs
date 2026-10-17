//! Terminals inside the window: the person's shell in a task's copy, in a
//! pseudo-terminal, outside the sandbox, as `Open terminal` starts one in
//! their own terminal app. What it changes in the copy is reviewed like the
//! agent's work. The window draws it with xterm.js; this side starts the
//! shell, passes keys in and output out, and ends it.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// More than anyone keeps open at once.
const MAX_OPEN: usize = 16;

struct Term {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

#[derive(Default)]
pub struct Terminals {
    next: AtomicU64,
    open: Mutex<HashMap<u64, Term>>,
}

/// What the window hears about a terminal: output, as base64 so a cut
/// inside a character survives, or that its shell ended.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Event {
    Output { id: u64, data: String },
    Exit { id: u64, code: Option<u32> },
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(2, 1000),
        cols: cols.clamp(2, 1000),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// What a terminal is, for its tab: its id and the shell's name.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Opened {
    pub id: u64,
    pub shell: String,
}

/// The name of the shell [`shell`] starts: `zsh`, `bash`, `cmd`.
pub fn shell_name() -> String {
    let path = if cfg!(windows) {
        std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into())
    } else {
        std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into())
    };
    Path::new(&path)
        .file_stem()
        .map_or_else(|| "shell".to_owned(), |s| s.to_string_lossy().into_owned())
}

/// The person's shell, started as a login shell so it sets up what their
/// terminal app would.
fn shell() -> CommandBuilder {
    if cfg!(windows) {
        let comspec = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
        return CommandBuilder::new(comspec);
    }
    let shell = std::env::var_os("SHELL")
        .filter(|s| Path::new(s).is_absolute())
        .unwrap_or_else(|| "/bin/sh".into());
    let mut cmd = CommandBuilder::new(shell);
    cmd.arg("-l");
    cmd
}

impl Terminals {
    /// Starts a shell in `dir` for task `task`; its id. The window hears
    /// its output and its end as `terminal` events.
    pub fn open<R: Runtime>(
        &self,
        app: AppHandle<R>,
        dir: &Path,
        task: &str,
        cols: u16,
        rows: u16,
    ) -> Result<u64, String> {
        self.start(shell(), dir, task, cols, rows, move |event| {
            let _ = app.emit("terminal", event);
        })
    }

    /// Starts `cmd` in a pseudo-terminal in `dir`; `tell` hears what it
    /// writes and when it ends.
    fn start(
        &self,
        mut cmd: CommandBuilder,
        dir: &Path,
        task: &str,
        cols: u16,
        rows: u16,
        tell: impl Fn(Event) + Send + 'static,
    ) -> Result<u64, String> {
        if self.open.lock().unwrap().len() >= MAX_OPEN {
            return Err(format!(
                "at most {MAX_OPEN} terminals at once; close one first"
            ));
        }
        let pair = native_pty_system()
            .openpty(size(cols, rows))
            .map_err(|e| format!("a terminal could not open: {e}"))?;
        cmd.cwd(dir);
        cmd.env("KULE_TASK", task);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("the shell could not start: {e}"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        self.open.lock().unwrap().insert(
            id,
            Term {
                writer,
                master: pair.master,
                child,
            },
        );
        std::thread::spawn(move || {
            let b64 = base64::engine::general_purpose::STANDARD;
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => tell(Event::Output {
                        id,
                        data: b64.encode(&buf[..n]),
                    }),
                }
            }
            tell(Event::Exit { id, code: None });
        });
        Ok(id)
    }

    /// Passes what the person typed or pasted to terminal `id`.
    pub fn write(&self, id: u64, data: &str) -> Result<(), String> {
        let mut open = self.open.lock().unwrap();
        let term = open.get_mut(&id).ok_or("that terminal has ended")?;
        term.writer
            .write_all(data.as_bytes())
            .and_then(|_| term.writer.flush())
            .map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: u64, cols: u16, rows: u16) -> Result<(), String> {
        let open = self.open.lock().unwrap();
        let term = open.get(&id).ok_or("that terminal has ended")?;
        term.master
            .resize(size(cols, rows))
            .map_err(|e| e.to_string())
    }

    /// Ends terminal `id`'s shell.
    pub fn close(&self, id: u64) {
        if let Some(mut term) = self.open.lock().unwrap().remove(&id) {
            let _ = term.child.kill();
            let _ = term.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::*;

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
                Ok(Event::Exit { .. }) | Err(_) => break,
            }
        }
        seen
    }

    #[test]
    fn a_terminal_runs_in_the_copy_and_takes_what_is_typed() {
        let dir = tempfile::TempDir::new().unwrap();
        let terms = Terminals::default();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "printf 'in:%s task:%s\\n' \"$(pwd -P)\" \"$KULE_TASK\"; read line; printf 'got:%s\\n' \"$line\"; stty size"]);
        let id = terms
            .start(cmd, dir.path(), "t1", 80, 24, move |e| {
                let _ = tx.lock().unwrap().send(e);
            })
            .unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        let out = until(&rx, "task:t1");
        assert!(
            out.contains(&format!("in:{} task:t1", real.display())),
            "{out:?}"
        );

        terms.resize(id, 100, 30).unwrap();
        terms.write(id, "hello\n").unwrap();
        let out = until(&rx, "30 100");
        assert!(out.contains("got:hello"), "{out:?}");
        assert!(out.contains("30 100"), "{out:?}");

        // Its end is told, and it can no longer be written to once closed.
        terms.close(id);
        assert!(terms.write(id, "x").is_err());
    }

    #[test]
    fn a_closed_terminal_ends_its_shell() {
        let dir = tempfile::TempDir::new().unwrap();
        let terms = Terminals::default();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "printf up; sleep 60"]);
        let id = terms
            .start(cmd, dir.path(), "t2", 80, 24, move |e| {
                let _ = tx.lock().unwrap().send(e);
            })
            .unwrap();
        until(&rx, "up");
        let started = Instant::now();
        terms.close(id);
        let ended = loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(Event::Exit { .. }) => break true,
                Ok(_) => continue,
                Err(_) => break false,
            }
        };
        assert!(ended && started.elapsed() < Duration::from_secs(5));
    }
}
