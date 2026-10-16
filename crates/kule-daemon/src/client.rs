//! The clients' side of the socket: a connection per call, and one held
//! open for notices.

use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use crate::protocol::{Call, Request, Response};
use crate::server::{read_line, socket_path, write_line};
use crate::service::{Notice, Result};

/// The longest response a client reads; a task's timeline can be long.
const MAX_RESPONSE: usize = 64 << 20;

#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new(home: &Path) -> Client {
        Client {
            socket: socket_path(home),
        }
    }

    pub fn is_running(&self) -> bool {
        UnixStream::connect(&self.socket).is_ok()
    }

    /// Starts the daemon with `start` unless one answers, then waits for it.
    pub fn ensure_running(&self, start: impl FnOnce() -> io::Result<()>) -> Result<()> {
        if self.is_running() {
            return Ok(());
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
        write_line(&mut writer, &Request { id: 1, call }).map_err(|e| e.to_string())?;
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

    fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket).map_err(|e| {
            format!(
                "cannot reach the Kule daemon at {}: {e}",
                self.socket.display()
            )
        })
    }
}

/// Starts `kule daemon` for `home` in the background, detached from the
/// caller so it outlives it; its output goes to `kuled.log` in `home`.
pub fn spawn_daemon(kule: &Path, home: &Path) -> io::Result<()> {
    std::fs::create_dir_all(home)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("kuled.log"))?;
    Command::new(kule)
        .arg("daemon")
        .env("KULE_HOME", home)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        // Its own process group: signals meant for the caller's do not
        // reach it.
        .process_group(0)
        .spawn()
        .map(|_| ())
}
