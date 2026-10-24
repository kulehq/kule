//! The clients' side of the socket: a connection per call, and one held
//! open for notices.

use std::io::{self, BufReader};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use crate::protocol::{Call, Hello, PROTOCOL, Request, Response};
use crate::server::{read_line, socket_path, token_path, write_line};
use crate::service::{Notice, Result};

/// The longest response a client reads; a task's timeline can be long.
const MAX_RESPONSE: usize = 64 << 20;

#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
    token: PathBuf,
}

impl Client {
    pub fn new(home: &Path) -> Client {
        Client {
            socket: socket_path(home),
            token: token_path(home),
        }
    }

    pub fn is_running(&self) -> bool {
        UnixStream::connect(&self.socket).is_ok()
    }

    /// Starts the daemon with `start` unless one that speaks this protocol
    /// answers, then waits for it. A daemon of another version is asked to
    /// stop first; it refuses while it has agents at work.
    pub fn ensure_running(&self, start: impl FnOnce() -> io::Result<()>) -> Result<()> {
        if self.is_running() {
            if matches!(self.call::<Hello>(Call::Hello), Ok(h) if h.protocol == PROTOCOL) {
                return Ok(());
            }
            self.call::<()>(Call::Shutdown).map_err(|e| {
                format!(
                    "a Kule daemon of another version is running and did not stop ({e}); \
                     stop it when its agents are done"
                )
            })?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.is_running() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        start().map_err(|e| format!("could not start the Kule daemon: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err("the Kule daemon did not come up; see kuled.log in Kule's home".to_owned())
    }

    /// Makes one call and returns its result.
    pub fn call<T: DeserializeOwned>(&self, call: Call) -> Result<T> {
        let stream = self.connect()?;
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        write_line(
            &mut writer,
            &Request {
                id: 1,
                token: self.token(),
                call,
            },
        )
        .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        let line = read_line(&mut reader, MAX_RESPONSE)
            .map_err(|e| e.to_string())?
            .ok_or("the Kule daemon closed the connection")?;
        let response: Response = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if let Some(error) = response.error {
            return Err(error);
        }
        serde_json::from_value(response.result.unwrap_or_default()).map_err(|e| e.to_string())
    }

    /// Calls `heard` with each notice until the daemon goes away.
    pub fn subscribe(&self, mut heard: impl FnMut(Notice)) -> Result<()> {
        let stream = self.connect()?;
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        write_line(
            &mut writer,
            &Request {
                id: 1,
                token: self.token(),
                call: Call::Subscribe,
            },
        )
        .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream);
        // The first line answers the subscribe call.
        read_line(&mut reader, MAX_RESPONSE).map_err(|e| e.to_string())?;
        while let Some(line) = read_line(&mut reader, MAX_RESPONSE).map_err(|e| e.to_string())? {
            if let Ok(notice) = serde_json::from_str(&line) {
                heard(notice);
            }
        }
        Ok(())
    }

    /// The running daemon's token, read afresh: a new daemon has a new one.
    fn token(&self) -> Option<String> {
        std::fs::read_to_string(&self.token)
            .ok()
            .map(|t| t.trim().to_owned())
    }

    /// A connection to the daemon, once its other end is shown to be this
    /// user's: the token goes to nothing else listening there.
    fn connect(&self) -> Result<UnixStream> {
        let stream = UnixStream::connect(&self.socket).map_err(|e| {
            format!(
                "cannot reach the Kule daemon at {}: {e}",
                self.socket.display()
            )
        })?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        if crate::peer_uid(&stream) != Some(me) {
            return Err(format!(
                "what listens at {} is not your own Kule daemon; Kule sends it nothing",
                self.socket.display()
            ));
        }
        Ok(stream)
    }
}

/// Starts `kule daemon` for `home` in the background, detached from the
/// caller so it outlives it; its output goes to `kuled.log` in `home`.
pub fn spawn_daemon(kule: &Path, home: &Path) -> io::Result<()> {
    std::fs::create_dir_all(home)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(home.join("kuled.log"))?;
    let mut command = Command::new(kule);
    command
        .arg("daemon")
        .env("KULE_HOME", home)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // A session of its own: no controlling terminal to be reached through,
    // and no signal meant for the caller's.
    // SAFETY: setsid is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().map(|_| ())
}
