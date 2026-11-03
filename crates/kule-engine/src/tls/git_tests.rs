//! A git host looked into: a fetch passes once the host is approved, and a
//! push, or any other write, waits for a person on its own. Raw requests
//! to a host that echoes what it gets, and a real git against git's own
//! `http-backend`, both through the real interceptor.

use std::collections::BTreeSet;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempfile::TempDir;

use super::tests::{HOST, Hold, ask_until, echo_host};
use super::*;

/// An interceptor that reads `HOST` as a git host, holding no secret.
fn git_interceptor(roots: RootCertStore) -> Interceptor {
    let (home, ws, dir) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
    Interceptor::new(vault, roots, BTreeSet::new(), [HOST.to_owned()].into()).unwrap()
}

/// A person who gives `answer` (`None`: nobody is there), and what was
/// put to them.
fn person(answer: Option<Answer>) -> (Hold, Arc<Mutex<Vec<HeldRequest>>>) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&asked);
    let hold = move |held: &HeldRequest| {
        into.lock().unwrap().push(held.clone());
        answer
    };
    (Box::new(hold), asked)
}

fn push_to(repo: &str) -> HeldRequest {
    HeldRequest::Push {
        repo: repo.to_owned(),
    }
}

fn get(target: &str) -> String {
    format!("GET {target} HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n")
}

fn post(target: &str, kind: &str, body: &str) -> String {
    format!(
        "POST {target} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

const PACK: &str = "application/x-git-receive-pack-request";

#[test]
fn a_fetch_passes_and_a_push_waits_for_a_person_for_its_repository() {
    let host = echo_host();
    let interceptor = Arc::new(git_interceptor(host.roots.clone()));
    let seen = |line: &str| {
        host.lines
            .lock()
            .unwrap()
            .iter()
            .any(|l| l.starts_with(line))
    };

    // A fetch: discovery, then the negotiation. Nobody is asked.
    let fetch = format!(
        "GET /me/proj.git/info/refs?service=git-upload-pack HTTP/1.1\r\nHost: {HOST}\r\n\r\n{}",
        post(
            "/me/proj.git/git-upload-pack",
            "application/x-git-upload-pack-request",
            "0000"
        )
    );
    let (hold, asked) = person(Some(Answer::Denied));
    let (reply, served, _) = ask_until(&interceptor, host.port, HOST, &fetch, None, false, hold);
    assert_eq!(reply.unwrap().matches("HTTP/1.1 200 OK").count(), 2);
    assert_eq!(served.unwrap().refused, None);
    assert!(asked.lock().unwrap().is_empty());

    // A push, whether it asks for refs first or posts its pack straight
    // away: held, and denied, it never reaches the host.
    for (request, line) in [
        (
            get("/me/proj.git/info/refs?service=git-receive-pack"),
            "GET /me/proj.git/info/refs?service=git-receive-pack",
        ),
        (
            post("/me/proj.git/git-receive-pack", PACK, "0000"),
            "POST /me/proj.git/git-receive-pack",
        ),
    ] {
        let (hold, asked) = person(Some(Answer::Denied));
        let (reply, served, _) =
            ask_until(&interceptor, host.port, HOST, &request, None, false, hold);
        let reply = reply.unwrap();
        assert!(reply.starts_with("HTTP/1.1 403 Forbidden"), "{reply}");
        assert!(
            reply.contains("a push to api.test.example/me/proj waits for a person, who denied it"),
            "{reply}"
        );
        assert_eq!(served.unwrap().refused, Some(REFUSED_HELD));
        assert_eq!(*asked.lock().unwrap(), [push_to("me/proj")]);
        assert!(!seen(line), "{line} reached the host");
    }

    // Approved, it goes on.
    let (hold, asked) = person(Some(Answer::Approved));
    let push = post("/me/proj.git/git-receive-pack", PACK, "0000");
    let (reply, served, _) = ask_until(&interceptor, host.port, HOST, &push, None, false, hold);
    assert!(reply.unwrap().starts_with("HTTP/1.1 200 OK"));
    assert_eq!(served.unwrap().refused, None);
    assert_eq!(*asked.lock().unwrap(), [push_to("me/proj")]);
    assert!(seen("POST /me/proj.git/git-receive-pack"));
}

#[test]
fn with_nobody_to_ask_a_push_is_refused_and_says_so() {
    let host = echo_host();
    let interceptor = Arc::new(git_interceptor(host.roots.clone()));
    let (hold, _) = person(None);
    let push = post("/me/proj/git-receive-pack", PACK, "0000");
    let (reply, served, _) = ask_until(&interceptor, host.port, HOST, &push, None, false, hold);
    let reply = reply.unwrap();
    assert!(
        reply.contains("waits for a person, and no one is there to ask"),
        "{reply}"
    );
    assert_eq!(served.unwrap().refused, Some(REFUSED_HELD));
    assert!(host.lines.lock().unwrap().is_empty());
}

#[test]
fn reads_pass_and_any_other_write_waits_for_a_person_for_the_host() {
    let host = echo_host();
    let interceptor = Arc::new(git_interceptor(host.roots.clone()));
    let (hold, asked) = person(Some(Answer::Denied));
    let download = get("/me/proj/releases/download/v1/tool.tar.gz");
    let (reply, _, _) = ask_until(&interceptor, host.port, HOST, &download, None, false, hold);
    assert!(reply.unwrap().starts_with("HTTP/1.1 200 OK"));
    assert!(asked.lock().unwrap().is_empty());

    for answer in [Answer::Denied, Answer::TimedOut] {
        let (hold, asked) = person(Some(answer));
        let write = post(
            "/api/v4/projects/1/repository/files/x",
            "application/json",
            "{}",
        );
        let (reply, served, _) =
            ask_until(&interceptor, host.port, HOST, &write, None, false, hold);
        let reply = reply.unwrap();
        assert!(reply.starts_with("HTTP/1.1 403 Forbidden"), "{reply}");
        assert!(
            reply.contains("a write to api.test.example (not a fetch) waits for a person"),
            "{reply}"
        );
        if answer == Answer::TimedOut {
            assert!(reply.contains("skip this step"), "{reply}");
        }
        assert_eq!(served.unwrap().refused, Some(REFUSED_HELD));
        assert_eq!(*asked.lock().unwrap(), [HeldRequest::Write]);
    }
    assert_eq!(
        *host.lines.lock().unwrap(),
        ["GET /me/proj/releases/download/v1/tool.tar.gz HTTP/1.1"]
    );
}

#[test]
fn an_lfs_download_passes_and_an_upload_is_a_push() {
    let host = echo_host();
    let interceptor = Arc::new(git_interceptor(host.roots.clone()));
    let batch = "/me/proj.git/info/lfs/objects/batch";
    let kind = "application/vnd.git-lfs+json";

    let (hold, asked) = person(Some(Answer::Denied));
    let download = post(batch, kind, r#"{"operation":"download","objects":[]}"#);
    let (reply, _, _) = ask_until(&interceptor, host.port, HOST, &download, None, false, hold);
    let reply = reply.unwrap();
    // The body Kule read whole goes on as it came.
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    assert!(
        reply.contains(r#"{"operation":"download","objects":[]}"#),
        "{reply}"
    );
    assert!(asked.lock().unwrap().is_empty());

    let (hold, asked) = person(Some(Answer::Denied));
    let upload = post(batch, kind, r#"{"operation":"upload","objects":[]}"#);
    let (reply, served, _) = ask_until(&interceptor, host.port, HOST, &upload, None, false, hold);
    assert!(reply.unwrap().starts_with("HTTP/1.1 403 Forbidden"));
    assert_eq!(served.unwrap().refused, Some(REFUSED_HELD));
    assert_eq!(*asked.lock().unwrap(), [push_to("me/proj")]);

    // Read two ways, or chunked so it cannot be read whole: refused,
    // without asking.
    let chunked = format!(
        "POST {batch} HTTP/1.1\r\nHost: {HOST}\r\nContent-Type: {kind}\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{{}}\r\n0\r\n\r\n"
    );
    for request in [
        post(
            batch,
            kind,
            r#"{"operation":"download","operation":"upload"}"#,
        ),
        chunked,
    ] {
        let (hold, asked) = person(Some(Answer::Approved));
        let (reply, served, _) =
            ask_until(&interceptor, host.port, HOST, &request, None, false, hold);
        assert!(reply.unwrap().starts_with("HTTP/1.1 403 Forbidden"));
        assert_eq!(served.unwrap().refused, Some(REFUSED_GIT));
        assert!(asked.lock().unwrap().is_empty());
    }
    assert_eq!(host.lines.lock().unwrap().len(), 1);
}

#[test]
fn a_request_read_two_ways_is_refused_without_asking() {
    let host = echo_host();
    let interceptor = Arc::new(git_interceptor(host.roots.clone()));
    for request in [
        get("/me/proj/info/refs?service=git%2Dreceive-pack"),
        get("/me/proj/info/refs?service=git-receive-pack&service=git-upload-pack"),
        post("/me/../them/proj/git-receive-pack", PACK, "0000"),
    ] {
        let (hold, asked) = person(Some(Answer::Approved));
        let (reply, served, _) =
            ask_until(&interceptor, host.port, HOST, &request, None, false, hold);
        assert!(reply.unwrap().starts_with("HTTP/1.1 403 Forbidden"));
        assert_eq!(served.unwrap().refused, Some(REFUSED_GIT));
        assert!(asked.lock().unwrap().is_empty());
    }
    assert!(host.lines.lock().unwrap().is_empty());
}

/// Someone else's credential on a push to a host Kule fills for is refused
/// as on any request there, without putting the push to the person first.
#[test]
fn a_push_with_a_credential_kule_does_not_hold_is_refused_unasked() {
    let host = echo_host();
    let (home, ws, dir) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    crate::secrets::add_secret(home.path(), "GH", "real-value", &[HOST.to_owned()], None).unwrap();
    let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
    let interceptor = Arc::new(
        Interceptor::new(
            vault,
            host.roots.clone(),
            BTreeSet::new(),
            [HOST.to_owned()].into(),
        )
        .unwrap(),
    );
    let push = format!(
        "POST /me/proj/git-receive-pack HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer someone-elses\r\nContent-Type: {PACK}\r\nContent-Length: 4\r\nConnection: close\r\n\r\n0000"
    );
    let (hold, asked) = person(Some(Answer::Approved));
    let (reply, served, _) = ask_until(&interceptor, host.port, HOST, &push, None, false, hold);
    assert!(reply.unwrap().starts_with("HTTP/1.1 403 Forbidden"));
    assert_eq!(served.unwrap().refused, Some(REFUSED_CREDENTIAL));
    assert!(asked.lock().unwrap().is_empty());
}

/// A request that waited for a person goes to the host on a connection
/// of its own, made once the answer came: the one opened before may be
/// closed by then. One answered at once goes on the first.
#[test]
fn a_request_that_waited_goes_on_a_fresh_connection() {
    let host = echo_host();
    let mut interceptor = git_interceptor(host.roots.clone());
    let port = host.port;
    interceptor.dial = Box::new(move |_| TcpStream::connect(("127.0.0.1", port)));
    let interceptor = Arc::new(interceptor);
    let push = post("/me/proj/git-receive-pack", PACK, "0000");

    let (reply, _, _) = ask_until(
        &interceptor,
        host.port,
        HOST,
        &push,
        None,
        false,
        Box::new(|_| Some(Answer::Approved)),
    );
    assert!(reply.unwrap().starts_with("HTTP/1.1 200 OK"));
    assert_eq!(
        host.connections.load(std::sync::atomic::Ordering::SeqCst),
        1
    );

    let slow = Box::new(|_: &HeldRequest| {
        std::thread::sleep(WAITED + Duration::from_millis(100));
        Some(Answer::Approved)
    });
    let (reply, served, _) = ask_until(&interceptor, host.port, HOST, &push, None, false, slow);
    assert!(reply.unwrap().starts_with("HTTP/1.1 200 OK"));
    assert_eq!(served.unwrap().refused, None);
    // The tunnel's own, then the fresh one.
    assert_eq!(
        host.connections.load(std::sync::atomic::Ordering::SeqCst),
        3
    );
}

/// The proxy's own connection reaches public addresses alone, as a
/// tunnel's does.
#[test]
fn the_proxy_dials_no_address_on_this_computer() {
    let error = dial_public("localhost").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::AddrNotAvailable, "{error}");
}

// ------------------------------------------------------------ a real git

/// A real git, through the interceptor, against git's own `http-backend`.
#[cfg(unix)]
mod real_git {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use super::*;

    /// A git host for `HOST` over HTTPS, serving the bare repositories under
    /// `root` through git's own `http-backend`; its port and the root to
    /// trust it by.
    fn git_host(root: std::path::PathBuf) -> (u16, RootCertStore) {
        let root_key = KeyPair::generate().unwrap();
        let mut ca = CertificateParams::default();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let ca_cert = ca.self_signed(&root_key).unwrap();
        let issuer = Issuer::new(ca, root_key);
        let key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec![HOST.to_owned()])
            .unwrap()
            .signed_by(&key, &issuer)
            .unwrap();
        let config = Arc::new(
            ServerConfig::builder_with_provider(provider())
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![leaf.der().clone()],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
                )
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let (config, root) = (Arc::clone(&config), root.clone());
                std::thread::spawn(move || {
                    let conn = ServerConnection::new(config).unwrap();
                    let mut tls = Conn::new(StreamOwned::new(conn, stream.unwrap()));
                    while let Ok(Some(head)) = tls.head() {
                        let head = String::from_utf8_lossy(&head).into_owned();
                        let request = Request::of(&head).unwrap();
                        let mut body = Vec::new();
                        if relay(request.framing, &mut tls, &mut body, false).is_err() {
                            break;
                        }
                        if request.framing == Framing::Chunked {
                            body = unchunk(&body);
                        }
                        let reply = backend(&root, &head, &request, &body);
                        if tls.stream.write_all(&reply).is_err() || request.close {
                            break;
                        }
                        let _ = tls.stream.flush();
                    }
                    tls.stream.conn.send_close_notify();
                    let _ = tls.stream.flush();
                });
            }
        });
        let mut roots = RootCertStore::empty();
        roots.add(ca_cert.der().clone()).unwrap();
        (port, roots)
    }

    /// A chunked body without its chunks' framing.
    fn unchunk(framed: &[u8]) -> Vec<u8> {
        let mut from = Conn::new(Cursor::new(framed.to_vec()));
        let mut out = Vec::new();
        loop {
            let line = from.line().unwrap();
            let size = chunk_size(&line).unwrap();
            if size == 0 {
                return out;
            }
            let mut chunk = vec![0u8; size as usize];
            from.read_exact(&mut chunk).unwrap();
            out.extend_from_slice(&chunk);
            from.read_exact(&mut [0u8; 2]).unwrap();
        }
    }

    /// One request answered by `git http-backend`, as an HTTP response.
    fn backend(root: &std::path::Path, head: &str, request: &Request, body: &[u8]) -> Vec<u8> {
        use std::process::{Command, Stdio};
        let (path, query) = request
            .target
            .split_once('?')
            .unwrap_or((&request.target, ""));
        let header = |name: &str| headers(head, name).next().unwrap_or_default().to_owned();
        let mut child = Command::new("git")
            .arg("http-backend")
            .env("GIT_PROJECT_ROOT", root)
            .env("GIT_HTTP_EXPORT_ALL", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("REQUEST_METHOD", &request.method)
            .env("PATH_INFO", path)
            .env("QUERY_STRING", query)
            .env("CONTENT_TYPE", header("content-type"))
            .env("CONTENT_LENGTH", body.len().to_string())
            .env("HTTP_CONTENT_ENCODING", header("content-encoding"))
            .env("GIT_PROTOCOL", header("git-protocol"))
            .env("REMOTE_USER", "test")
            .env("REMOTE_ADDR", "127.0.0.1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(body).unwrap();
        let output = child.wait_with_output().unwrap();
        let out = output.stdout;
        let split = out
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|i| (i, 4))
            .or_else(|| out.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)))
            .unwrap();
        let cgi = String::from_utf8_lossy(&out[..split.0]).into_owned();
        let content = &out[split.0 + split.1..];
        let mut status = "200 OK".to_owned();
        let mut reply = String::new();
        for line in cgi.lines() {
            match line.split_once(':') {
                Some((name, value)) if name.eq_ignore_ascii_case("status") => {
                    status = value.trim().to_owned()
                }
                Some(_) => reply.push_str(&format!("{line}\r\n")),
                None => {}
            }
        }
        let mut response = format!(
            "HTTP/1.1 {status}\r\n{reply}Content-Length: {}\r\n\r\n",
            content.len()
        )
        .into_bytes();
        response.extend_from_slice(content);
        response
    }

    /// A proxy for git to speak to: each `CONNECT` it is sent goes to the git
    /// host at `up` through `interceptor`, holding as `answer` says.
    fn front(
        interceptor: Arc<Interceptor>,
        up: u16,
        answer: Arc<Mutex<Option<Answer>>>,
        asked: Arc<Mutex<Vec<HeldRequest>>>,
    ) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut client = stream.unwrap();
                let (interceptor, answer, asked) = (
                    Arc::clone(&interceptor),
                    Arc::clone(&answer),
                    Arc::clone(&asked),
                );
                std::thread::spawn(move || {
                    let mut head = Vec::new();
                    let mut byte = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        client.read_exact(&mut byte).unwrap();
                        head.push(byte[0]);
                    }
                    assert!(head.starts_with(format!("CONNECT {HOST}:443 ").as_bytes()));
                    client
                        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                        .unwrap();
                    let tunnel = lonca_net::Tunnel {
                        client,
                        upstream: TcpStream::connect(("127.0.0.1", up)).unwrap(),
                        early: Vec::new(),
                    };
                    let mut hold = |held: &HeldRequest| {
                        asked.lock().unwrap().push(held.clone());
                        *answer.lock().unwrap()
                    };
                    let _ = interceptor.serve_noting(HOST, tunnel, &mut BTreeSet::new(), &mut hold);
                });
            }
        });
        port
    }

    /// A real `git clone`, `push` and `fetch` through the interceptor: the
    /// clone and the fetch ask nobody; the push waits for a person, for its
    /// repository, and reaches the repository only once approved.
    #[test]
    fn a_real_git_push_waits_for_a_person_and_its_fetch_does_not() {
        use std::path::Path;
        use std::process::Command;
        let dir = TempDir::new().unwrap();
        let (root, home) = (dir.path().join("root"), dir.path().join("home"));
        std::fs::create_dir_all(root.join("me")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let bare = root.join("me/proj.git");
        let git = |at: &Path, args: &[&str], proxy: Option<(u16, &Path)>| {
            let mut command = Command::new("git");
            command
                .current_dir(at)
                .env("HOME", &home)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
                .env("GIT_TERMINAL_PROMPT", "0")
                .env_remove("GIT_SSL_NO_VERIFY")
                .env_remove("GIT_SSL_CAINFO")
                .env_remove("NO_PROXY")
                .env_remove("no_proxy")
                .args([
                    "-c",
                    "user.name=Kule Test",
                    "-c",
                    "user.email=test@kule.invalid",
                    "-c",
                    "init.defaultBranch=main",
                ]);
            if let Some((port, ca)) = proxy {
                command.args([
                    "-c".to_owned(),
                    format!("http.proxy=http://127.0.0.1:{port}"),
                    "-c".to_owned(),
                    format!("http.sslCAInfo={}", ca.display()),
                ]);
            }
            command.args(args).output().unwrap()
        };
        let ok = |out: std::process::Output| {
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        ok(git(
            dir.path(),
            &["init", "--bare", bare.to_str().unwrap()],
            None,
        ));
        let seed = dir.path().join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        ok(git(&seed, &["init"], None));
        std::fs::write(seed.join("a.txt"), "a\n").unwrap();
        ok(git(&seed, &["add", "."], None));
        ok(git(&seed, &["commit", "-m", "a"], None));
        ok(git(&seed, &["push", bare.to_str().unwrap(), "main"], None));
        let first = ok(git(&bare, &["rev-parse", "main"], None));

        let (up, roots) = git_host(root.clone());
        let interceptor = Arc::new(git_interceptor(roots));
        let ca = dir.path().join("kule-ca.pem");
        std::fs::write(&ca, interceptor.ca_pem()).unwrap();
        let answer = Arc::new(Mutex::new(Some(Answer::Denied)));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let port = front(interceptor, up, Arc::clone(&answer), Arc::clone(&asked));
        let via = Some((port, ca.as_path()));
        let url = format!("https://{HOST}/me/proj.git");

        ok(git(dir.path(), &["clone", &url, "work"], via));
        assert!(asked.lock().unwrap().is_empty(), "a clone asked");
        let work = dir.path().join("work");
        std::fs::write(work.join("b.txt"), "b\n").unwrap();
        ok(git(&work, &["add", "."], None));
        ok(git(&work, &["commit", "-m", "b"], None));
        let second = ok(git(&work, &["rev-parse", "HEAD"], None));

        // Denied: the push fails, and the repository is as it was.
        let denied = git(&work, &["push", "origin", "main"], via);
        assert!(!denied.status.success());
        assert_eq!(ok(git(&bare, &["rev-parse", "main"], None)), first);
        assert!(!asked.lock().unwrap().is_empty());
        assert!(
            asked
                .lock()
                .unwrap()
                .iter()
                .all(|h| *h == push_to("me/proj"))
        );

        // Approved: it lands.
        *answer.lock().unwrap() = Some(Answer::Approved);
        ok(git(&work, &["push", "origin", "main"], via));
        assert_eq!(ok(git(&bare, &["rev-parse", "main"], None)), second);

        // A fetch afterwards asks nobody.
        *answer.lock().unwrap() = Some(Answer::Denied);
        let before = asked.lock().unwrap().len();
        ok(git(&work, &["fetch", "origin"], via));
        assert_eq!(asked.lock().unwrap().len(), before, "a fetch asked");
    }
}
