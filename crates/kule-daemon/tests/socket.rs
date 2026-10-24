//! The daemon over a real Unix socket: calls, notices, one daemon per home,
//! the idle exit and the line cap.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kule_daemon::{
    Call, Client, Hello, MAX_REQUEST, Notice, PROTOCOL, RunEvent, Service, TaskView, bind,
    new_token, run, serve, socket_path, token_path,
};
use kule_engine::{Policy, Proxy, Task};
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
    // Where Kule keeps its tasks stays its own business.
    assert!(!error.contains(home.path().to_str().unwrap()), "{error}");
}

/// Projects beside a folder go through the daemon as through the CLI: a
/// folder Kule does not hide as the person's own is refused, and nothing
/// is kept.
#[test]
fn a_folder_kule_does_not_hide_is_not_opened_through_the_daemon() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let client = Client::new(home.path());
    let (ws, beside) = (workspace(), TempDir::new().unwrap());
    let error = client
        .call::<kule_engine::Beside>(Call::AddBeside {
            workspace: ws.path().to_owned(),
            path: beside.path().to_owned(),
        })
        .unwrap_err();
    assert!(
        error.contains("not in a folder Kule hides as yours"),
        "{error}"
    );
    let kept: Vec<serde_json::Value> = client.call(Call::Besides).unwrap();
    assert!(kept.is_empty());
    let suggested: Vec<std::path::PathBuf> = client
        .call(Call::SuggestBeside {
            workspace: ws.path().to_owned(),
        })
        .unwrap();
    assert!(suggested.is_empty());
    assert!(!client.call::<bool>(Call::RemoveBeside { id: 1 }).unwrap());
}

/// The most a prompt may carry (every place it may name, at its longest,
/// and every picture of a region it may take) fits in one request.
#[test]
fn the_largest_prompt_fits_in_a_request() {
    use kule_view::refs::{Crop, MAX_CROP, MAX_CROPS, MAX_QUOTE, MAX_REFS, Reference};
    let region = Reference::Region {
        path: "p".repeat(200),
        x: 0,
        y: 0,
        w: 1,
        h: 1,
        width: 1,
        height: 1,
        crop: Some(Crop {
            mime: "image/jpeg".into(),
            data: "A".repeat(MAX_CROP.div_ceil(3) * 4),
        }),
    };
    let lines = Reference::Lines {
        path: "p".repeat(200),
        start: 1,
        end: 1,
        // Every character escaped in JSON.
        quote: "\u{1}".repeat(MAX_QUOTE),
    };
    let mut refs = vec![region; MAX_CROPS];
    refs.resize(MAX_REFS, lines);
    let request = kule_daemon::Request {
        id: u64::MAX,
        token: Some("t".repeat(64)),
        call: Call::StartRun {
            task: "t".repeat(40),
            agent: "a".repeat(40),
            prompt: "x".repeat(20_000),
            refs,
        },
    };
    let line = serde_json::to_string(&request).unwrap();
    assert!(line.len() < MAX_REQUEST, "{} bytes", line.len());
}

#[test]
fn the_preview_reads_a_file_of_the_copy_as_its_kind() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let client = Client::new(home.path());
    let ws = workspace();
    let task: TaskView = client
        .call(Call::CreateTask {
            workspace: ws.path().to_owned(),
        })
        .unwrap();
    let copy: std::path::PathBuf = client
        .call(Call::CopyOf {
            task: task.id.clone(),
        })
        .unwrap();
    fs::write(copy.join("report.md"), "# Q2\n").unwrap();
    let view: kule_view::View = client
        .call(Call::View {
            task: task.id.clone(),
            path: "report.md".into(),
        })
        .unwrap();
    assert_eq!(
        view,
        kule_view::View::Markdown {
            text: "# Q2\n".into(),
            cut: false,
            size: 5
        }
    );
    let error = client
        .call::<kule_view::View>(Call::View {
            task: task.id.clone(),
            path: "../a.txt".into(),
        })
        .unwrap_err();
    assert!(error.contains("without .."), "{error}");
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
        &[],
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
            remember: true,
        })
        .unwrap();
    // Gone from the questions, and what became of it told on the task's
    // events, in either order.
    let mut heard = (false, false);
    while heard != (true, true) {
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Notice::Questions { waiting } => {
                assert!(waiting.is_empty());
                heard.0 = true;
            }
            Notice::Run {
                event: RunEvent::Answered { answer, .. },
                ..
            } => {
                assert_eq!(answer, kule_engine::Answer::Denied);
                heard.1 = true;
            }
            other => panic!("expected questions, heard {other:?}"),
        }
    }
    // Denied: nothing to remember, whatever was asked.
    let rules: Vec<kule_engine::Remembered> = client.call(Call::Rules).unwrap();
    assert!(rules.is_empty());

    // Approved and remembered: kept for the task's folder, and the next
    // proxy on that folder lets it through without asking.
    let mut again = ProxyClient::connect(proxy.port());
    again.ask("example.invalid:8443");
    let asked = loop {
        match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
            Notice::Questions { waiting } if !waiting.is_empty() => break waiting,
            // What became of the last question, told on its task's events.
            Notice::Questions { .. } | Notice::Run { .. } => {}
            other => panic!("expected questions, heard {other:?}"),
        }
    };
    client
        .call::<()>(Call::Answer {
            id: asked[0].id,
            approve: true,
            remember: true,
        })
        .unwrap();
    let rules: Vec<kule_engine::Remembered> = client.call(Call::Rules).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].allowance,
        kule_engine::Allowance::Connect {
            host: "example.invalid".into(),
            port: 8443
        }
    );
    assert_eq!(rules[0].workspace.as_deref(), Some(task.workspace()));
    let policy = Policy::load(Some(home.path()), task.workspace()).unwrap();
    assert_eq!(
        policy.judge_connection("example.invalid", 8443).verdict,
        kule_engine::Verdict::Allow
    );
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

/// A connection that never says who it is goes after a while, and a crowd
/// of them leaves the daemon answering the app.
#[test]
fn connections_that_say_nothing_are_let_go() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let silent: Vec<UnixStream> = (0..100)
        .map(|_| UnixStream::connect(socket_path(home.path())).unwrap())
        .collect();
    let mut first = silent.into_iter().next().unwrap();
    // Refused where the daemon already ended it.
    let _ = first.set_read_timeout(Some(Duration::from_secs(30)));
    let started = std::time::Instant::now();
    let mut reply = Vec::new();
    let _ = first.read_to_end(&mut reply);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "a silent connection was kept {:?}",
        started.elapsed()
    );
    // Once they are gone the daemon answers whoever carries the token.
    std::thread::sleep(Duration::from_secs(1));
    let tasks: Vec<TaskView> = Client::new(home.path()).call(Call::Tasks).unwrap();
    assert!(tasks.is_empty());
}

/// Connections that have not shown the token take only a few of the
/// daemon's places: whoever carries it is answered at once, however many
/// wait.
#[test]
fn connections_not_yet_proven_leave_room_for_the_app() {
    use std::io::BufRead;
    let home = TempDir::new().unwrap();
    daemon(home.path());
    // A client that showed the token before the crowd came keeps its place.
    let token = fs::read_to_string(token_path(home.path())).unwrap();
    let request = format!("{{\"id\":1,\"token\":\"{token}\",\"method\":\"tasks\"}}\n");
    let mut shown = UnixStream::connect(socket_path(home.path())).unwrap();
    shown
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut answers = std::io::BufReader::new(shown.try_clone().unwrap());
    let mut answer = String::new();
    shown.write_all(request.as_bytes()).unwrap();
    answers.read_line(&mut answer).unwrap();
    assert!(answer.contains("\"result\""), "{answer}");

    let _silent: Vec<UnixStream> = (0..100)
        .map(|_| UnixStream::connect(socket_path(home.path())).unwrap())
        .collect();
    // Taken in by the daemon before the app comes.
    std::thread::sleep(Duration::from_millis(300));
    let started = std::time::Instant::now();
    let tasks: Vec<TaskView> = Client::new(home.path()).call(Call::Tasks).unwrap();
    assert!(tasks.is_empty());
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    answer.clear();
    shown.write_all(request.as_bytes()).unwrap();
    answers.read_line(&mut answer).unwrap();
    assert!(answer.contains("\"result\""), "{answer}");
}

/// Neither a line given a byte at a time nor lines that are not requests
/// keep a connection that has not shown the token.
#[test]
fn connections_that_never_show_the_token_are_let_go() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let slow = |what: &'static str| {
        let mut stream = UnixStream::connect(socket_path(home.path())).unwrap();
        let mut reader = stream.try_clone().unwrap();
        let started = std::time::Instant::now();
        let writer = std::thread::spawn(move || {
            // Each well within the old per-read timeout, for 15 s.
            for _ in 0..15 {
                if stream.write_all(what.as_bytes()).is_err() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
        reader
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut reply = Vec::new();
        let _ = reader.read_to_end(&mut reply);
        let kept = started.elapsed();
        writer.join().unwrap();
        assert!(kept < Duration::from_secs(12), "{what:?} was kept {kept:?}");
    };
    slow("{");
    slow("not json\n");
    // The daemon answers whoever carries the token.
    let tasks: Vec<TaskView> = Client::new(home.path()).call(Call::Tasks).unwrap();
    assert!(tasks.is_empty());
}

#[test]
fn the_socket_is_for_its_owner_only() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let mode = fs::metadata(socket_path(home.path()))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");
}

#[test]
fn a_daemon_says_which_protocol_it_speaks() {
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let hello: Hello = Client::new(home.path()).call(Call::Hello).unwrap();
    assert_eq!(hello.protocol, PROTOCOL);
}

#[test]
fn shutdown_waits_until_nothing_is_going_on() {
    let home = TempDir::new().unwrap();
    let path = home.path().to_owned();
    let service_home = path.clone();
    let socket = socket_path(&path);
    let listener = bind(&socket).unwrap();
    let service = Arc::new(Service::new(service_home));
    let served = service.clone();
    let token = new_token(&path).unwrap();
    let daemon = std::thread::spawn(move || {
        serve(listener, served, &socket, Duration::from_secs(600), token)
    });
    let client = Client::new(home.path());
    let ws = workspace();
    let view: TaskView = client
        .call(Call::CreateTask {
            workspace: ws.path().to_owned(),
        })
        .unwrap();

    // A question waiting keeps it up.
    let task = Task::open(home.path(), &view.id).unwrap();
    let proxy = Proxy::start_asking(
        &task,
        Policy::builtin(),
        service.approvals(),
        Duration::from_secs(60),
        &[],
    )
    .unwrap();
    ProxyClient::connect(proxy.port()).ask("example.invalid:443");
    let start = Instant::now();
    while service.questions().is_empty() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let refused = client.call::<()>(Call::Shutdown).unwrap_err();
    assert!(refused.contains("question is waiting"), "{refused}");
    assert!(client.is_running());

    drop(proxy);
    client.call::<()>(Call::Shutdown).unwrap();
    daemon.join().unwrap();
    assert!(!client.is_running());
}

/// Answers like a daemon of another protocol, and stops when asked.
fn foreign_daemon(home: &Path) -> std::thread::JoinHandle<()> {
    use std::io::{BufRead, BufReader};
    let socket = socket_path(home);
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            if line.contains("\"hello\"") {
                let _ = stream
                    .write_all(b"{\"id\":1,\"result\":{\"protocol\":999,\"version\":\"old\"}}\n");
            } else if line.contains("\"shutdown\"") {
                let _ = stream.write_all(b"{\"id\":1,\"result\":null}\n");
                drop(listener);
                fs::remove_file(&socket).unwrap();
                return;
            }
        }
    })
}

#[test]
fn a_daemon_of_another_protocol_is_replaced() {
    let home = TempDir::new().unwrap();
    let old = foreign_daemon(home.path());
    let client = Client::new(home.path());
    assert!(client.is_running());

    let path = home.path().to_owned();
    client
        .ensure_running(|| {
            std::thread::spawn(move || run(&path, Duration::from_secs(600)));
            Ok(())
        })
        .unwrap();
    old.join().unwrap();
    let hello: Hello = client.call(Call::Hello).unwrap();
    assert_eq!(hello.protocol, PROTOCOL);
}

#[test]
fn a_request_without_the_token_is_refused() {
    use std::io::{BufRead, BufReader};
    let home = TempDir::new().unwrap();
    daemon(home.path());
    let token = fs::read_to_string(token_path(home.path())).unwrap();
    let mode = fs::metadata(token_path(home.path()))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "{mode:o}");

    let ask = |line: &str| {
        let mut stream = UnixStream::connect(socket_path(home.path())).unwrap();
        stream.write_all(line.as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(stream).read_line(&mut reply).unwrap();
        reply
    };
    for line in [
        "{\"id\":1,\"method\":\"tasks\"}\n".to_owned(),
        "{\"id\":1,\"token\":\"0000\",\"method\":\"tasks\"}\n".to_owned(),
        format!(
            "{{\"id\":1,\"token\":\"{}\",\"method\":\"tasks\"}}\n",
            "f".repeat(token.len())
        ),
    ] {
        let reply = ask(&line);
        assert!(reply.contains("not authorized"), "{line} -> {reply}");
    }
    let reply = ask(&format!(
        "{{\"id\":1,\"token\":\"{token}\",\"method\":\"tasks\"}}\n"
    ));
    assert!(reply.contains("\"result\":[]"), "{reply}");
}

#[test]
fn extensions_are_installed_and_approved_over_the_socket_and_subscribers_hear_it() {
    let home = TempDir::new().unwrap();
    let service = daemon(home.path());
    let notices = service.subscribe();
    let client = Client::new(home.path());

    let src = TempDir::new().unwrap();
    fs::write(
        src.path().join("kule-extension.json"),
        r#"{ "kule_extension": 1, "id": "acme.notes", "name": "Notes", "version": "1.0.0",
             "publisher": "acme", "permissions": ["storage"],
             "contributes": { "configuration": [
                 { "key": "level", "title": "Level", "type": "enum", "values": ["info", "warn"], "default": "warn" } ] } }"#,
    )
    .unwrap();
    let file = src.path().join("notes.kulext");
    fs::write(&file, kule_ext::pack(src.path()).unwrap()).unwrap();

    let listed: kule_ext::Listed = client.call(Call::InstallExtension { path: file }).unwrap();
    assert_eq!(listed.held.as_deref(), Some("not approved yet"));
    assert!(matches!(
        notices.recv_timeout(Duration::from_secs(5)).unwrap(),
        Notice::Extensions
    ));

    let stale = client
        .call::<kule_ext::Listed>(Call::ApproveExtension {
            id: "acme.notes".into(),
            digest: "not-what-you-saw".into(),
        })
        .unwrap_err();
    assert!(stale.contains("changed since you looked"), "{stale}");
    let listed: kule_ext::Listed = client
        .call(Call::ApproveExtension {
            id: "acme.notes".into(),
            digest: listed.digest,
        })
        .unwrap();
    assert!(listed.runs());

    client
        .call::<()>(Call::SetExtensionConfig {
            id: "acme.notes".into(),
            key: "level".into(),
            value: serde_json::json!("info"),
        })
        .unwrap();
    let config: serde_json::Value = client
        .call(Call::ExtensionConfig {
            id: "acme.notes".into(),
        })
        .unwrap();
    assert_eq!(config, serde_json::json!({ "level": "info" }));
    client
        .call::<()>(Call::SetExtensionStorage {
            id: "acme.notes".into(),
            value: serde_json::json!([1, 2]),
        })
        .unwrap();
    let kept: serde_json::Value = client
        .call(Call::ExtensionStorage {
            id: "acme.notes".into(),
        })
        .unwrap();
    assert_eq!(kept, serde_json::json!([1, 2]));

    client
        .call::<()>(Call::RemoveExtension {
            id: "acme.notes".into(),
        })
        .unwrap();
    let all: Vec<kule_ext::Listed> = client.call(Call::Extensions).unwrap();
    assert!(all.is_empty());
}
