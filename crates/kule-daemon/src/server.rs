//! The daemon's side of the socket: only the user who runs it may connect,
//! and each line is capped, so a client cannot make it read without end.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::protocol::{Call, Hello, PROTOCOL, Request, Response};
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
    let token = new_token(home)?;
    let socket = socket_path(home);
    let listener = bind(&socket)?;
    // What an earlier removal left behind, out of the way of requests.
    let trash = home.to_owned();
    std::thread::spawn(move || {
        let _ = kule_engine::Task::empty_trash(&trash);
    });
    serve(
        listener,
        Arc::new(Service::new(home.to_owned())),
        &socket,
        idle_after,
        token,
    );
    Ok(())
}

/// Where the daemon keeps the token every request must carry.
pub fn token_path(home: &Path) -> PathBuf {
    home.join("kuled.token")
}

/// Makes a fresh token and writes it, readable by its owner only, to
/// [`token_path`]. The sandbox hides Kule's home, so a task can reach the
/// socket (Linux has no rule against that) but not speak to the daemon.
pub fn new_token(home: &Path) -> io::Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let path = token_path(home);
    let _ = fs::remove_file(&path);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(token.as_bytes())?;
    Ok(token)
}

/// Serves `service` on `listener` (bound at `socket`) until it has been
/// idle for `idle_after`, then removes the socket. Requests without
/// `token` are refused.
pub fn serve(
    listener: UnixListener,
    service: Arc<Service>,
    socket: &Path,
    idle_after: Duration,
    token: String,
) {
    let clients = Arc::new(AtomicUsize::new(0));
    let control = Arc::new(Control {
        stopping: AtomicBool::new(false),
        socket: socket.to_owned(),
        token,
    });
    {
        let (service, clients, control) = (service.clone(), clients.clone(), control.clone());
        std::thread::spawn(move || {
            let mut quiet_since = Instant::now();
            loop {
                std::thread::sleep((idle_after / 10).min(Duration::from_secs(30)));
                if clients.load(Ordering::SeqCst) > 0 || service.busy() {
                    quiet_since = Instant::now();
                } else if quiet_since.elapsed() >= idle_after {
                    control.stop();
                    return;
                }
            }
        });
    }
    // SAFETY: geteuid has no preconditions and cannot fail.
    let me = unsafe { libc::geteuid() };
    for conn in listener.incoming() {
        if control.stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = conn else { continue };
        if peer_uid(&stream) != Some(me) {
            continue; // dropped: closes the connection
        }
        let (service, clients, control) = (service.clone(), clients.clone(), control.clone());
        clients.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            let _ = handle(stream, &service, &control);
            clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    let _ = fs::remove_file(socket);
}

/// Lets the idle watch and a shutdown call stop the accept loop.
struct Control {
    stopping: AtomicBool,
    socket: PathBuf,
    token: String,
}

/// Compares in time that does not depend on where they differ.
fn same_token(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

impl Control {
    fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Wakes the accept loop, which sees the flag.
        let _ = UnixStream::connect(&self.socket);
    }
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

fn handle(stream: UnixStream, service: &Arc<Service>, control: &Control) -> io::Result<()> {
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
        if !request
            .token
            .as_deref()
            .is_some_and(|given| same_token(given, &control.token))
        {
            let refused = "not authorized: requests carry the token in Kule's home";
            write_line(&mut writer, &failed(request.id, refused.to_owned()))?;
            return Ok(());
        }
        if request.call == Call::Subscribe {
            let notices = service.subscribe();
            write_line(&mut writer, &answered(request.id, Value::Null))?;
            // Ends when the client goes away and a write fails.
            for notice in notices {
                write_line(&mut writer, &notice)?;
            }
            return Ok(());
        }
        if request.call == Call::Shutdown {
            if service.busy() {
                let busy = "an agent is at work or a question is waiting; not stopping";
                write_line(&mut writer, &failed(request.id, busy.to_owned()))?;
                continue;
            }
            write_line(&mut writer, &answered(request.id, Value::Null))?;
            control.stop();
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
        Call::Agents => json(service.agents()?),
        Call::AgentList => json(service.agent_list()?),
        Call::AgentRegistry => json(service.agent_registry()?),
        Call::SaveAgent { profile } => json(service.save_agent(&profile)?),
        Call::RemoveAgent { name } => json(service.remove_agent(&name)?),
        Call::ApproveAgent { name, digest } => json(service.approve_agent(&name, &digest)?),
        Call::RevokeAgent { name } => json(service.revoke_agent(&name)?),
        Call::TryAgent { profile } => json(service.try_agent(&profile)?),
        Call::Tasks => json(service.tasks()?),
        Call::CreateTask { workspace } => json(service.create(&workspace)?),
        Call::Review {
            task,
            include_ignored,
        } => json(service.review(&task, include_ignored)?),
        Call::Preview { task, path } => json(service.preview(&task, &path)?),
        Call::Commit {
            task,
            approve,
            include_ignored,
            reviewed,
            remember,
        } => json(service.commit_remembering(
            &task,
            approve,
            include_ignored,
            reviewed.as_deref(),
            &remember,
        )?),
        Call::Rollback { task } => json(service.rollback(&task)?),
        Call::Undo { task } => json(service.undo(&task)?),
        Call::Timeline { task } => json(service.timeline(&task)?),
        Call::RemoveTask { task } => json(service.remove(&task)?),
        Call::CopyOf { task } => json(service.copy_of(&task)?),
        Call::TerminalDir { task } => json(service.terminal_dir(&task)?),
        Call::StartRun {
            task,
            agent,
            prompt,
        } => json(service.start_run(&task, &agent, &prompt)?),
        Call::Stream { task } => json(service.stream(&task)),
        Call::Questions => json(service.questions()),
        Call::Answer {
            id,
            approve,
            remember,
        } => json(service.answer_remembering(id, approve, remember)?),
        Call::Rules => json(service.rules()?),
        Call::ForgetRule { id } => json(service.forget_rule(id)?),
        Call::RuleImpact {
            allowance,
            workspace,
        } => json(service.rule_impact(&allowance, workspace.as_deref())?),
        Call::SetApprovalTimeout { task, minutes } => {
            json(service.set_approval_timeout(&task, minutes)?)
        }
        Call::SetBudget { task, budget } => json(service.set_budget(&task, budget)?),
        Call::ApplyHome { task, paths } => json(service.apply_home(&task, &paths)?),
        Call::DiscardHome { task, paths } => json(service.discard_home(&task, &paths)?),
        Call::Secrets => json(service.secrets()?),
        Call::AddSecret {
            name,
            value,
            hosts,
            workspace,
        } => json(service.add_secret(&name, &value, &hosts, workspace.as_deref())?),
        Call::RemoveSecret { name, workspace } => {
            json(service.remove_secret(&name, workspace.as_deref())?)
        }
        Call::Hello => json(Hello {
            protocol: PROTOCOL,
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }),
        Call::Subscribe | Call::Shutdown => Err("not a call for dispatch".to_owned()),
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
