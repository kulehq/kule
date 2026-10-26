//! The daemon over a real named pipe on Windows: one daemon per home, calls
//! reach it, a request without the token is refused, and Kule's home is
//! closed to others.
#![cfg(windows)]

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kule_daemon::{
    Call, Client, Hello, PROTOCOL, Service, TaskView, bind, check_home, new_token, serve,
    socket_path, token_path,
};
use tempfile::TempDir;

/// Serves a fresh home on a thread; returns it with the service behind it.
fn daemon(home: &Path) -> Arc<Service> {
    let socket = socket_path(home);
    let listener = bind(&socket).unwrap();
    let service = Arc::new(Service::new(home.to_owned()));
    let served = service.clone();
    let token = new_token(home).unwrap();
    std::thread::spawn(move || serve(listener, served, &socket, Duration::from_secs(600), token));
    service
}

#[test]
fn calls_reach_the_service_over_the_pipe() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let client = Client::new(home.path());
    assert!(client.is_running());
    let hello: Hello = client.call(Call::Hello).unwrap();
    assert_eq!(hello.protocol, PROTOCOL);

    let ws = TempDir::new().unwrap();
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let task: TaskView = client
        .call(Call::CreateTask {
            workspace: ws.path().to_owned(),
        })
        .unwrap();
    let tasks: Vec<TaskView> = client.call(Call::Tasks).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, task.id);
}

/// The pipe's name belongs to the first daemon: a second one for the same
/// home is refused, and a different home gets a different name.
#[test]
fn one_daemon_per_home() {
    let home = TempDir::new().unwrap();
    let socket = socket_path(home.path());
    assert!(socket.to_string_lossy().starts_with(r"\\.\pipe\kule-"));
    let _first = bind(&socket).unwrap();
    let e = match bind(&socket) {
        Ok(_) => panic!("a second daemon took the same pipe"),
        Err(e) => e,
    };
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse, "{e}");
    let other = TempDir::new().unwrap();
    assert_ne!(socket, socket_path(other.path()));
}

/// A request without the token in Kule's home is refused.
#[test]
fn a_request_without_the_token_is_refused() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    fs::write(token_path(home.path()), "not-the-token").unwrap();
    let e = Client::new(home.path())
        .call::<Hello>(Call::Hello)
        .unwrap_err();
    assert!(e.contains("not authorized"), "{e}");
}

/// Kule's home is a folder of the person's own, closed to others: one that
/// lets another account in is closed, and the note says so.
#[test]
fn kule_s_home_is_closed_to_others() {
    let base = TempDir::new().unwrap();
    let home = base.path().join("made").join("Kule");
    assert_eq!(check_home(&home).unwrap(), None);
    assert_eq!(check_home(&home).unwrap(), None);

    // Let every user in, as a careless setup might.
    let granted = std::process::Command::new("icacls")
        .arg(&home)
        .args(["/grant", "*S-1-5-32-545:(OI)(CI)(R)"])
        .output()
        .unwrap();
    assert!(
        granted.status.success(),
        "{}",
        String::from_utf8_lossy(&granted.stderr)
    );
    let note = check_home(&home).unwrap().unwrap();
    assert!(note.contains("open to others"), "{note}");
    assert_eq!(check_home(&home).unwrap(), None);

    let file = base.path().join("file");
    fs::write(&file, "").unwrap();
    assert!(check_home(&file).unwrap_err().contains("not a folder"));
}
