//! The proxy against a real upstream: the gate decides, and hears how each
//! connection ended.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use lonca_net::{Gate, Outcome, Request, Verdict};

/// Lets through only `allowed`, reaching private addresses (the upstream is
/// on loopback), and records every outcome.
struct Recorder {
    allowed: String,
    outcomes: Mutex<Vec<(String, Outcome)>>,
}

impl Gate for Recorder {
    fn decide(&self, request: &Request<'_>) -> Verdict {
        if request.host == self.allowed {
            Verdict {
                allow: true,
                allow_private: true,
                reason: String::new(),
            }
        } else {
            Verdict::deny("not on the list")
        }
    }

    fn finished(&self, request: &Request<'_>, outcome: Outcome) {
        self.outcomes
            .lock()
            .unwrap()
            .push((request.host.to_owned(), outcome));
    }
}

fn upstream() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let _ = s.write_all(b"hello from upstream\n");
        }
    });
    port
}

fn connect(proxy: u16, target: &str) -> (String, String) {
    let mut s = TcpStream::connect(("127.0.0.1", proxy)).unwrap();
    write!(s, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").unwrap();
    let mut r = BufReader::new(s);
    let mut status = String::new();
    r.read_line(&mut status).unwrap();
    let mut rest = String::new();
    let _ = r.read_to_string(&mut rest);
    (status.trim().to_owned(), rest)
}

fn wait_for(gate: &Recorder, n: usize) -> Vec<(String, Outcome)> {
    for _ in 0..200 {
        let seen = gate.outcomes.lock().unwrap().clone();
        if seen.len() >= n {
            return seen;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the gate heard {:?}", gate.outcomes.lock().unwrap());
}

#[test]
fn the_gate_decides_and_hears_how_it_ended() {
    let gate = Arc::new(Recorder {
        allowed: "127.0.0.1".into(),
        outcomes: Mutex::new(Vec::new()),
    });
    let listener = lonca_net::bind("127.0.0.1:0").unwrap();
    let proxy = listener.local_addr().unwrap().port();
    let served = gate.clone();
    std::thread::spawn(move || lonca_net::serve(listener, served));
    let up = upstream();

    let (status, body) = connect(proxy, &format!("127.0.0.1:{up}"));
    assert!(status.contains("200"), "{status}");
    assert!(body.contains("hello from upstream"), "{body}");

    let (status, _) = connect(proxy, "elsewhere.example:443");
    assert!(status.contains("403"), "{status}");

    let mut seen = wait_for(&gate, 2);
    seen.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        seen,
        [
            ("127.0.0.1".to_owned(), Outcome::Tunneled),
            ("elsewhere.example".to_owned(), Outcome::Refused),
        ]
    );
}

#[test]
fn only_loopback_can_be_bound() {
    assert!(lonca_net::bind("0.0.0.0:0").is_err());
}
