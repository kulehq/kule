//! `kule shell`'s terminal on Windows: the shell runs in the task's
//! container on a pseudo console that Kule's helper opens inside it, and
//! Kule passes the person's keys in and the console's output back. The
//! person's own console stays out of the container's reach: nothing in
//! there can type into it or read it. What the console writes reaches the
//! person's through [`crate::term::Filter`], as what `kule exec` runs on a
//! console does (see [`filtered`]).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use anyhow::{Result, anyhow};
use kule_daemon::shell::{Sandboxed, ShellEnd};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    CONSOLE_MODE, CONSOLE_SCREEN_BUFFER_INFO, DISABLE_NEWLINE_AUTO_RETURN, ENABLE_ECHO_INPUT,
    ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT,
    ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetConsoleScreenBufferInfo, GetStdHandle,
    STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleMode,
};

fn std_handle(which: STD_HANDLE) -> Option<HANDLE> {
    // SAFETY: only reads this process's standard handle.
    unsafe { GetStdHandle(which) }
        .ok()
        .filter(|h| !h.is_invalid())
}

/// The size of the person's console window, or 80 × 24 where there is
/// none.
fn their_size() -> (u16, u16) {
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    let read = std_handle(STD_OUTPUT_HANDLE)
        // SAFETY: fills `info` for a console's output handle.
        .is_some_and(|h| unsafe { GetConsoleScreenBufferInfo(h, &mut info) }.is_ok());
    let w = info.srWindow;
    let (cols, rows) = (
        i32::from(w.Right) - i32::from(w.Left) + 1,
        i32::from(w.Bottom) - i32::from(w.Top) + 1,
    );
    if read && cols > 0 && rows > 0 {
        (cols.min(1000) as u16, rows.min(1000) as u16)
    } else {
        (80, 24)
    }
}

/// The person's console in raw mode until dropped: every key goes to the
/// shell as typed, Ctrl-C included, as the escape sequences a terminal
/// sends; what the shell writes back is drawn as a terminal draws it.
struct Raw(Vec<(HANDLE, CONSOLE_MODE)>);

impl Raw {
    fn on() -> Raw {
        let mut saved = Vec::new();
        let mut set = |which, change: fn(CONSOLE_MODE) -> CONSOLE_MODE| {
            let Some(h) = std_handle(which) else { return };
            let mut mode = CONSOLE_MODE::default();
            // SAFETY: reads and sets the mode of this process's console.
            unsafe {
                if GetConsoleMode(h, &mut mode).is_ok() && SetConsoleMode(h, change(mode)).is_ok() {
                    saved.push((h, mode));
                }
            }
        };
        set(STD_INPUT_HANDLE, |m| {
            (m & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT
        });
        set(STD_OUTPUT_HANDLE, |m| {
            m | ENABLE_VIRTUAL_TERMINAL_PROCESSING | DISABLE_NEWLINE_AUTO_RETURN
        });
        Raw(saved)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        for (h, mode) in &self.0 {
            // SAFETY: puts back what GetConsoleMode returned.
            let _ = unsafe { SetConsoleMode(*h, *mode) };
        }
    }
}

/// Runs `shell` in task `task`'s sandbox on a console of its own, passing
/// the person's console through, until it ends: its exit status, and what
/// its end tells. Refused while an agent is at work in the task.
pub fn run(home: &Path, task: &str, shell: &str) -> Result<(Option<i32>, ShellEnd)> {
    let sandboxed = Sandboxed::new(home, task).map_err(|e| anyhow!(e))?;
    for note in sandboxed.notes() {
        eprintln!("kule: {note}");
    }
    let (cols, rows) = their_size();
    let mut start = sandboxed
        .terminal(shell, &[], cols, rows)
        .map_err(|e| anyhow!(e))?;
    start.env("TERM", "xterm-256color");
    start.env("COLORTERM", "truecolor");
    let kule_engine::Terminal {
        input,
        output,
        errors,
        mut process,
    } = start
        .spawn()
        .map_err(|e| anyhow!("could not start {shell}: {e}"))?;
    let raw = Raw::on();
    let input = Arc::new(Mutex::new(input));

    let typed = Arc::clone(&input);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdin.read(&mut buf) {
            let mut to_shell = typed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if n == 0 || to_shell.write(&buf[..n]).is_err() {
                break;
            }
        }
    });
    // The person's console resized: so is the shell's.
    let sized = Arc::clone(&input);
    std::thread::spawn(move || {
        let mut last = their_size();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let now = their_size();
            if now != last {
                let mut to_shell = sized
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if to_shell.resize(now.0, now.1).is_err() {
                    break;
                }
                last = now;
            }
        }
    });
    let (read_out, all_read) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut from_shell = output;
        let mut stdout = std::io::stdout();
        let mut filter = crate::term::Filter::new();
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = from_shell.read(&mut buf) {
            if n == 0
                || stdout
                    .write_all(&filter.feed(&buf[..n]))
                    .and_then(|()| stdout.flush())
                    .is_err()
            {
                break;
            }
        }
        let _ = read_out.send(());
    });
    std::thread::spawn(move || {
        let _ = filter_into(errors, std::io::stderr());
    });
    let status = process.wait()?;
    let _ = all_read.recv_timeout(Duration::from_millis(300));
    drop(raw);
    Ok((status.code(), sandboxed.end()))
}

/// Copies `from` to `to` through [`crate::term::Filter`], until it ends.
fn filter_into(mut from: impl Read, mut to: impl Write) -> std::io::Result<()> {
    let mut filter = crate::term::Filter::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&filter.feed(&buf[..n]))?;
        to.flush()?;
    }
}

/// Runs `start` (`kule exec`'s command) with its output and errors read by
/// Kule where they are the person's console, and passed on to it through
/// [`crate::term::Filter`]: no sequence that sets the clipboard or types
/// into the console reaches it. The command then writes to a pipe, not a
/// console. `None` where neither is a console: nothing to pass through.
pub fn filtered(start: &mut kule_engine::Start) -> Result<Option<std::process::ExitStatus>> {
    use std::io::IsTerminal;
    let (out, err) = (
        std::io::stdout().is_terminal(),
        std::io::stderr().is_terminal(),
    );
    if !out && !err {
        return Ok(None);
    }
    if out {
        start.stdout(kule_engine::Io::Piped);
    }
    if err {
        start.stderr(kule_engine::Io::Piped);
    }
    let mut process = start.spawn()?;
    let (done, all_read) = mpsc::channel::<()>();
    let mut passing = 0;
    if let Some(s) = process.stdout.take() {
        let done = done.clone();
        passing += 1;
        std::thread::spawn(move || {
            let _ = filter_into(s, std::io::stdout());
            let _ = done.send(());
        });
    }
    if let Some(s) = process.stderr.take() {
        let done = done.clone();
        passing += 1;
        std::thread::spawn(move || {
            let _ = filter_into(s, std::io::stderr());
            let _ = done.send(());
        });
    }
    let status = process.wait()?;
    // What it wrote last; what it left running may hold the pipe on.
    for _ in 0..passing {
        if all_read.recv_timeout(Duration::from_millis(300)).is_err() {
            break;
        }
    }
    Ok(Some(status))
}
