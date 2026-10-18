//! Serves extensions' files to the window's frames over loopback HTTP.
//!
//! Why not a custom URI scheme: Tauri treats every registered scheme as the
//! app's own origin, and WebKit hands the IPC bridge to every frame, so a
//! frame on such a scheme would stand one leaked invoke key away from every
//! command. A frame on `http://127.0.0.1` is a remote origin to Tauri, and
//! remote origins reach no command at all (no `remote` capability exists).
//! Checked in the real app (2026-10-18): a frame from here, handed the real
//! invoke key, still could not run a command.
//!
//! The server answers GET only, on 127.0.0.1, under a random key in the path,
//! to requests whose Host is exactly its own address. It serves what a
//! [`Files`] hands it and nothing else, each answer with a content policy
//! that keeps the extension's page to its own files and off the network.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// The longest request head read.
const MAX_HEAD: usize = 8 * 1024;
/// Connections served at once; more wait for the kernel's backlog.
const MAX_OPEN: usize = 32;
/// How long a client may take to send its whole request.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// What an extension's page may not use: devices, the clipboard, payment,
/// fullscreen, and the rest it has no business with.
const FEATURES: &str = "camera=(), microphone=(), geolocation=(), display-capture=(), \
    clipboard-read=(), clipboard-write=(), fullscreen=(), payment=(), usb=(), \
    serial=(), bluetooth=(), midi=(), screen-wake-lock=(), web-share=()";

/// Where the files come from.
pub trait Files: Send + Sync + 'static {
    /// The bytes and media type of `path` inside extension `ext`, or None.
    /// `ext` and `path` are already plain (see [`plain_path`]).
    fn get(&self, ext: &str, path: &str) -> Option<(Vec<u8>, &'static str)>;
}

/// A running server.
pub struct Server {
    port: u16,
    key: String,
}

impl Server {
    /// What every extension's files start with: `<prefix><ext>/<path>`.
    pub fn prefix(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.port, self.key)
    }

    /// The one origin the window may frame: this server's.
    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// The window's content policy with frames allowed from `origin` alone, in
/// place of the `frame-src 'none'` the config carries. A frame can navigate
/// itself; with any other port allowed it could carry what it read to any
/// server on the computer, or send GETs to one, without the `net`
/// permission.
pub fn frame_policy(csp: &str, origin: Option<&str>) -> String {
    match origin {
        Some(origin) => csp.replace("frame-src 'none'", &format!("frame-src {origin}")),
        None => csp.to_owned(),
    }
}

fn base(port: u16, key: &str, ext: &str) -> String {
    format!("http://127.0.0.1:{port}/{key}/{ext}/")
}

/// Starts serving on a free loopback port.
pub fn start(files: Arc<dyn Files>) -> io::Result<Server> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let key = random_key()?;
    let server = Server {
        port,
        key: key.clone(),
    };
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            if open.load(Ordering::SeqCst) >= MAX_OPEN {
                let _ = stream.shutdown(Shutdown::Both);
                continue;
            }
            open.fetch_add(1, Ordering::SeqCst);
            let (files, key, open) = (files.clone(), key.clone(), open.clone());
            std::thread::spawn(move || {
                let _ = serve(stream, port, &key, files.as_ref());
                open.fetch_sub(1, Ordering::SeqCst);
            });
        }
    });
    Ok(server)
}

fn random_key() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Reads that give up once `until` passes, however slowly the bytes come:
/// a byte every few seconds would otherwise hold a connection for good.
struct Deadline {
    stream: TcpStream,
    until: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        self.stream.set_read_timeout(Some(left))?;
        self.stream.read(buf)
    }
}

fn serve(stream: TcpStream, port: u16, key: &str, files: &dyn Files) -> io::Result<()> {
    serve_within(stream, port, key, files, READ_TIMEOUT)
}

/// Answers one request whose head arrives whole within `time`.
fn serve_within(
    stream: TcpStream,
    port: u16,
    key: &str,
    files: &dyn Files,
    time: Duration,
) -> io::Result<()> {
    stream.set_write_timeout(Some(READ_TIMEOUT))?;
    let deadline = Deadline {
        stream: stream.try_clone()?,
        until: Instant::now() + time,
    };
    let mut reader = BufReader::new(deadline.take(MAX_HEAD as u64));
    let mut head = Vec::new();
    loop {
        let before = head.len();
        reader.read_until(b'\n', &mut head)?;
        if head.len() == before {
            return Ok(()); // closed, or the head is too long
        }
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            break;
        }
    }
    let answer = answer(&head, port, key, files);
    let mut stream = stream;
    stream.write_all(&answer)?;
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

/// The whole HTTP answer to one request head.
fn answer(head: &[u8], port: u16, key: &str, files: &dyn Files) -> Vec<u8> {
    let Ok(head) = std::str::from_utf8(head) else {
        return status(400);
    };
    let mut lines = head.lines();
    let Some(request) = lines.next() else {
        return status(400);
    };
    let mut parts = request.split(' ');
    let (Some(method), Some(target), Some(_version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return status(400);
    };
    if method != "GET" {
        return status(405);
    }
    // A page elsewhere that learns the port still cannot rebind a name to it.
    let own = format!("127.0.0.1:{port}");
    let host = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.trim())
        .collect::<Vec<_>>();
    if host != [own.as_str()] {
        return status(403);
    }
    let Some(rest) = target.strip_prefix('/') else {
        return status(404);
    };
    let Some((given, rest)) = rest.split_once('/') else {
        return status(404);
    };
    if !same(given.as_bytes(), key.as_bytes()) {
        return status(404);
    }
    let Some((ext, path)) = rest.split_once('/') else {
        return status(404);
    };
    // Nothing to decode and nowhere to climb: no `%`, `?`, `..` or hidden name.
    if !plain_ext(ext) || !kule_ext::plain_path(path) {
        return status(404);
    }
    let Some((body, media)) = files.get(ext, path) else {
        return status(404);
    };
    let own_files = base(port, key, ext);
    let policy = format!(
        "default-src 'none'; script-src {own_files}; style-src {own_files}; \
         img-src {own_files} data:; font-src {own_files}; connect-src 'none'; \
         frame-src 'none'; form-action 'none'; base-uri 'none'; object-src 'none'; \
         frame-ancestors tauri://localhost http://tauri.localhost"
    );
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {media}\r\nContent-Length: {}\r\n\
         Content-Security-Policy: {policy}\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\nPermissions-Policy: {FEATURES}\r\n\
         X-DNS-Prefetch-Control: off\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(&body);
    out
}

fn status(code: u16) -> Vec<u8> {
    let reason = match code {
        400 => "Bad Request",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        _ => "Not Found",
    };
    format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nX-Content-Type-Options: nosniff\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n"
    )
    .into_bytes()
}

/// Compares without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// An extension id as it appears in a path: `publisher.name`.
fn plain_ext(ext: &str) -> bool {
    !ext.is_empty()
        && ext.len() <= 41
        && ext
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serves any path of acme.notes but missing.html, as a careless store
    /// would: what climbs or decodes must be stopped by the server.
    struct One;
    impl Files for One {
        fn get(&self, ext: &str, path: &str) -> Option<(Vec<u8>, &'static str)> {
            (ext == "acme.notes" && path != "missing.html")
                .then(|| (b"<p>hi</p>".to_vec(), "text/html; charset=utf-8"))
        }
    }

    const KEY: &str = "00112233445566778899aabbccddeeff";

    fn ask(head: &str) -> String {
        String::from_utf8(answer(head.as_bytes(), 4000, KEY, &One)).unwrap()
    }

    fn get(target: &str) -> String {
        ask(&format!(
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:4000\r\n\r\n"
        ))
    }

    #[test]
    fn a_file_comes_with_a_policy_that_keeps_it_to_its_own_files() {
        let reply = get(&format!("/{KEY}/acme.notes/panel.html"));
        assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
        assert!(reply.contains("X-Content-Type-Options: nosniff"), "{reply}");
        let own = format!("http://127.0.0.1:4000/{KEY}/acme.notes/");
        assert!(
            reply.contains(&format!("default-src 'none'; script-src {own};")),
            "{reply}"
        );
        assert!(reply.contains("connect-src 'none'"), "{reply}");
        assert!(
            reply.contains("frame-ancestors tauri://localhost"),
            "{reply}"
        );
        assert!(reply.contains("Permissions-Policy: camera=()"), "{reply}");
        assert!(reply.contains("clipboard-write=()"), "{reply}");
        assert!(reply.contains("X-DNS-Prefetch-Control: off"), "{reply}");
        assert!(reply.ends_with("\r\n\r\n<p>hi</p>"), "{reply}");
    }

    #[test]
    fn anything_but_a_plain_get_of_a_known_file_is_refused() {
        let other_key = "ffffffffffffffffffffffffffffffff";
        for (target, code) in [
            (format!("/{other_key}/acme.notes/panel.html"), "404"),
            (format!("/{KEY}/acme.notes/missing.html"), "404"),
            (format!("/{KEY}/other.ext/panel.html"), "404"),
            (format!("/{KEY}/acme.notes/../acme.notes/panel.html"), "404"),
            (format!("/{KEY}/acme.notes/%2e%2e/panel.html"), "404"),
            (format!("/{KEY}/acme.notes/panel.html?x=1"), "404"),
            (format!("/{KEY}/acme.notes//panel.html"), "404"),
            (format!("/{KEY}/ACME.notes/panel.html"), "404"),
            (format!("/{KEY}"), "404"),
            ("/".to_owned(), "404"),
        ] {
            let reply = get(&target);
            assert!(
                reply.starts_with(&format!("HTTP/1.1 {code} ")),
                "{target}: {reply}"
            );
            assert!(!reply.contains("<p>hi</p>"), "{target}");
        }
        let target = format!("/{KEY}/acme.notes/panel.html");
        for head in [
            format!("POST {target} HTTP/1.1\r\nHost: 127.0.0.1:4000\r\n\r\n"),
            format!("GET {target} HTTP/1.1\r\nHost: evil.example:4000\r\n\r\n"),
            format!("GET {target} HTTP/1.1\r\nHost: localhost:4000\r\n\r\n"),
            format!("GET {target} HTTP/1.1\r\n\r\n"),
            format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:4000\r\nHost: evil.example\r\n\r\n"),
            format!("GET {target} HTTP/1.1 extra\r\nHost: 127.0.0.1:4000\r\n\r\n"),
        ] {
            let reply = ask(&head);
            assert!(!reply.starts_with("HTTP/1.1 200"), "{head:?}: {reply}");
        }
    }

    /// A frame that navigates itself may go only where the window's policy
    /// lets frames be: the extension server, never another local port.
    #[test]
    fn the_window_frames_the_extension_server_alone() {
        let config = include_str!("../tauri.conf.json");
        assert!(
            config.contains("frame-src 'none'"),
            "the config frames nothing"
        );
        assert!(
            !config.contains(":*"),
            "no port wildcard anywhere in the config"
        );
        let csp = "default-src 'self'; frame-src 'none'";
        let policy = frame_policy(csp, Some("http://127.0.0.1:4000"));
        assert_eq!(
            policy,
            "default-src 'self'; frame-src http://127.0.0.1:4000"
        );
        assert_eq!(frame_policy(csp, None), csp, "no server, no frames");
    }

    /// A byte now and then does not hold a connection past its time.
    #[test]
    fn a_head_that_trickles_in_is_given_up_on() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            for _ in 0..20 {
                if stream.write_all(b"G").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let (stream, _) = listener.accept().unwrap();
        let started = Instant::now();
        let served = serve_within(stream, addr.port(), KEY, &One, Duration::from_millis(300));
        assert!(served.is_err(), "{served:?}");
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "{:?}",
            started.elapsed()
        );
        client.join().unwrap();
    }

    #[test]
    fn the_server_answers_over_loopback_only_under_its_key() {
        let server = start(Arc::new(One)).unwrap();
        let prefix = server.prefix();
        assert!(prefix.starts_with("http://127.0.0.1:"), "{prefix}");
        let addr = prefix
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_owned();
        let path = prefix
            .trim_start_matches("http://")
            .trim_start_matches(&addr)
            .to_owned();
        let mut stream = TcpStream::connect(&addr).unwrap();
        write!(
            stream,
            "GET {path}acme.notes/panel.html HTTP/1.1\r\nHost: {addr}\r\n\r\n"
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(reply.ends_with("<p>hi</p>"), "{reply}");

        // A head over the cap is not read to its end, so not answered.
        let mut stream = TcpStream::connect(&addr).unwrap();
        let filler = "a".repeat(MAX_HEAD);
        let _ = write!(
            stream,
            "GET {path}acme.notes/panel.html HTTP/1.1\r\nHost: {addr}\r\nX-Filler: {filler}\r\n\r\n"
        );
        let mut reply = Vec::new();
        let _ = stream.read_to_end(&mut reply);
        assert!(reply.is_empty(), "{}", String::from_utf8_lossy(&reply));
    }
}
