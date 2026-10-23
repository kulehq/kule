//! `kule shell`'s terminal: the shell runs in the task's sandbox on a
//! pseudo-terminal of its own, and Kule passes the person's keys to it and
//! its output back, as `script` does. The person's own terminal stays out
//! of the sandbox's reach: nothing in there can type into it or read it.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use kule_daemon::shell::Sandboxed;
use kule_engine::Denial;

/// The size of the person's terminal, or 80 × 24 where there is none.
fn their_size() -> libc::winsize {
    // SAFETY: TIOCGWINSZ fills the zeroed struct.
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(0, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            ws
        } else {
            libc::winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }
        }
    }
}

/// The person's terminal in raw mode until dropped: every key goes to the
/// shell as typed, Ctrl-C included, and the shell's terminal echoes.
struct Raw(Option<libc::termios>);

impl Raw {
    fn on() -> Raw {
        // SAFETY: tcgetattr/tcsetattr on stdin with a zeroed struct they fill.
        unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            if libc::isatty(0) != 1 || libc::tcgetattr(0, &mut term) != 0 {
                return Raw(None);
            }
            let saved = term;
            libc::cfmakeraw(&mut term);
            if libc::tcsetattr(0, libc::TCSANOW, &term) != 0 {
                return Raw(None);
            }
            Raw(Some(saved))
        }
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        if let Some(saved) = &self.0 {
            // SAFETY: restores what tcgetattr returned.
            unsafe {
                libc::tcsetattr(0, libc::TCSAFLUSH, saved);
            }
        }
    }
}

/// Runs `shell` in task `task`'s sandbox on a terminal of its own, passing
/// the person's terminal through, until it ends: its exit status, and what
/// the sandbox refused it. Refused while an agent is at work in the task.
pub fn run(home: &Path, task: &str, shell: &str) -> Result<(Option<i32>, Vec<Denial>)> {
    let sandboxed = Sandboxed::new(home, task).map_err(|e| anyhow!(e))?;
    for note in sandboxed.notes() {
        eprintln!("kule: {note}");
    }
    let mut ws = their_size();
    let (mut leader, mut follower) = (0, 0);
    // SAFETY: openpty fills the two descriptors; no name is asked for, the
    // modes are the system's, the size is read from `ws`.
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
        bail!(
            "a terminal could not open: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: openpty made both, and nothing else owns them.
    let (leader, follower) =
        unsafe { (OwnedFd::from_raw_fd(leader), OwnedFd::from_raw_fd(follower)) };
    for fd in [&leader, &follower] {
        // SAFETY: fcntl on a descriptor this function owns.
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) };
    }
    let mut cmd = sandboxed
        .command(shell, &[], follower.as_fd())
        .map_err(|e| anyhow!(e))?;
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("could not start {shell}: {e}"))?;
    drop(cmd);
    drop(follower);
    let raw = Raw::on();
    let leader = File::from(leader);

    let mut to_shell = leader.try_clone()?;
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || to_shell.write_all(&buf[..n]).is_err() {
                break;
            }
        }
    });
    // The person's terminal resized: so is the shell's.
    let sized = leader.try_clone()?;
    std::thread::spawn(move || {
        let mut last = their_size();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let now = their_size();
            if (now.ws_col, now.ws_row) != (last.ws_col, last.ws_row) {
                // SAFETY: TIOCSWINSZ reads the size from `now`.
                unsafe { libc::ioctl(sized.as_raw_fd(), libc::TIOCSWINSZ, &now) };
                last = now;
            }
        }
    });
    let (read_out, all_read) = mpsc::channel::<()>();
    let mut from_shell = leader;
    std::thread::spawn(move || {
        let mut stdout = std::io::stdout();
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = from_shell.read(&mut buf) {
            if n == 0
                || stdout
                    .write_all(&buf[..n])
                    .and_then(|()| stdout.flush())
                    .is_err()
            {
                break;
            }
        }
        let _ = read_out.send(());
    });
    let status = child.wait()?;
    // What it wrote last; what it left in the background may hold its
    // terminal on, and a commit ends that.
    let _ = all_read.recv_timeout(Duration::from_millis(300));
    drop(raw);
    let denials = sandboxed.end().map_err(|e| anyhow!(e))?;
    Ok((status.code(), denials))
}
