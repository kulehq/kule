//! Servers working in a folder are found by their working directory and
//! their listening port; others are not.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use kule_engine::listening_in;
use tempfile::TempDir;

/// A server listening on a free loopback port, working in `dir`; returns
/// it with its port.
fn server_in(dir: &Path) -> (Child, u16) {
    let mut child = Command::new("python3")
        .args([
            "-c",
            "import socket,time\ns=socket.socket()\ns.bind(('127.0.0.1',0))\ns.listen()\nprint(s.getsockname()[1],flush=True)\ntime.sleep(30)",
        ])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    (child, line.trim().parse().unwrap())
}

#[test]
fn a_server_working_in_the_folder_is_found_and_one_elsewhere_is_not() {
    let (copy, elsewhere) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    std::fs::create_dir(copy.path().join("web")).unwrap();
    let (mut inside, port) = server_in(&copy.path().join("web"));
    let (mut outside, other) = server_in(elsewhere.path());

    let found = listening_in(copy.path()).unwrap();
    let _ = (inside.kill(), outside.kill());
    let ports: Vec<u16> = found.iter().map(|l| l.port).collect();
    assert!(ports.contains(&port), "{found:?}");
    assert!(!ports.contains(&other), "{found:?}");
    let server = found.iter().find(|l| l.port == port).unwrap();
    assert_eq!(server.address, "127.0.0.1");
    assert!(server.reachable);
    assert!(
        server.process.to_lowercase().contains("python"),
        "{server:?}"
    );
}
