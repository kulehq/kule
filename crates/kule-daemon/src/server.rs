//! The daemon's side of the socket: only the user who runs it may connect,
//! and each line is capped, so a client cannot make it read without end.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::protocol::{Call, Request, Response};
use crate::service::Service;

/// The longest request line the daemon reads.
pub const MAX_REQUEST: usize = 1 << 20;
/// Unix socket paths are limited to about this many bytes.
const MAX_SOCKET_PATH: usize = 100;

pub fn socket_path(home: &Path) -> PathBuf {
    home.join("kuled.sock")
}

/// Runs the daemon for `home` until it has been idle for `idle_after`: no
/// client connected, no agent at work, no question waiting. Refuses to
/// start when another daemon holds `home`.
pub fn run(home: &Path, idle_after: Duration) -> io::Result<()> {
    fs::create_dir_all(home)?;
    fs::set_permissions(home, fs::Permissions::from_mode(0o700))?;
    let _lock = lock(&home.join("kuled.lock"))?;
    let socket = socket_path(home);
    let listener = bind(&socket)?;
    serve(
        listener,
        Arc::new(Service::new(home.to_owned())),
        &socket,
        idle_after,
    );
    Ok(())
}

/// Serves `service` on `listener` (bound at `socket`) until it has been
/// idle for `idle_after`, then removes the socket.
pub fn serve(listener: UnixListener, service: Arc<Service>, socket: &Path, idle_after: Duration) {
    let clients = Arc::new(AtomicUsize::new(0));
    let stopping = Arc::new(AtomicBool::new(false));
    {
        let (service, clients, stopping) = (service.clone(), clients.clone(), stopping.clone());
        let socket = socket.to_owned();
        std::thread::spawn(move || {
            let mut quiet_since = Instant::now();
            loop {
                std::thread::sleep((idle_after / 10).min(Duration::from_secs(30)));
                if clients.load(Ordering::SeqCst) > 0 || service.busy() {
                    quiet_since = Instant::now();
                } else if quiet_since.elapsed() >= idle_after {
                    stopping.store(true, Ordering::SeqCst);
                    // Wakes the accept loop, which sees the flag.
                    let _ = UnixStream::connect(&socket);
                    return;
                }
            }
        });
    }
    let me = unsafe { libc::geteuid() };
    for conn in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = conn else { continue };
        if peer_uid(&stream) != Some(me) {
            continue; // dropped: closes the connection
        }
        let (service, clients) = (service.clone(), clients.clone());
        clients.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            let _ = handle(stream, &service);
            clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    let _ = fs::remove_file(socket);
}

/// Holds `path` locked for as long as the file stays open.
fn lock(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)?;
    // SAFETY: the descriptor is valid for the lifetime of `file`.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "a Kule daemon is already running for this home",
        ));
    }
    Ok(file)
}

/// Binds the socket, replacing a stale one, readable by its owner only.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    if path.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "socket path {} is longer than {MAX_SOCKET_PATH} bytes; use a shorter KULE_HOME",
                path.display()
            ),
        ));
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "a Kule daemon is already running",
            ));
        }
        fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// The uid of the process on the other end of a Unix socket.
pub fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let fd = stream.as_raw_fd();
    #[cfg(target_os = "macos")]
    {
        let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
        // SAFETY: fd is a valid socket for the lifetime of `stream`; the out
        // pointers point to live stack variables.
        let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
        (rc == 0).then_some(uid)
    }
    #[cfg(target_os = "linux")]
    {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: fd is a valid socket; cred and len are live and sized right.
        let rc = unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut cred as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        (rc == 0).then_some(cred.uid)
    }
}

fn handle(stream: UnixStream, service: &Arc<Service>) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    while let Some(line) = read_line(&mut reader, MAX_REQUEST)? {
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<Request>(&line) {
            Ok(request) => request,
            Err(e) => {
                write_line(&mut writer, &failed(0, format!("bad request: {e}")))?;
                continue;
            }
        };
        if request.call == Call::Subscribe {
            let notices = service.subscribe();
            write_line(&mut writer, &answered(request.id, Value::Null))?;
            // Ends when the client goes away and a write fails.
            for notice in notices {
                write_line(&mut writer, &notice)?;
            }
            return Ok(());
        }
        let response = match dispatch(service, request.call) {
            Ok(result) => answered(request.id, result),
            Err(e) => failed(request.id, e),
        };
        write_line(&mut writer, &response)?;
    }
    Ok(())
}

/// Carries one call to the service.
pub fn dispatch(service: &Arc<Service>, call: Call) -> Result<Value, String> {
    fn json(value: impl Serialize) -> Result<Value, String> {
        serde_json::to_value(value).map_err(|e| e.to_string())
    }
    match call {
        Call::Agents => json(service.agents()),
        Call::Tasks => json(service.tasks()?),
        Call::CreateTask { workspace } => json(service.create(&workspace)?),
        Call::Review {
            task,
            include_ignored,
        } => json(service.review(&task, include_ignored)?),
        Call::Commit {
            task,
            approve,
            include_ignored,
        } => json(service.commit(&task, approve, include_ignored)?),
        Call::Rollback { task } => json(service.rollback(&task)?),
        Call::Undo { task } => json(service.undo(&task)?),
        Call::Timeline { task } => json(service.timeline(&task)?),
        Call::RemoveTask { task } => json(service.remove(&task)?),
        Call::CopyOf { task } => json(service.copy_of(&task)?),
        Call::StartRun {
            task,
            agent,
            prompt,
        } => json(service.start_run(&task, &agent, &prompt)?),
        Call::Stream { task } => json(service.stream(&task)),
        Call::Questions => json(service.questions()),
        Call::Answer { id, approve } => json(service.answer(id, approve)?),
        Call::SetApprovalTimeout { task, minutes } => {
            json(service.set_approval_timeout(&task, minutes)?)
        }
        Call::Subscribe => Err("subscribe on a connection of its own".to_owned()),
    }
}

fn answered(id: u64, result: Value) -> Response {
    Response {
        id,
        result: Some(result),
        error: None,
    }
}

fn failed(id: u64, error: String) -> Response {
    Response {
        id,
        result: None,
        error: Some(error),
    }
}

/// Reads one line of at most `max` bytes; `None` at the end of the stream.
pub(crate) fn read_line(reader: &mut impl BufRead, max: usize) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    let n = reader.take(max as u64 + 1).read_until(b'\n', &mut line)?;
    if n == 0 {
        return Ok(None);
    }
    if line.len() > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub(crate) fn write_line(writer: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    writer.write_all(&line)?;
    writer.flush()
}
