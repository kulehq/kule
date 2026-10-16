//! The daemon over a real Unix socket: calls, notices, one daemon per home,
//! the idle exit and the line cap.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kule_daemon::{
    Call, Client, MAX_REQUEST, Notice, Service, TaskView, bind, run, serve, socket_path,
};
use kule_engine::{Policy, Proxy, Task};
use tempfile::TempDir;

/// Serves a fresh home on a thread; returns it with the service behind it.
fn daemon(home: &Path) -> Arc<Service> {
    let socket = socket_path(home);
    let listener = bind(&socket).unwrap();
    let service = Arc::new(Service::new(home.to_owned()));
    let served = service.clone();
    std::thread::spawn(move || serve(listener, served, &socket, Duration::from_secs(600)));
    service
}

fn workspace() -> TempDir {
    let ws = TempDir::new().unwrap();
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    ws
}

#[test]
fn calls_reach_the_service_and_errors_come_back() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let client = Client::new(home.path());
    assert!(client.is_running());

    let ws = workspace();
    let task: TaskView = client
        .call(Call::CreateTask {
            workspace: ws.path().to_owned(),
        })
        .unwrap();
    let tasks: Vec<TaskView> = client.call(Call::Tasks).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, task.id);

    let copy: std::path::PathBuf = client
        .call(Call::CopyOf {
            task: task.id.clone(),
        })
        .unwrap();
    fs::write(copy.join("b.txt"), "b\n").unwrap();
    let rolled: usize = client
        .call(Call::Rollback {
            task: task.id.clone(),
        })
        .unwrap();
    assert_eq!(rolled, 1);

    let error = client
        .call::<usize>(Call::Rollback {
            task: "nope".into(),
        })
        .unwrap_err();
    assert!(error.contains("no task nope"), "{error}");
}

#[test]
fn subscribers_hear_questions_come_and_go() {
    let home = TempDir::new().unwrap();
    let service = daemon(home.path());
    let client = Client::new(home.path());
    let ws = workspace();
    let task: TaskView = client
        .call(Call::CreateTask {
            workspace: ws.path().to_owned(),
        })
        .unwrap();

    let (tx, rx) = mpsc::channel();
    let listener = client.clone();
    std::thread::spawn(move || listener.subscribe(move |n| tx.send(n).unwrap()));
    // The subscription is live once the daemon counts it; ask after that.
    std::thread::sleep(Duration::from_millis(200));

    let task = Task::open(home.path(), &task.id).unwrap();
    let proxy = Proxy::start_asking(
        &task,
        Policy::builtin(),
        service.approvals(),
        Duration::from_secs(60),
    )
    .unwrap();
    let mut asking = ProxyClient::connect(proxy.port());
    asking.ask("example.invalid:443");

    let asked = match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        Notice::Questions { waiting } => waiting,
        other => panic!("expected questions, heard {other:?}"),
    };
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].host, "example.invalid");

    client
        .call::<()>(Call::Answer {
            id: asked[0].id,
            approve: false,
        })
        .unwrap();
    match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
        Notice::Questions { waiting } => assert!(waiting.is_empty()),
        other => panic!("expected questions, heard {other:?}"),
    }
    drop(proxy);
}

/// A raw client of the task's proxy, asking for a tunnel.
struct ProxyClient(std::net::TcpStream);

impl ProxyClient {
    fn connect(port: u16) -> Self {
        ProxyClient(std::net::TcpStream::connect(("127.0.0.1", port)).unwrap())
    }

    fn ask(&mut self, target: &str) {
        write!(self.0, "CONNECT {target} HTTP/1.1\r\n\r\n").unwrap();
    }
}

#[test]
fn one_daemon_per_home() {
    let home = TempDir::new().unwrap();
    let path = home.path().to_owned();
    std::thread::spawn(move || run(&path, Duration::from_secs(600)));
    let client = Client::new(home.path());
    client.ensure_running(|| Ok(())).unwrap();

    let second = run(home.path(), Duration::from_secs(600)).unwrap_err();
    assert_eq!(second.kind(), std::io::ErrorKind::AddrInUse);
    assert!(client.is_running());
}

#[test]
fn an_idle_daemon_goes_away() {
    let home = TempDir::new().unwrap();
    let path = home.path().to_owned();
    let started = Instant::now();
    let daemon = std::thread::spawn(move || run(&path, Duration::from_millis(300)));
    let client = Client::new(home.path());
    client.ensure_running(|| Ok(())).unwrap();

    daemon.join().unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!client.is_running());
    assert!(!socket_path(home.path()).exists());
}

#[test]
fn an_overlong_request_is_cut_off() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let mut stream = UnixStream::connect(socket_path(home.path())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    // A whole line, ended: without the cap the daemon would read it and
    // answer.
    let mut junk = vec![b'x'; MAX_REQUEST + 10];
    junk.push(b'\n');
    let _ = stream.write_all(&junk);
    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    assert!(reply.is_empty(), "{}", String::from_utf8_lossy(&reply));
    // The daemon is still there for everyone else.
    let tasks: Vec<TaskView> = Client::new(home.path()).call(Call::Tasks).unwrap();
    assert!(tasks.is_empty());
}

#[test]
fn the_socket_is_for_its_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let mode = fs::metadata(socket_path(home.path()))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
}
