//! `kule shell`'s terminal: the shell runs in the task's sandbox on a
//! pseudo-terminal of its own, and Kule passes the person's keys to it and
//! its output back, as `script` does. The person's own terminal stays out
//! of the sandbox's reach: nothing in there can type into it or read it.
//! What the shell writes reaches it through [`crate::term::Filter`], as
//! what `kule exec` runs on a terminal does (see [`filtered`]).

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use kule_daemon::shell::{Sandboxed, ShellEnd};

/// The size of the person's terminal (on the first of the standard
/// streams that is one), or 80 × 24 where there is none.
fn their_size() -> libc::winsize {
    for fd in [0, 1, 2] {
        // SAFETY: TIOCGWINSZ fills the zeroed struct.
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
                return ws;
            }
        }
    }
    libc::winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

/// A pseudo-terminal of `ws`'s size: its leader and follower ends, neither
/// passed on to what Kule starts but where it is given.
fn pty(mut ws: libc::winsize) -> Result<(OwnedFd, OwnedFd)> {
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
    Ok((leader, follower))
}

/// Copies what the terminal's `leader` end gives to `out` through the
/// filter, until it ends; tells `done` once it has.
fn pass_out(mut leader: File, mut out: impl Write + Send + 'static, done: mpsc::Sender<()>) {
    std::thread::spawn(move || {
        let mut filter = crate::term::Filter::new();
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = leader.read(&mut buf) {
            if n == 0
                || out
                    .write_all(&filter.feed(&buf[..n]))
                    .and_then(|()| out.flush())
                    .is_err()
            {
                break;
            }
        }
        let _ = done.send(());
    });
}

/// Keeps the size of the terminal at `leader` the person's.
fn follow_size(sized: File) {
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
}

/// Runs `start` (`kule exec`'s command) with a terminal of its own in
/// place of each of the person's standard streams that is a terminal, and
/// its output passed to theirs through [`crate::term::Filter`]: the
/// command still writes to a terminal (colours, sizes), but no sequence
/// that sets the clipboard or types into the person's terminal reaches it.
/// The person's terminal stays as it is (Ctrl-C stops Kule and the command
/// as before, a line is edited there); the command's terminal echoes
/// nothing and leaves output as written, so what is typed shows once, and
/// the end of the person's input (Ctrl-D) is the end of the command's.
/// `None` where no standard stream is a terminal: nothing to pass through.
pub fn filtered(start: &mut kule_engine::Start) -> Result<Option<std::process::ExitStatus>> {
    // SAFETY: isatty on the standard streams.
    let ttys: Vec<i32> = [0, 1, 2]
        .into_iter()
        .filter(|fd| unsafe { libc::isatty(*fd) } == 1)
        .collect();
    if ttys.is_empty() {
        return Ok(None);
    }
    let kule_engine::Start::Command(command) = start;
    let (leader, follower) = pty(their_size())?;
    // SAFETY: tcgetattr/tcsetattr on the follower Kule just made.
    let eof = unsafe {
        let mut term: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(follower.as_raw_fd(), &mut term) != 0 {
            bail!(
                "a terminal could not be set: {}",
                std::io::Error::last_os_error()
            );
        }
        term.c_lflag &= !(libc::ECHO | libc::ECHOE | libc::ECHOK | libc::ECHONL);
        term.c_oflag &= !libc::OPOST;
        if libc::tcsetattr(follower.as_raw_fd(), libc::TCSANOW, &term) != 0 {
            bail!(
                "a terminal could not be set: {}",
                std::io::Error::last_os_error()
            );
        }
        term.c_cc[libc::VEOF]
    };
    for fd in &ttys {
        let io = std::process::Stdio::from(follower.try_clone()?);
        match fd {
            0 => command.stdin(io),
            1 => command.stdout(io),
            _ => command.stderr(io),
        };
    }
    let mut process = start.spawn()?;
    // The command's copies of the follower go with it: once it is gone,
    // the leader reads to its end.
    let kule_engine::Start::Command(command) = start;
    for fd in &ttys {
        let none = std::process::Stdio::null();
        match fd {
            0 => command.stdin(none),
            1 => command.stdout(none),
            _ => command.stderr(none),
        };
    }
    drop(follower);
    let leader = File::from(leader);
    if ttys.contains(&0) {
        let mut to_it = leader.try_clone()?;
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = [0u8; 4096];
            while let Ok(n) = stdin.read(&mut buf) {
                if n == 0 {
                    // The end of the person's input, as the command's
                    // terminal reads it.
                    let _ = to_it.write_all(&[eof]);
                    break;
                }
                if to_it.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        });
    }
    follow_size(leader.try_clone()?);
    // To the person's terminal: their output's, or errors', or input's.
    let to = [1, 2, 0]
        .into_iter()
        .find(|fd| ttys.contains(fd))
        .expect("a terminal among them");
    // SAFETY: dup of a standard stream, owned by the new descriptor.
    let out = unsafe { File::from(OwnedFd::from_raw_fd(libc::dup(to))) };
    let (done, all_read) = mpsc::channel::<()>();
    pass_out(leader, out, done);
    let status = process.wait()?;
    // What it wrote last; what it left in the background may hold its
    // terminal on, and is ended as the command's strays are.
    let _ = all_read.recv_timeout(Duration::from_millis(300));
    Ok(Some(status))
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
/// its end tells (what the sandbox refused it, what Kule ended). Refused while an agent is at work in the task.
pub fn run(home: &Path, task: &str, shell: &str) -> Result<(Option<i32>, ShellEnd)> {
    let sandboxed = Sandboxed::new(home, task).map_err(|e| anyhow!(e))?;
    for note in sandboxed.notes() {
        eprintln!("kule: {note}");
    }
    let (leader, follower) = pty(their_size())?;
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
    follow_size(leader.try_clone()?);
    let (read_out, all_read) = mpsc::channel::<()>();
    pass_out(leader, std::io::stdout(), read_out);
    let status = child.wait()?;
    // What it wrote last; what it left in the background may hold its
    // terminal on, and a commit ends that.
    let _ = all_read.recv_timeout(Duration::from_millis(300));
    drop(raw);
    Ok((status.code(), sandboxed.end()))
}
