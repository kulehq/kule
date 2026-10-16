//! M3: a connection the policy puts to a person waits for an answer, for as
//! long as the task allows. Real sockets, a local upstream, a sandboxed
//! client; the test plays the person.

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kule_engine::{
    Answer, Approvals, DEFAULT_TIMEOUT, Network, Policy, Proxy, Sandbox, Task, connections,
};
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
    up: u16,
}

impl Setup {
    /// The built-in policy has no rule for loopback, so it asks.
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        fs::write(ws.path().join("README.md"), "hello\n").unwrap();
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

    fn asking(&self, approvals: &Approvals, timeout: Duration) -> Proxy {
        Proxy::start_asking(&self.task, Policy::builtin(), approvals, timeout).unwrap()
    }

    /// Connects to the upstream through `proxy` from inside the sandbox, on
    /// another thread; joins to what came back.
    fn connect(&self, proxy: &Proxy) -> JoinHandle<String> {
        let mut sandbox = Sandbox::new(&self.task).unwrap();
        sandbox.set_network(Network::Proxy(proxy.port()));
        let script = format!(
            r#"
import os, socket
port = int(os.environ["HTTPS_PROXY"].rsplit(":", 1)[1])
s = socket.create_connection(("127.0.0.1", port), timeout=30)
s.sendall(b"CONNECT 127.0.0.1:{up} HTTP/1.1\r\n\r\n")
data = b""
while True:
    chunk = s.recv(4096)
    if not chunk:
        break
    data += chunk
print(data.decode())
"#,
            up = self.up
        );
        let mut command = sandbox
            .command("python3", &["-c".to_owned(), script])
            .unwrap();
        std::thread::spawn(move || {
            let out = command.output().unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        })
    }

    fn answers(&self) -> Vec<Option<Answer>> {
        connections(&self.task)
            .unwrap()
            .into_iter()
            .filter(|c| c.port == self.up)
            .map(|c| c.answer)
            .collect()
    }
}

/// Waits until `approvals` has a question, and returns its id.
fn question(approvals: &Approvals) -> u64 {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if let Some(q) = approvals.waiting().first() {
            return q.id;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("nothing was asked");
}

#[test]
fn an_approved_connection_goes_through() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let heard = seen.clone();
    approvals.on_change(move |waiting| heard.lock().unwrap().push(waiting));
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let client = s.connect(&proxy);

    let id = question(&approvals);
    let asked = &approvals.waiting()[0];
    assert_eq!((asked.host.as_str(), asked.port), ("127.0.0.1", s.up));
    assert_eq!(asked.task, s.task.id());
    assert!(approvals.answer(id, true));

    let reply = client.join().unwrap();
    assert!(reply.contains("200"), "{reply}");
    assert!(reply.contains("hello from upstream"), "{reply}");
    proxy.stop();
    assert_eq!(s.answers(), [Some(Answer::Approved)]);
    assert!(approvals.waiting().is_empty());
    // The window heard the question come and go.
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|w| w.len() == 1));
    assert!(seen.last().unwrap().is_empty());
}

#[test]
fn a_denied_connection_is_refused() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let client = s.connect(&proxy);

    approvals.answer(question(&approvals), false);
    let reply = client.join().unwrap();
    assert!(
        reply.contains("403") && reply.contains("denied by a person"),
        "{reply}"
    );
    proxy.stop();
    assert_eq!(s.answers(), [Some(Answer::Denied)]);
}

#[test]
fn nobody_answering_refuses_it_with_a_reason_to_go_on() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let proxy = s.asking(&approvals, Duration::from_millis(300));
    let reply = s.connect(&proxy).join().unwrap();

    assert!(
        reply.contains("403") && reply.contains("skip this step"),
        "{reply}"
    );
    assert!(approvals.waiting().is_empty());
    proxy.stop();
    assert_eq!(s.answers(), [Some(Answer::TimedOut)]);
}

#[test]
fn without_approvals_nobody_is_asked() {
    let s = Setup::new();
    let proxy = Proxy::start(&s.task, Policy::builtin()).unwrap();
    let reply = s.connect(&proxy).join().unwrap();
    assert!(
        reply.contains("403") && reply.contains("waits for approval"),
        "{reply}"
    );
    proxy.stop();
    assert_eq!(s.answers(), [None]);
}

#[test]
fn stopping_the_proxy_withdraws_its_questions() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let client = s.connect(&proxy);
    question(&approvals);

    let start = Instant::now();
    proxy.stop();
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
    assert!(approvals.waiting().is_empty());
    // The client hears the refusal, or the cut if that comes first.
    let reply = client.join().unwrap();
    assert!(!reply.contains("200"), "{reply}");
    assert_eq!(s.answers(), [Some(Answer::Denied)]);
}

#[test]
fn a_task_waits_fifteen_minutes_unless_a_person_sets_it_within_bounds() {
    let s = Setup::new();
    assert_eq!(s.task.approval_timeout().unwrap(), DEFAULT_TIMEOUT);
    s.task
        .set_approval_timeout(Duration::from_secs(5 * 60))
        .unwrap();
    let again = Task::open(
        s.task
            .copy()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap(),
        s.task.id(),
    )
    .unwrap();
    assert_eq!(again.approval_timeout().unwrap(), Duration::from_secs(300));
    for wrong in [
        Duration::ZERO,
        Duration::from_secs(30),
        Duration::from_secs(9 * 3600),
    ] {
        assert!(s.task.set_approval_timeout(wrong).is_err());
    }
    assert_eq!(s.task.approval_timeout().unwrap(), Duration::from_secs(300));
}

/// A raw client of `proxy`, asking for a tunnel to `target` on its own
/// thread; joins to the status line.
fn ask_through(proxy: &Proxy, target: &str) -> JoinHandle<String> {
    use std::io::{BufRead, BufReader};
    let (port, target) = (proxy.port(), target.to_owned());
    std::thread::spawn(move || {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "CONNECT {target} HTTP/1.1\r\n\r\n").unwrap();
        let mut status = String::new();
        let _ = BufReader::new(s).read_line(&mut status);
        status
    })
}

#[test]
fn one_answer_per_host_and_port_in_a_run() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let target = format!("127.0.0.1:{}", s.up);
    let first = ask_through(&proxy, &target);
    let id = question(&approvals);
    let second = ask_through(&proxy, &target);
    std::thread::sleep(Duration::from_millis(200));
    // The second connection waits on the same question.
    assert_eq!(approvals.waiting().len(), 1);

    approvals.answer(id, true);
    assert!(first.join().unwrap().contains("200"));
    assert!(second.join().unwrap().contains("200"));
    // And the next one is not asked again.
    let third = ask_through(&proxy, &target).join().unwrap();
    assert!(third.contains("200"), "{third}");
    assert!(approvals.waiting().is_empty());
}

#[test]
fn a_task_cannot_bury_the_person_in_questions() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let waiting: Vec<_> = (0..kule_engine::MAX_WAITING)
        .map(|i| ask_through(&proxy, &format!("host{i}.invalid:443")))
        .collect();
    let start = Instant::now();
    while approvals.waiting().len() < kule_engine::MAX_WAITING {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let one_more = ask_through(&proxy, "one-more.invalid:443").join().unwrap();
    assert!(one_more.contains("403"), "{one_more}");
    assert_eq!(approvals.waiting().len(), kule_engine::MAX_WAITING);
    proxy.stop();
    for client in waiting {
        let _ = client.join();
    }
}
