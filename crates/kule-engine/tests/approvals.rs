//! M3: a connection the policy puts to a person waits for an answer, for as
//! long as the task allows. Real sockets, a local upstream, a sandboxed
//! client; the test plays the person.
#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use kule_engine::{
    AfterApproval, Answer, Approvals, DEFAULT_TIMEOUT, Network, Policy, Proxy, Sandbox, Settled,
    Task, connections,
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
        Proxy::start_asking(&self.task, Policy::builtin(), approvals, timeout, &[]).unwrap()
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
    // Tunneled: Kule does not look inside it.
    assert_eq!(asked.inside, None);
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
    let settled = settled(&approvals);
    let proxy = s.asking(&approvals, Duration::from_millis(300));
    let reply = s.connect(&proxy).join().unwrap();

    assert!(
        reply.contains("403") && reply.contains("skip this step"),
        "{reply}"
    );
    assert!(approvals.waiting().is_empty());
    proxy.stop();
    assert_eq!(s.answers(), [Some(Answer::TimedOut)]);
    // Told, though nobody answered: the card is gone, the person learns why.
    let told = settled.lock().unwrap().clone();
    assert!(
        matches!(&told[..], [Settled { question: Some(_), answer: Answer::TimedOut, port, .. }] if *port == s.up),
        "{told:?}"
    );
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
    let settled = settled(&approvals);
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
    assert!(!reply.contains("by a person"), "{reply}");
    // Kule's doing, not the person's.
    assert_eq!(s.answers(), [Some(Answer::Withdrawn)]);
    let told = settled.lock().unwrap().clone();
    assert!(
        matches!(
            &told[..],
            [Settled {
                question: Some(_),
                answer: Answer::Withdrawn,
                ..
            }]
        ),
        "{told:?}"
    );
}

#[test]
fn a_task_waits_fifteen_minutes_unless_a_person_sets_it_within_bounds() {
    let s = Setup::new();
    assert_eq!(s.task.approval_timeout().unwrap(), DEFAULT_TIMEOUT);
    s.task
        .set_approval_timeout(Duration::from_secs(5 * 60))
        .unwrap();
    // The copy is `<home>/tasks/<id>/w/<name>`.
    let again = Task::open(s.task.copy().ancestors().nth(4).unwrap(), s.task.id()).unwrap();
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
    let settled = settled(&approvals);
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let waiting: Vec<_> = (0..kule_engine::MAX_WAITING)
        .map(|i| ask_through(&proxy, &format!("host{i}.invalid:443")))
        .collect();
    let start = Instant::now();
    while approvals.waiting().len() < kule_engine::MAX_WAITING {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let one_more = reply_through(&proxy, "one-more.invalid:443");
    assert!(one_more.contains("403"), "{one_more}");
    // Kule refused it, without asking: never "denied by a person".
    assert!(
        one_more.contains("refused by Kule without asking") && !one_more.contains("by a person"),
        "{one_more}"
    );
    let again = reply_through(&proxy, "one-more.invalid:443");
    assert!(again.contains("refused by Kule without asking"), "{again}");
    assert_eq!(approvals.waiting().len(), kule_engine::MAX_WAITING);
    // Told once, though asked twice.
    let told: Vec<Settled> = settled
        .lock()
        .unwrap()
        .iter()
        .filter(|t| t.host == "one-more.invalid")
        .cloned()
        .collect();
    assert!(
        matches!(
            &told[..],
            [Settled {
                question: None,
                answer: Answer::TooMany,
                ..
            }]
        ),
        "{told:?}"
    );
    proxy.stop();
    for client in waiting {
        let _ = client.join();
    }
}

/// What the `approvals` listeners are told of questions that stop waiting.
fn settled(approvals: &Approvals) -> std::sync::Arc<std::sync::Mutex<Vec<Settled>>> {
    let told = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let into = std::sync::Arc::clone(&told);
    approvals.on_settled(move |s| into.lock().unwrap().push(s.clone()));
    told
}

/// What the `approvals` listeners are told of approved connections that
/// did not go through as approved.
fn after(approvals: &Approvals) -> std::sync::Arc<std::sync::Mutex<Vec<AfterApproval>>> {
    let told = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let into = std::sync::Arc::clone(&told);
    approvals.on_after_approval(move |a| into.lock().unwrap().push(a.clone()));
    told
}

/// Asks `proxy` for a tunnel to `target` and reads all it answers.
fn reply_through(proxy: &Proxy, target: &str) -> String {
    use std::io::Read;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    write!(s, "CONNECT {target} HTTP/1.1\r\n\r\n").unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reply = Vec::new();
    let _ = s.read_to_end(&mut reply);
    String::from_utf8_lossy(&reply).into_owned()
}

/// Waits until `told` holds something, and returns it.
fn told<T: Clone>(told: &std::sync::Mutex<Vec<T>>) -> Vec<T> {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let now = told.lock().unwrap().clone();
        if !now.is_empty() {
            return now;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("nothing was told");
}

/// A question says what Kule does inside the connection once approved:
/// for a host a stored secret goes to, the secrets it fills; for a
/// registry, that it is read only; otherwise nothing, as it is tunneled.
#[test]
fn a_question_says_whether_kule_looks_inside() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    kule_engine::add_secret(
        home.path(),
        "STRIPE_KEY",
        "real-value",
        &["api.stripe.example".into()],
        None,
    )
    .unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let approvals = Approvals::new();
    let proxy =
        Proxy::start_asking(&task, Policy::builtin(), &approvals, DEFAULT_TIMEOUT, &[]).unwrap();
    let asking = |target: &'static str| {
        let client = ask_through(&proxy, target);
        let id = question(&approvals);
        let asked = approvals.waiting()[0].clone();
        approvals.answer(id, false);
        let _ = client.join();
        asked
    };
    let filled = asking("API.Stripe.example.:443");
    let inside = filled.inside.expect("looked into");
    assert_eq!(inside.fills, ["STRIPE_KEY"]);
    assert!(!inside.read_only);
    // Not on 443: tunneled, secret or not.
    assert_eq!(asking("api.stripe.example:8443").inside, None);
    assert_eq!(asking("elsewhere.example:443").inside, None);
    proxy.stop();
}

/// A connection the person approved and Kule then cut (its TLS hello asks
/// for another host) is told with why, as it happens, beside the approval.
#[test]
fn an_approved_connection_kule_cuts_is_told_with_why() {
    use std::io::Read;
    let s = Setup::new();
    let approvals = Approvals::new();
    let after = after(&approvals);
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let (port, up) = (proxy.port(), s.up);
    let client = std::thread::spawn(move || {
        let mut c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(c, "CONNECT 127.0.0.1:{up} HTTP/1.1\r\n\r\n").unwrap();
        c.write_all(&client_hello("other.example")).unwrap();
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reply = Vec::new();
        let _ = c.read_to_end(&mut reply);
        String::from_utf8_lossy(&reply).into_owned()
    });
    approvals.answer(question(&approvals), true);
    // The upstream speaks first, so the client may hear it before the cut.
    let reply = client.join().unwrap();
    assert!(reply.contains("200"), "{reply}");
    let told = told(&after);
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!((told[0].host.as_str(), told[0].port), ("127.0.0.1", s.up));
    assert!(
        told[0].why.starts_with("cut by Kule") && told[0].why.contains("other.example"),
        "{told:?}"
    );
    // One that goes through as approved is not news.
    let quiet = Setup::new();
    let fine = after_quiet(&quiet);
    assert!(fine.is_empty(), "{fine:?}");
    proxy.stop();
}

/// An approved connection to `s`'s upstream that goes through: what the
/// listeners are told after it.
fn after_quiet(s: &Setup) -> Vec<AfterApproval> {
    let approvals = Approvals::new();
    let after = after(&approvals);
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let client = s.connect(&proxy);
    approvals.answer(question(&approvals), true);
    assert!(client.join().unwrap().contains("hello from upstream"));
    proxy.stop();
    after.lock().unwrap().clone()
}

/// An approved connection to an address on this computer's network is
/// refused all the same. The person is told the address; the command only
/// that such addresses are out of reach.
#[test]
fn an_approved_private_address_is_refused_saying_where_to_the_person_only() {
    let s = Setup::new();
    let approvals = Approvals::new();
    let after = after(&approvals);
    let proxy = s.asking(&approvals, DEFAULT_TIMEOUT);
    let client = ask_through(&proxy, "10.255.0.1:443");
    approvals.answer(question(&approvals), true);
    let status = client.join().unwrap();
    assert!(status.contains("403"), "{status}");
    let reply = reply_through(&proxy, "10.255.0.1:443");
    assert!(
        reply.contains("not reachable") && !reply.contains("10.255"),
        "{reply}"
    );
    let told = told(&after);
    assert!(
        told[0].why.starts_with("refused by Kule")
            && told[0].why.contains("10.255.0.1 is an address"),
        "{told:?}"
    );
    proxy.stop();
    let logged = connections(&s.task).unwrap();
    assert!(
        logged.iter().any(|c| c.answer == Some(Answer::Approved)
            && c.refused
                .as_deref()
                .is_some_and(|r| r.contains("10.255.0.1"))),
        "{logged:?}"
    );
}

/// The first bytes a TLS client sends to `name`: its hello, made by rustls.
fn client_hello(name: &str) -> Vec<u8> {
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(name.to_owned()).unwrap();
    let mut conn = rustls::ClientConnection::new(std::sync::Arc::new(config), name).unwrap();
    let mut out = Vec::new();
    conn.write_tls(&mut out).unwrap();
    out
}
