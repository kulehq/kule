//! The proxy as a process of its own, reached over its pipes: what Kule
//! hands it, what it asks back and what it logs, on every platform. The
//! program runs here as a plain child process; what contains it on
//! Windows is tested in `container.rs`.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use kule_engine::{
    AgentKey, Answer, Approvals, DEFAULT_POLICY, Policy, Proxy, Settled, Task, Verdict, connections,
};
use tempfile::TempDir;

const PROXY: &str = env!("CARGO_BIN_EXE_kule-proxy");

/// The built-in policy, plus loopback by address for the tests' upstream.
fn policy() -> Policy {
    let local = r#"
@id("test-upstream")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && resource.host == "127.0.0.1" };
"#;
    Policy::from_texts(&format!("{DEFAULT_POLICY}\n{local}"), None).unwrap()
}

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
    up: u16,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        std::fs::write(ws.path().join("README.md"), "hello\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let up = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let _ = stream.unwrap().write_all(b"hello from upstream\n");
            }
        });
        Setup {
            _home: home,
            _ws: ws,
            task,
            up,
        }
    }

    fn through(&self, policy: Policy) -> Proxy {
        Proxy::start_through(Path::new(PROXY), &self.task, policy, None, &[]).unwrap()
    }

    /// The built-in policy has no rule for loopback, so it asks.
    fn asking(&self, approvals: &Approvals) -> Proxy {
        Proxy::start_through(
            Path::new(PROXY),
            &self.task,
            Policy::from_texts(DEFAULT_POLICY, None).unwrap(),
            Some((approvals, Duration::from_secs(60))),
            &[],
        )
        .unwrap()
    }

    /// The log entries for `host`, once the proxy has written them.
    fn logged(&self, host: &str, n: usize) -> Vec<kule_engine::Connection> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let seen: Vec<_> = connections(&self.task)
                .unwrap()
                .into_iter()
                .filter(|c| c.host == host)
                .collect();
            if seen.len() >= n || Instant::now() > deadline {
                assert_eq!(seen.len(), n, "{seen:?}");
                return seen;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Asks the proxy on `port` for a tunnel to `target` and returns whatever
/// comes back, once the connection closes.
fn connect(port: u16, target: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    s.write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes())
        .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < deadline, "{what} did not happen in time");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn an_allowed_host_is_reached_through_the_proxy_process_and_logged() {
    let s = Setup::new();
    let proxy = s.through(policy());
    assert_ne!(proxy.port(), 0);
    let reply = connect(proxy.port(), &format!("127.0.0.1:{}", s.up));
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("hello from upstream"), "{reply}");
    proxy.stop();
    let logged = s.logged("127.0.0.1", 1);
    assert_eq!(logged[0].verdict, Verdict::Allow);
    assert_eq!(logged[0].outcome, "tunneled");
    assert_eq!(logged[0].port, s.up);
}

#[test]
fn a_host_the_policy_does_not_let_through_is_refused_and_logged() {
    let s = Setup::new();
    let proxy = s.through(policy());
    let reply = connect(proxy.port(), "192.168.1.1:80");
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    proxy.stop();
    let logged = s.logged("192.168.1.1", 1);
    assert_eq!(logged[0].verdict, Verdict::Ask);
    assert_eq!(logged[0].outcome, "refused");
    assert_eq!(logged[0].answer, None);
}

/// A repository's forbid crosses the pipes under the repository's name:
/// the process judges by the same rules, told the same way.
#[test]
fn the_rules_cross_the_pipes_with_their_names() {
    let s = Setup::new();
    let repo = r#"
@id("no-loopback")
forbid (principal, action, resource)
when { resource.kind == "net.connect" && resource.host == "127.0.0.1" };
"#;
    let local = r#"
@id("test-upstream")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && resource.host == "127.0.0.1" };
"#;
    let policy = Policy::from_texts(&format!("{DEFAULT_POLICY}\n{local}"), Some(repo)).unwrap();
    let proxy = s.through(policy);
    let reply = connect(proxy.port(), &format!("127.0.0.1:{}", s.up));
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    proxy.stop();
    let logged = s.logged("127.0.0.1", 1);
    assert_eq!(logged[0].verdict, Verdict::Deny);
    assert_eq!(logged[0].rules, ["repo:no-loopback"], "{:?}", logged[0]);
}

/// A question crosses the pipes to the person, and their answer goes
/// back: the connection waits for it and goes through once approved.
#[test]
fn a_question_crosses_the_pipes_and_so_does_the_answer() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let settled: Arc<Mutex<Vec<Settled>>> = Arc::default();
    approvals.on_settled({
        let settled = Arc::clone(&settled);
        move |s| settled.lock().unwrap().push(s.clone())
    });
    let proxy = s.asking(&approvals);
    let (port, up) = (proxy.port(), s.up);
    let client = std::thread::spawn(move || connect(port, &format!("127.0.0.1:{up}")));
    wait_until("the question", || !approvals.waiting().is_empty());
    let question = approvals.waiting().remove(0);
    assert_eq!((question.host.as_str(), question.port), ("127.0.0.1", up));
    assert_eq!(question.task, s.task.id());
    assert!(approvals.answer(question.id, true));
    let reply = client.join().unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains("hello from upstream"), "{reply}");
    proxy.stop();
    let logged = s.logged("127.0.0.1", 1);
    assert_eq!(logged[0].verdict, Verdict::Ask);
    assert_eq!(logged[0].answer, Some(Answer::Approved));
    assert_eq!(logged[0].outcome, "tunneled");
    let settled = settled.lock().unwrap();
    assert_eq!(settled.len(), 1, "{settled:?}");
    assert_eq!(settled[0].answer, Answer::Approved);
}

/// Stopping the proxy withdraws the question still waiting, the client
/// is told so, and the process is gone.
#[test]
fn stopping_withdraws_the_question_waiting_and_ends_the_process() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let proxy = s.asking(&approvals);
    let (port, up) = (proxy.port(), s.up);
    let pid = proxy.child_pid().unwrap();
    assert!(kule_engine::osfs::alive(pid));
    let client = std::thread::spawn(move || connect(port, &format!("127.0.0.1:{up}")));
    wait_until("the question", || !approvals.waiting().is_empty());
    let started = Instant::now();
    drop(proxy);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "stopping took {:?}",
        started.elapsed()
    );
    assert!(approvals.waiting().is_empty());
    let reply = client.join().unwrap();
    assert!(reply.starts_with("HTTP/1.1 403"), "{reply}");
    assert!(
        reply.contains("the run ended before anyone answered"),
        "{reply}"
    );
    wait_until("the process's end", || !kule_engine::osfs::alive(pid));
    let logged = s.logged("127.0.0.1", 1);
    assert_eq!(logged[0].answer, Some(Answer::Withdrawn));
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

/// What the commands get comes from the process: the placeholder of an
/// agent's key, and the authority the process made, in the task's tmp.
#[test]
fn the_placeholders_and_the_authority_come_from_the_process() {
    let s = Setup::new();
    let key = AgentKey {
        name: "AGENT_KEY".to_owned(),
        value: "sk-test-0123456789abcdef".to_owned(),
        hosts: vec!["api.example".to_owned()],
    };
    let proxy = Proxy::start_through(Path::new(PROXY), &s.task, policy(), None, &[key]).unwrap();
    let env: std::collections::HashMap<_, _> = proxy.env().iter().cloned().collect();
    let placeholder = env["AGENT_KEY"].to_string_lossy().into_owned();
    assert!(placeholder.starts_with("kule_secret_"), "{placeholder}");
    let ca = std::fs::read_to_string(&env["NODE_EXTRA_CA_CERTS"]).unwrap();
    assert!(ca.starts_with("-----BEGIN CERTIFICATE-----"), "{ca}");
    let bundle = std::fs::read_to_string(&env["SSL_CERT_FILE"]).unwrap();
    assert!(
        bundle.ends_with(&ca),
        "the bundle ends with the proxy's authority"
    );
}

/// A program that is not there is told of, not waited for.
#[test]
fn a_program_that_cannot_start_is_told_of() {
    let s = Setup::new();
    let missing = s._home.path().join("no-such-kule-proxy");
    let started = Instant::now();
    let err = match Proxy::start_through(&missing, &s.task, policy(), None, &[]) {
        Ok(_) => panic!("started a program that is not there"),
        Err(e) => e.to_string(),
    };
    assert!(err.starts_with("Kule's proxy did not start"), "{err}");
    assert!(started.elapsed() < Duration::from_secs(5));
}
