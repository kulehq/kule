//! M1, step 4: the network goes through the task's proxy, which asks the
//! policy about every connection and logs it. Real sockets, a local upstream,
//! no outside network.
#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream, UdpSocket};
use std::process::Output;
use std::time::Duration;

use kule_engine::{
    AgentKey, DEFAULT_POLICY, Network, Policy, Proxy, Sandbox, Task, Verdict, connections,
};
use tempfile::TempDir;

/// The built-in policy, plus loopback for the tests' upstream.
fn policy() -> Policy {
    let local = r#"
@id("test-upstream")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && resource.host == "127.0.0.1" };
@id("test-lan")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && resource.host == "192.168.1.1" };
@id("test-not-public")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "net.connect" && ["198.18.0.1", "240.0.0.1", "64:ff9b::7f00:1", "::127.0.0.1"].contains(resource.host) };
"#;
    Policy::from_texts(&format!("{DEFAULT_POLICY}\n{local}"), None).unwrap()
}

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
    sandbox: Sandbox,
    _proxy: Proxy,
}

impl Setup {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("README.md"), "hello\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        let proxy = Proxy::start(&task, policy()).unwrap();
        let mut sandbox = Sandbox::new(&task).unwrap();
        sandbox.set_network(Network::Proxy(proxy.port()));
        Setup {
            _home: home,
            _ws: ws,
            task,
            sandbox,
            _proxy: proxy,
        }
    }

    fn python(&self, script: &str) -> Output {
        self.sandbox
            .command("python3", &["-c".to_owned(), script.to_owned()])
            .unwrap()
            .output()
            .unwrap()
    }

    /// Asks the proxy named in HTTPS_PROXY for a tunnel to `target` and
    /// prints whatever comes back.
    fn connect_via_proxy(&self, target: &str) -> String {
        let script = format!(
            r#"
import os, socket
port = int(os.environ["HTTPS_PROXY"].rsplit(":", 1)[1])
s = socket.create_connection(("127.0.0.1", port), timeout=5)
s.sendall(b"CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n")
data = b""
while True:
    chunk = s.recv(4096)
    if not chunk:
        break
    data += chunk
print(data.decode())
"#
        );
        let out = self.python(&script);
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// The log entries for `host`, once the proxy has written them.
    fn logged(&self, host: &str, n: usize) -> Vec<kule_engine::Connection> {
        for _ in 0..300 {
            let seen: Vec<_> = connections(&self.task)
                .unwrap()
                .into_iter()
                .filter(|c| c.host == host)
                .collect();
            if seen.len() >= n {
                return seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("{host} not logged: {:?}", connections(&self.task).unwrap());
    }
}

fn upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let _ = stream.unwrap().write_all(b"hello from upstream\n");
        }
    });
    port
}

#[test]
fn an_allowed_host_is_reached_through_the_proxy_and_logged() {
    let s = Setup::new();
    let up = upstream();
    let reply = s.connect_via_proxy(&format!("127.0.0.1:{up}"));
    assert!(reply.contains("200 Connection established"), "{reply}");
    assert!(reply.contains("hello from upstream"), "{reply}");

    let log = s.logged("127.0.0.1", 1);
    assert_eq!(log[0].port, up);
    assert_eq!(log[0].verdict, Verdict::Allow);
    assert_eq!(log[0].rules, ["test-upstream"]);
    assert_eq!(log[0].outcome, "tunneled");
}

#[test]
fn a_host_the_policy_does_not_let_through_is_refused_and_logged() {
    let s = Setup::new();
    let reply = s.connect_via_proxy("exfiltrate.example:443");
    assert!(reply.contains("403"), "{reply}");
    assert!(reply.contains("waits for approval"), "{reply}");

    let log = s.logged("exfiltrate.example", 1);
    assert_eq!(log[0].verdict, Verdict::Ask);
    assert_eq!(log[0].outcome, "refused");
}

#[test]
fn the_lan_stays_out_of_reach_even_when_the_policy_allows_it() {
    let s = Setup::new();
    let reply = s.connect_via_proxy("192.168.1.1:443");
    assert!(reply.contains("403"), "{reply}");
    assert!(reply.contains("not reachable"), "{reply}");
    let log = s.logged("192.168.1.1", 1);
    assert_eq!(log[0].verdict, Verdict::Allow);
    // Refused by Kule's own check, before the proxy's would.
    assert_eq!(log[0].outcome, "refused");
}

/// Ranges the proxy library takes for public are judged by Kule's own
/// check before anything connects: a benchmarking network, a reserved one,
/// and this computer inside an IPv6 address.
#[test]
fn addresses_that_are_not_public_are_refused_before_connecting() {
    let s = Setup::new();
    for (target, host) in [
        ("198.18.0.1:443", "198.18.0.1"),
        ("240.0.0.1:443", "240.0.0.1"),
        ("[64:ff9b::7f00:1]:443", "64:ff9b::7f00:1"),
        ("[::127.0.0.1]:443", "::127.0.0.1"),
    ] {
        let reply = s.connect_via_proxy(target);
        assert!(reply.contains("403"), "{target}: {reply}");
        assert!(reply.contains("not reachable"), "{target}: {reply}");
        let log = s.logged(host, 1);
        assert_eq!(log[0].verdict, Verdict::Allow, "{target}");
        assert_eq!(log[0].outcome, "refused", "{target}");
    }
}

#[test]
fn connections_around_the_proxy_are_blocked() {
    let s = Setup::new();
    let up = upstream();
    let out = s.python(&format!(
        "import socket; socket.create_connection((\"127.0.0.1\", {up}), timeout=5)"
    ));
    assert!(!out.status.success(), "a direct connection went through");
    assert!(connections(&s.task).unwrap().is_empty());
}

/// An upstream that keeps every connection open and never answers.
fn silent_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            held.push(stream.unwrap());
        }
    });
    port
}

#[test]
fn stopping_the_proxy_logs_the_tunnels_a_command_left_open() {
    let s = Setup::new();
    let proxy = Proxy::start(&s.task, policy()).unwrap();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.set_network(Network::Proxy(proxy.port()));
    let up = silent_upstream();
    let script = format!(
        r#"
import os, socket
port = int(os.environ["HTTPS_PROXY"].rsplit(":", 1)[1])
s = socket.create_connection(("127.0.0.1", port), timeout=5)
s.sendall(b"CONNECT 127.0.0.1:{up} HTTP/1.1\r\n\r\n")
print(s.recv(4096).decode().splitlines()[0])
"#
    );
    let out = sandbox
        .command("python3", &["-c".to_owned(), script])
        .unwrap()
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("200"),
        "{out:?}"
    );

    let logged = || {
        connections(&s.task)
            .unwrap()
            .iter()
            .any(|c| c.port == up && c.outcome == "tunneled")
    };
    // The command is gone, but the upstream never closed: still open.
    assert!(!logged());
    proxy.stop();
    assert!(logged());
}

#[test]
fn a_dropped_proxy_stops_serving() {
    let s = Setup::new();
    let proxy = Proxy::start(&s.task, policy()).unwrap();
    let port = proxy.port();
    assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
    drop(proxy);
    // Another test's proxy may get the port next, so ask what this task's
    // log heard rather than whether the port answers.
    let logged = connections(&s.task).unwrap().len();
    if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
        let _ = stream.write_all(b"CONNECT 127.0.0.1:9 HTTP/1.1\r\n\r\n");
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert_eq!(connections(&s.task).unwrap().len(), logged);
}

#[test]
fn commands_find_the_proxy_in_the_usual_variables() {
    let s = Setup::new();
    let out = s.python(
        "import os; print(os.environ['HTTPS_PROXY'], os.environ['http_proxy'], os.environ.get('NO_PROXY'), os.environ.get('NODE_USE_ENV_PROXY'), os.environ['JAVA_TOOL_OPTIONS']); a = os.environ['MAVEN_ARGS'].split(' ', 1); print(a[0], open(a[1]).read().count('<port>' + os.environ['HTTPS_PROXY'].rsplit(':', 1)[1] + '</port>'), os.environ['DOTNET_SYSTEM_NET_DISABLEIPV6'])",
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = text.split_whitespace().collect();
    assert!(parts[0].starts_with("http://127.0.0.1:"), "{text}");
    assert_eq!(parts[0], parts[1]);
    assert_eq!(parts[2], "None");
    // Node's fetch reads the variables only when this says so.
    assert_eq!(parts[3], "1");
    // Java reads none of them: every JVM is told the same proxy.
    let port = parts[0].rsplit(':').next().unwrap();
    assert_eq!(
        parts[4..9],
        [
            "-Dhttp.proxyHost=127.0.0.1".to_owned(),
            format!("-Dhttp.proxyPort={port}"),
            "-Dhttps.proxyHost=127.0.0.1".to_owned(),
            format!("-Dhttps.proxyPort={port}"),
            "-Djava.net.preferIPv4Stack=true".to_owned(),
        ],
        "{text}"
    );
    // Maven takes the proxy from a settings file of the task's: once for
    // http, once for https.
    assert_eq!(parts[9..11], ["-s", "2"], "{text}");
    // .NET would reach the proxy through an IPv6 socket, which the macOS
    // sandbox does not take for loopback: IPv4 only.
    assert_eq!(parts[11..], ["1"], "{text}");
}

/// A program in the task reaches a server the task runs (Gradle its daemon,
/// a test the server beside it): on Linux, where the task's network
/// namespace holds nothing but its own programs and the proxy's relay. On
/// macOS the task's programs share this computer's loopback with the
/// person's own services, so nothing listens unless the person allows
/// servers, and a connection to anything but the proxy stays refused
/// (docs/troubleshooting.md).
#[test]
fn a_tasks_programs_reach_the_tasks_own_servers() {
    let s = Setup::new();
    let out = s.python(
        r#"
import http.server as h, socket, threading
try:
    s = h.ThreadingHTTPServer(("127.0.0.1", 0), h.SimpleHTTPRequestHandler)
    threading.Thread(target=s.serve_forever, daemon=True).start()
    c = socket.create_connection(("127.0.0.1", s.server_port), timeout=5)
    c.sendall(b"GET / HTTP/1.0\r\n\r\n")
    print("reached" if c.recv(12).startswith(b"HTTP/1.0 200") else "odd")
except OSError as e:
    print("refused", e.errno)
"#,
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    if cfg!(target_os = "linux") {
        assert_eq!(text.trim(), "reached", "{text}\n{err}");
    } else {
        assert!(text.trim().starts_with("refused"), "{text}\n{err}");
    }
}

#[test]
fn the_default_policy_lets_registries_through_only() {
    let policy = Policy::builtin();
    for host in ["registry.npmjs.org", "index.crates.io", "api.nuget.org"] {
        assert_eq!(
            policy.judge_connection(host, 443).verdict,
            Verdict::Allow,
            "{host}"
        );
    }
    assert_eq!(
        policy.judge_connection("registry.npmjs.org", 80).verdict,
        Verdict::Ask
    );
    // Model APIs only for the agent whose profile opens them; Go's module
    // proxy fetches from any address a request names.
    for host in [
        "github.com",
        "pastebin.com",
        "127.0.0.1",
        "api.anthropic.com",
        "api.openai.com",
        "generativelanguage.googleapis.com",
        "proxy.golang.org",
        "sum.golang.org",
    ] {
        assert_eq!(
            policy.judge_connection(host, 443).verdict,
            Verdict::Ask,
            "{host}"
        );
    }
}

#[test]
fn a_profile_can_open_its_own_hosts_but_not_past_a_forbid() {
    let opened = Policy::builtin()
        .permit_hosts("profile-gemini", &["oauth2.googleapis.com"])
        .unwrap();
    let j = opened.judge_connection("oauth2.googleapis.com", 443);
    assert_eq!(
        (j.verdict, j.rules),
        (Verdict::Allow, vec!["profile-gemini".to_owned()])
    );
    assert_eq!(
        opened.judge_connection("oauth2.googleapis.com", 80).verdict,
        Verdict::Ask
    );
    assert_eq!(
        Policy::builtin()
            .judge_connection("oauth2.googleapis.com", 443)
            .verdict,
        Verdict::Ask
    );

    let forbid = r#"@id("no-google") forbid (principal, action, resource) when { resource.host like "*.googleapis.com" };"#;
    let tight = Policy::from_texts(DEFAULT_POLICY, Some(forbid))
        .unwrap()
        .permit_hosts("profile-gemini", &["oauth2.googleapis.com"])
        .unwrap();
    assert_eq!(
        tight.judge_connection("oauth2.googleapis.com", 443).verdict,
        Verdict::Deny
    );

    assert!(
        Policy::builtin()
            .permit_hosts("x", &["evil\" || true"])
            .is_err()
    );
}

/// This machine's address on its network, where it has one: what is sent
/// there leaves the loopback interface. No packet is sent to find it.
fn own_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

fn python_in(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .command("python3", &["-c".to_owned(), script.to_owned()])
        .unwrap()
        .output()
        .unwrap()
}

/// Landlock matches the port, not the address: a server elsewhere that
/// listens on the proxy's port number stays out of reach all the same.
/// (macOS counts the machine's own addresses as localhost, which the proxy
/// rule names; what is reached there never leaves the machine.)
#[cfg(target_os = "linux")]
#[test]
fn another_address_on_the_proxys_port_is_out_of_reach() {
    let Some(ip) = own_address() else {
        eprintln!("no address beyond loopback; nothing to try");
        return;
    };
    let s = Setup::new();
    let listener = TcpListener::bind((ip, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    // A sandbox whose proxy, as far as it knows, listens on that port.
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.set_network(Network::Proxy(port));
    let out = python_in(
        &sandbox,
        &format!("import socket; socket.create_connection((\"{ip}\", {port}), timeout=3)"),
    );
    assert!(!out.status.success(), "reached {ip}:{port} past the proxy");
}

/// No datagram leaves the sandbox: not to a local resolver, not to the
/// network. The proxy resolves names itself.
#[test]
fn datagrams_do_not_leave_the_sandbox() {
    let s = Setup::new();
    let mut targets = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    targets.extend(own_address());
    for ip in targets {
        let receiver = UdpSocket::bind((ip, 0)).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();
        let _ = python_in(
            &s.sandbox,
            &format!(
                "import socket\ns = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)\n\
                 for _ in range(3):\n    try: s.sendto(b'leak', (\"{ip}\", {port}))\n    except OSError: pass"
            ),
        );
        let mut buf = [0u8; 16];
        assert!(receiver.recv(&mut buf).is_err(), "a datagram reached {ip}");
    }
}

/// With a secret kept, the proxy looks inside TLS only for its hosts:
/// every other tunnel passes as it is, and the commands get the
/// placeholder and the authority to trust.
#[test]
fn a_secret_changes_nothing_for_other_hosts() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    kule_engine::add_secret(
        home.path(),
        "API_KEY",
        "real-value",
        &["api.example.com".into()],
        None,
    )
    .unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let proxy = Proxy::start(&task, policy()).unwrap();
    let names: Vec<_> = proxy.env().iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"API_KEY") && names.contains(&"SSL_CERT_FILE"),
        "{names:?}"
    );
    let placeholder = &proxy.env().iter().find(|(n, _)| n == "API_KEY").unwrap().1;
    assert_ne!(placeholder.to_str(), Some("real-value"));

    let up = upstream();
    let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
    write!(
        s,
        "CONNECT 127.0.0.1:{up} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
    )
    .unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut reply = String::new();
    let _ = std::io::Read::read_to_string(&mut s, &mut reply);
    assert!(reply.contains("hello from upstream"), "{reply}");
}

/// An agent's own key: the agent's commands see a placeholder of the run;
/// a kept secret of the same name wins; a key needs real host names.
#[test]
fn an_agents_key_is_a_placeholder_to_everything_the_run_starts() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    let key = |name: &str, host: &str| AgentKey {
        name: name.into(),
        value: "sk-real-key".into(),
        hosts: vec![host.into()],
    };
    let proxy = Proxy::start_with_keys(
        &task,
        policy(),
        &[key("MODEL_API_KEY", "api.model.example")],
    )
    .unwrap();
    let mut sandbox = Sandbox::for_task(&task).unwrap();
    sandbox.use_proxy(&proxy);
    let out = sandbox
        .command(
            "sh",
            &[
                "-c".into(),
                "echo \"$MODEL_API_KEY\"; test -n \"$SSL_CERT_FILE\" && echo trusts".into(),
            ],
        )
        .unwrap()
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.starts_with("kule_secret_") && out.contains("trusts"),
        "{out}"
    );
    assert!(!out.contains("sk-real-key"), "{out}");
    drop(proxy);

    kule_engine::add_secret(
        home.path(),
        "MODEL_API_KEY",
        "kept-value",
        &["api.model.example".into()],
        None,
    )
    .unwrap();
    let kept = Proxy::start(&task, policy()).unwrap();
    let with_key = Proxy::start_with_keys(
        &task,
        policy(),
        &[key("MODEL_API_KEY", "api.model.example")],
    )
    .unwrap();
    // One value under the name, as the sandbox sets the last one given.
    let placeholder = |p: &Proxy| {
        let all: Vec<_> = p
            .env()
            .iter()
            .filter(|(n, _)| n == "MODEL_API_KEY")
            .map(|(_, v)| v.clone())
            .collect();
        assert_eq!(all.len(), 1, "{all:?}");
        all[0].clone()
    };
    assert_eq!(placeholder(&kept), placeholder(&with_key));

    assert!(
        Proxy::start_with_keys(&task, policy(), &[key("OTHER_KEY", "https://x.example")]).is_err()
    );

    // A key pasted into a terminal with its paste markers: refused, naming
    // the variable and what is wrong, never the value.
    let pasted = AgentKey {
        name: "PASTED_KEY".into(),
        value: "\u{1b}[200~sk-pasted-key\u{1b}[201~".into(),
        hosts: vec!["api.model.example".into()],
    };
    let err = Proxy::start_with_keys(&task, policy(), &[pasted])
        .err()
        .expect("a value with escapes is refused")
        .to_string();
    assert!(
        err.starts_with("PASTED_KEY: ") && err.contains("control character"),
        "{err}"
    );
    assert!(!err.contains("sk-pasted-key"), "{err}");
}

/// Go checks certificates against the roots the proxy's variables name,
/// with the person's own Go settings kept: on macOS it would otherwise ask
/// the system's services, which a command reaches only with the network
/// open. A Go program whose module says Go 1.21 trusts the task's authority
/// as one of the system's roots.
#[test]
fn go_trusts_the_roots_the_proxy_names() {
    let has_go = std::process::Command::new("go")
        .arg("version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_go {
        eprintln!("skipped: no go here");
        return;
    }
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("go.mod"), "module probe\ngo 1.21\n").unwrap();
    fs::write(
        src.path().join("main.go"),
        r#"package main

import (
	"crypto/x509"
	"encoding/pem"
	"fmt"
	"os"
)

func main() {
	fmt.Println(os.Getenv("GODEBUG"))
	data, err := os.ReadFile(os.Getenv("NODE_EXTRA_CA_CERTS"))
	if err != nil {
		fmt.Println("read:", err)
		return
	}
	block, _ := pem.Decode(data)
	cert, err := x509.ParseCertificate(block.Bytes)
	if err != nil {
		fmt.Println("parse:", err)
		return
	}
	if _, err := cert.Verify(x509.VerifyOptions{}); err != nil {
		fmt.Println("verify:", err)
		return
	}
	fmt.Println("trusted")
}
"#,
    )
    .unwrap();
    let probe = src.path().join("probe");
    let built = std::process::Command::new("go")
        .args(["build", "-o"])
        .arg(&probe)
        .arg(".")
        .current_dir(src.path())
        .output()
        .unwrap();
    assert!(built.status.success(), "{built:?}");

    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    // A task runs a program only from a folder Kule grants.
    sandbox.grant_env(&[src.path().to_owned()], None);
    sandbox.use_proxy(&s._proxy);
    let out = sandbox
        .command(probe.to_str().unwrap(), &[])
        .unwrap()
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("x509sslcertoverrideplatform=1"),
        "{out:?}"
    );
    assert_eq!(lines.next(), Some("trusted"), "{out:?}");
}

/// The task's tmp is the sandbox's to write to, and kept between runs: a
/// link a command left there, at a name Kule writes or in a folder it keeps
/// there, leads none of Kule's own writes outside.
#[test]
fn links_left_in_the_tasks_tmp_lead_kules_writes_nowhere() {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    let home = TempDir::new().unwrap();
    let ws = TempDir::new().unwrap();
    let person = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    fs::write(ws.path().join("README.md"), "hello\n").unwrap();
    fs::create_dir_all(person.path().join(".cargo/registry")).unwrap();
    fs::write(person.path().join(".cargo/config.toml"), "[net]\n").unwrap();
    let sources = kule_engine::CacheSources {
        home: person.path().to_owned(),
        cargo_home: person.path().join(".cargo"),
    };
    let task = Task::create(home.path(), ws.path()).unwrap();
    let tmp = task.tmp().unwrap();
    let victim = outside.path().join("victim");
    fs::write(&victim, "mine\n").unwrap();
    let elsewhere = outside.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let untouched = |when: &str| {
        assert_eq!(fs::read_to_string(&victim).unwrap(), "mine\n", "{when}");
        let made: Vec<_> = fs::read_dir(&elsewhere).unwrap().flatten().collect();
        assert!(made.is_empty(), "{when}: {made:?}");
    };
    let run = || {
        let proxy = Proxy::start(&task, policy()).unwrap();
        let mut sandbox = Sandbox::new(&task).unwrap();
        sandbox.set_network(Network::Proxy(proxy.port()));
        sandbox.use_task_caches(&sources).unwrap();
        let out = sandbox.command("true", &[]).unwrap().output().unwrap();
        assert!(out.status.success(), "{out:?}");
    };

    // At the names Kule writes, and in place of the folders it keeps.
    symlink(&victim, tmp.join("kule-ca.pem")).unwrap();
    symlink(&victim, tmp.join("kule-ca-bundle.pem")).unwrap();
    symlink(&elsewhere, tmp.join(".kule-loopback")).unwrap();
    symlink(&elsewhere, tmp.join("caches")).unwrap();
    run();
    untouched("links at the top");
    for name in ["kule-ca.pem", "kule-ca-bundle.pem"] {
        let meta = fs::symlink_metadata(tmp.join(name)).unwrap();
        assert!(meta.is_file(), "{name}");
    }
    for dir in [".kule-loopback", "caches", "caches/cargo"] {
        let meta = fs::symlink_metadata(tmp.join(dir)).unwrap();
        assert!(meta.is_dir(), "{dir}");
    }
    assert!(tmp.join(".kule-loopback/sitecustomize.py").is_file());
    assert_eq!(
        fs::read_to_string(tmp.join("caches/cargo/config.toml")).unwrap(),
        "[net]\n"
    );

    // Inside the folders Kule made.
    fs::remove_file(tmp.join(".kule-loopback/sitecustomize.py")).unwrap();
    symlink(&victim, tmp.join(".kule-loopback/sitecustomize.py")).unwrap();
    fs::remove_dir_all(tmp.join("caches/cargo")).unwrap();
    symlink(&elsewhere, tmp.join("caches/cargo")).unwrap();
    fs::remove_dir_all(tmp.join("caches/pip")).unwrap();
    symlink(&elsewhere, tmp.join("caches/pip")).unwrap();
    run();
    untouched("links inside");
    assert!(
        fs::symlink_metadata(tmp.join(".kule-loopback/sitecustomize.py"))
            .unwrap()
            .is_file()
    );
    for dir in ["caches/cargo", "caches/pip"] {
        assert!(
            fs::symlink_metadata(tmp.join(dir)).unwrap().is_dir(),
            "{dir}"
        );
    }
}

/// An upstream that keeps all it is sent, for the test to look at, and
/// answers nothing.
fn keeping_upstream() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
    use std::io::Read;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let into = std::sync::Arc::clone(&kept);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let (mut stream, into) = (stream.unwrap(), std::sync::Arc::clone(&into));
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    into.lock().unwrap().extend_from_slice(&buf[..n]);
                }
            });
        }
    });
    (port, kept)
}

/// The first bytes a TLS client sends to `name`: its hello, made by rustls.
fn client_hello(name: &str) -> Vec<u8> {
    let roots = rustls::RootCertStore::empty();
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(name.to_owned()).unwrap();
    let mut conn = rustls::ClientConnection::new(std::sync::Arc::new(config), name).unwrap();
    let mut out = Vec::new();
    conn.write_tls(&mut out).unwrap();
    out
}

/// Opens a tunnel through `proxy` to `target`, sends `first` down it, and
/// returns what came back once the proxy is done with it.
fn tunnel_with(proxy: u16, target: &str, first: &[u8]) -> String {
    use std::io::Read;
    let mut s = TcpStream::connect(("127.0.0.1", proxy)).unwrap();
    write!(s, "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").unwrap();
    s.write_all(first).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut reply = Vec::new();
    let _ = s.read_to_end(&mut reply);
    String::from_utf8_lossy(&reply).into_owned()
}

fn kept_after(kept: &std::sync::Mutex<Vec<u8>>, wait: Duration) -> Vec<u8> {
    std::thread::sleep(wait);
    kept.lock().unwrap().clone()
}

/// A tunnel Kule does not look inside goes on only once its TLS hello asks
/// for the host it was opened to: one asking for another host behind the
/// same address (as a CDN serves many) is cut before a byte reaches it, and
/// the log says why.
#[test]
fn a_tunnel_whose_tls_asks_for_another_host_is_cut() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    let proxy = Proxy::start(&task, policy()).unwrap();

    // To an address: a hello that names a host could reach any behind it.
    let (up, kept) = keeping_upstream();
    let reply = tunnel_with(
        proxy.port(),
        &format!("127.0.0.1:{up}"),
        &client_hello("other.example"),
    );
    assert!(reply.contains("200 Connection established"), "{reply}");
    assert!(
        kept_after(&kept, Duration::from_millis(300)).is_empty(),
        "the hello reached the host"
    );
    let cut = loop {
        if let Some(c) = connections(&task)
            .unwrap()
            .into_iter()
            .find(|c| c.port == up)
        {
            break c;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(cut.outcome, "cut");
    assert!(
        cut.cut
            .as_deref()
            .unwrap_or_default()
            .contains("other.example"),
        "{cut:?}"
    );

    // One that names none passes, and reaches the host as it was sent.
    let (up, kept) = keeping_upstream();
    let hello = client_hello("127.0.0.1");
    tunnel_with(proxy.port(), &format!("127.0.0.1:{up}"), &hello);
    assert_eq!(kept_after(&kept, Duration::from_millis(300)), hello);
}

/// A plain request goes only to the host it was let through to: a second
/// one on the same connection for another host behind the address, or a
/// first whose Host says another, is answered by Kule and never reaches it.
#[test]
fn a_plain_request_for_another_host_never_reaches_it() {
    use std::io::Read;
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let task = Task::create(home.path(), ws.path()).unwrap();
    let proxy = Proxy::start(&task, policy()).unwrap();
    let answering = || {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let into = std::sync::Arc::clone(&kept);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if std::io::BufRead::read_line(&mut reader, &mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line.starts_with("GET") {
                        into.lock().unwrap().push(line.trim().to_owned());
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                    }
                }
            }
        });
        (port, kept)
    };
    let ask = |text: String| {
        let mut s = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        s.write_all(text.as_bytes()).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reply = String::new();
        let _ = s.read_to_string(&mut reply);
        reply
    };

    let (up, kept) = answering();
    let reply = ask(format!(
        "GET http://127.0.0.1:{up}/a HTTP/1.1\r\nHost: 127.0.0.1:{up}\r\n\r\n\
         GET http://other.example/b HTTP/1.1\r\nHost: other.example\r\n\r\n"
    ));
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(
        reply.contains("400 Bad Request") && reply.contains("other.example"),
        "{reply}"
    );
    assert_eq!(*kept.lock().unwrap(), ["GET /a HTTP/1.1"]);

    let (up, kept) = answering();
    let reply = ask(format!(
        "GET http://127.0.0.1:{up}/a HTTP/1.1\r\nHost: other.example\r\n\r\n"
    ));
    assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
    std::thread::sleep(Duration::from_millis(200));
    assert!(kept.lock().unwrap().is_empty());
}
