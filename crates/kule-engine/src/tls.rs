//! The task's proxy looking inside TLS, for the hosts a secret is for and
//! no other: it puts the secret into the request there (see
//! [`crate::secrets`]).
//!
//! Each proxy makes a certificate authority of its own, valid for a week
//! and, by its name constraints, only for those hosts; its key never leaves
//! the proxy's memory. The task's commands are told to trust it through
//! the variables tools read (`SSL_CERT_FILE`, `NODE_EXTRA_CA_CERTS`, …). The
//! proxy speaks TLS to the real host itself, checking its certificate
//! against the web's roots.
//!
//! Requests on a connection are taken one after another (keep-alive): each
//! is read, filled and passed on whole, then its response is streamed back
//! as it comes, until its framing says it is over. HTTP/2 is not offered; a
//! client falls back to HTTP/1.1. A request whose framing two readers could
//! take differently (`Transfer-Encoding` with `Content-Length`, lengths that
//! disagree) is refused.

use std::collections::{BTreeSet, HashMap};
use std::io::{self, Cursor, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
};

use crate::error::{Error, Result};
use crate::secrets::Vault;

/// The most a request's line and headers may take.
const MAX_HEAD: usize = 64 * 1024;

pub(crate) struct Interceptor {
    vault: Vault,
    hosts: BTreeSet<String>,
    issuer: Issuer<'static, KeyPair>,
    ca_der: CertificateDer<'static>,
    ca_pem: String,
    /// Certificates made so far, by host.
    leaves: Mutex<HashMap<String, (CertificateDer<'static>, Vec<u8>)>>,
    upstream: Arc<ClientConfig>,
    /// How long a client may keep a connection without a request.
    idle: Duration,
}

fn tls_error(e: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("TLS: {e}"))
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The web's roots, which the proxy checks real hosts against.
pub(crate) fn web_roots() -> RootCertStore {
    RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    }
}

impl Interceptor {
    /// An authority for the hosts `vault`'s secrets are for, checking real
    /// hosts against `roots`.
    pub fn new(vault: Vault, roots: RootCertStore) -> Result<Interceptor> {
        let hosts = vault.hosts();
        let now = time::OffsetDateTime::now_utc();
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, "Kule task proxy (secrets)");
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        // Even taken, it can vouch for no other host.
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: hosts
                .iter()
                .map(|h| GeneralSubtree::DnsName(h.clone()))
                .collect(),
            excluded_subtrees: Vec::new(),
        });
        params.not_before = now - time::Duration::hours(1);
        params.not_after = now + time::Duration::days(7);
        let key = KeyPair::generate().map_err(tls_error)?;
        let cert = params.self_signed(&key).map_err(tls_error)?;
        let upstream = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let mut upstream = upstream;
        upstream.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Interceptor {
            vault,
            hosts,
            ca_der: cert.der().clone(),
            ca_pem: cert.pem(),
            issuer: Issuer::new(params, key),
            leaves: Mutex::new(HashMap::new()),
            upstream: Arc::new(upstream),
            idle: Duration::from_secs(60),
        })
    }

    /// The authority's certificate, for the task's commands to trust.
    pub fn ca_pem(&self) -> &str {
        &self.ca_pem
    }

    /// Whether the proxy looks inside a connection to `host` on `port`.
    pub fn wants(&self, host: &str, port: u16) -> bool {
        port == 443 && self.hosts.contains(host)
    }

    fn leaf(&self, host: &str) -> Result<(CertificateDer<'static>, Vec<u8>)> {
        let mut leaves = self.leaves.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(leaf) = leaves.get(host) {
            return Ok(leaf.clone());
        }
        let now = time::OffsetDateTime::now_utc();
        let mut params = CertificateParams::new(vec![host.to_owned()]).map_err(tls_error)?;
        params.distinguished_name.push(DnType::CommonName, host);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        // OpenSSL's strict checks (Python 3.13 on) refuse a leaf without it.
        params.use_authority_key_identifier_extension = true;
        params.not_before = now - time::Duration::hours(1);
        params.not_after = now + time::Duration::days(7);
        let key = KeyPair::generate().map_err(tls_error)?;
        let cert = params.signed_by(&key, &self.issuer).map_err(tls_error)?;
        let leaf = (cert.der().clone(), key.serialize_der());
        leaves.insert(host.to_owned(), leaf.clone());
        Ok(leaf)
    }

    /// Serves one tunnel to `host`: TLS with the client under the task's
    /// authority, TLS with the host, each request filled with the secrets
    /// bound there. Returns the names of the secrets that went in.
    pub fn serve(&self, host: &str, tunnel: lonca_net::Tunnel) -> Result<Vec<String>> {
        let io = |e: io::Error| Error::Invalid(format!("{host}: {e}"));
        tunnel
            .client
            .set_read_timeout(Some(self.idle))
            .map_err(io)?;
        // A model can take minutes to answer.
        tunnel
            .upstream
            .set_read_timeout(Some(Duration::from_secs(600)))
            .map_err(io)?;
        let (leaf, key) = self.leaf(host)?;
        let mut server = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(tls_error)?
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf, self.ca_der.clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
            )
            .map_err(tls_error)?;
        server.alpn_protocols = vec![b"http/1.1".to_vec()];
        let conn = ServerConnection::new(Arc::new(server)).map_err(tls_error)?;
        let mut client = Conn::new(StreamOwned::new(
            conn,
            Early {
                early: Cursor::new(tunnel.early),
                inner: tunnel.client,
            },
        ));
        let name = ServerName::try_from(host.to_owned()).map_err(tls_error)?;
        let conn = ClientConnection::new(Arc::clone(&self.upstream), name).map_err(tls_error)?;
        let mut upstream = Conn::new(StreamOwned::new(conn, tunnel.upstream));

        let mut used = BTreeSet::new();
        let mut served = 0;
        loop {
            let head = match client.head() {
                Ok(Some(head)) => head,
                Ok(None) => break,
                // A client that keeps a connection it no longer uses.
                Err(e) if served > 0 && quiet(&e) => break,
                Err(e) => return Err(io(e)),
            };
            let Ok(head) = String::from_utf8(head) else {
                return Err(Error::Invalid(format!(
                    "{host}: a request head that is not text"
                )));
            };
            let request = Request::of(&head).map_err(io)?;
            // Someone else's credentials to the person's host: an account
            // the work could be sent to. Answered here, never passed on.
            if let Some(header) = self.vault.foreign_credential(host, &head) {
                let said = format!(
                    "Kule refused this request: its {header} is not a credential Kule holds for {host}.\n"
                );
                write!(
                    client.stream,
                    "HTTP/1.1 403 Forbidden\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{said}",
                    said.len()
                )
                .map_err(io)?;
                client.stream.flush().map_err(io)?;
                break;
            }
            let (head, names) = self.vault.fill(host, &forward(&head, request.close));
            used.extend(names);
            // The body as it came: a secret goes in the credentials alone.
            upstream.stream.write_all(head.as_bytes()).map_err(io)?;
            relay(request.framing, &mut client, &mut upstream.stream).map_err(io)?;
            upstream.stream.flush().map_err(io)?;
            // The response, as it comes, with any secret it repeats blanked
            // out (an echo, an error quoting the key).
            let last = loop {
                let Some(head) = upstream.head().map_err(io)? else {
                    return Err(Error::Invalid(format!("{host}: closed without a response")));
                };
                let response = Response::of(&String::from_utf8_lossy(&head), &request.method);
                let mut out = Blanked {
                    blank: Blank::new(self.vault.values()),
                    to: &mut client.stream,
                };
                out.write_all(&head).map_err(io)?;
                relay(response.framing, &mut upstream, &mut out).map_err(io)?;
                out.finish().map_err(io)?;
                if !response.interim {
                    break response.close;
                }
            };
            served += 1;
            if last || request.close {
                break;
            }
        }
        client.stream.conn.send_close_notify();
        let _ = client.stream.flush();
        Ok(used.into_iter().collect())
    }
}

/// An error that only means the other side went quiet or away.
fn quiet(e: &io::Error) -> bool {
    use io::ErrorKind::*;
    matches!(
        e.kind(),
        WouldBlock | TimedOut | UnexpectedEof | ConnectionReset | BrokenPipe
    )
}

/// A writer that blanks the secrets out of what goes through it, passing
/// each piece on at once (a stream of events stays a stream).
struct Blanked<'a, W: Write> {
    blank: Blank,
    to: &'a mut W,
}

impl<W: Write> Write for Blanked<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let out = self.blank.pass(buf);
        self.to.write_all(&out)?;
        self.to.flush()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.to.flush()
    }
}

impl<W: Write> Blanked<'_, W> {
    /// What was held back, at the end of a message.
    fn finish(&mut self) -> io::Result<()> {
        let out = self.blank.finish();
        self.to.write_all(&out)?;
        self.to.flush()
    }
}

/// A stream with each of some values replaced by as many `*`: lengths stay,
/// so `Content-Length` and chunk sizes still hold. A value split over two
/// reads is caught by holding back the tail of each; one split by a chunk
/// header in between is not.
struct Blank {
    values: Vec<Vec<u8>>,
    held: Vec<u8>,
    longest: usize,
}

impl Blank {
    fn new(values: Vec<String>) -> Blank {
        let values: Vec<Vec<u8>> = values
            .into_iter()
            .filter(|v| v.len() >= 4)
            .map(String::into_bytes)
            .collect();
        let longest = values.iter().map(Vec::len).max().unwrap_or(0);
        Blank {
            values,
            held: Vec::new(),
            longest,
        }
    }

    /// What can go on now of what was held and `data`.
    fn pass(&mut self, data: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(data);
        self.blank();
        let keep = self.longest.saturating_sub(1).min(self.held.len());
        let rest = self.held.split_off(self.held.len() - keep);
        std::mem::replace(&mut self.held, rest)
    }

    /// What was still held, at the end of the stream.
    fn finish(&mut self) -> Vec<u8> {
        self.blank();
        std::mem::take(&mut self.held)
    }

    fn blank(&mut self) {
        for value in &self.values {
            let mut at = 0;
            while let Some(i) = self.held[at..]
                .windows(value.len())
                .position(|w| w == value.as_slice())
            {
                let start = at + i;
                self.held[start..start + value.len()].fill(b'*');
                at = start + value.len();
            }
        }
    }
}

/// The client's side of the tunnel: what it sent with its request first.
struct Early {
    early: Cursor<Vec<u8>>,
    inner: TcpStream,
}

impl Read for Early {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.early.read(buf)?;
        if n > 0 {
            return Ok(n);
        }
        self.inner.read(buf)
    }
}

impl Write for Early {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// One side of the tunnel, read a message at a time: what was read past
/// the end of one is where the next starts.
struct Conn<S> {
    buf: Vec<u8>,
    stream: S,
}

impl<S: Read> Read for Conn<S> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.buf.is_empty() {
            return self.stream.read(out);
        }
        let n = out.len().min(self.buf.len());
        out[..n].copy_from_slice(&self.buf[..n]);
        self.buf.drain(..n);
        Ok(n)
    }
}

impl<S: Read> Conn<S> {
    fn new(stream: S) -> Conn<S> {
        Conn {
            buf: Vec::new(),
            stream,
        }
    }

    /// The next message's line and headers; `None` when the stream ends
    /// before one starts.
    fn head(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut chunk = [0u8; 4096];
        loop {
            if let Some(end) = self.buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let rest = self.buf.split_off(end + 4);
                return Ok(Some(std::mem::replace(&mut self.buf, rest)));
            }
            if self.buf.len() > MAX_HEAD {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "message head too large",
                ));
            }
            let n = match self.stream.read(&mut chunk) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof && self.buf.is_empty() => 0,
                Err(e) => return Err(e),
            };
            if n == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "message head cut short",
                ));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    fn line(&mut self) -> io::Result<Vec<u8>> {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            if line.len() > MAX_HEAD {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "chunk line too long",
                ));
            }
            self.read_exact(&mut byte)?;
            line.push(byte[0]);
        }
        Ok(line)
    }
}

/// The request head to send on: without what concerns this hop only, and
/// without `Accept-Encoding`, so the response comes as it is, where a secret
/// it repeats can be blanked out. `close` asks the host to close after it.
fn forward(head: &str, close: bool) -> String {
    let mut out = String::with_capacity(head.len() + 20);
    let mut lines = head.split("\r\n");
    if let Some(first) = lines.next() {
        out.push_str(first);
        out.push_str("\r\n");
    }
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let name = line.split(':').next().unwrap_or("").trim();
        let dropped = [
            "connection",
            "proxy-connection",
            "keep-alive",
            "proxy-authorization",
            "te",
            "upgrade",
            "expect",
            "accept-encoding",
        ]
        .iter()
        .any(|d| name.eq_ignore_ascii_case(d));
        if !dropped {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    if close {
        out.push_str("Connection: close\r\n");
    }
    out.push_str("\r\n");
    out
}

/// Every value of the header `wanted`.
fn headers<'a>(head: &'a str, wanted: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    head.split("\r\n").skip(1).filter_map(move |line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(wanted)
            .then(|| value.trim())
    })
}

/// Whether the head's `Connection` header holds `token`.
fn connection_has(head: &str, token: &str) -> bool {
    headers(head, "connection")
        .flat_map(|v| v.split(','))
        .any(|t| t.trim().eq_ignore_ascii_case(token))
}

/// How a message's body ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framing {
    Empty,
    Length(u64),
    Chunked,
    /// When the sender closes; a response only.
    Close,
}

/// The `Content-Length` of a head, if it has one: refused when its values
/// disagree or are not a number.
fn content_length(head: &str) -> io::Result<Option<u64>> {
    let mut length = None;
    for value in headers(head, "content-length").flat_map(|v| v.split(',')) {
        let n = value
            .trim()
            .parse::<u64>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad Content-Length"))?;
        if length.is_some_and(|l| l != n) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Content-Length values disagree",
            ));
        }
        length = Some(n);
    }
    Ok(length)
}

/// Whether the head's last transfer coding is chunked; `None` without one.
fn chunked(head: &str) -> Option<bool> {
    let codings: Vec<String> = headers(head, "transfer-encoding")
        .flat_map(|v| v.split(','))
        .map(|c| c.trim().to_ascii_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    codings.last().map(|c| c == "chunked")
}

struct Request {
    method: String,
    framing: Framing,
    /// The client is done after this one.
    close: bool,
}

impl Request {
    fn of(head: &str) -> io::Result<Request> {
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
        let line = head.split("\r\n").next().unwrap_or("");
        let mut parts = line.split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let version = parts.nth(1).unwrap_or("");
        let length = content_length(head)?;
        let framing = match (chunked(head), length) {
            (Some(_), Some(_)) => return Err(bad("both Transfer-Encoding and Content-Length")),
            (Some(true), None) => Framing::Chunked,
            (Some(false), None) => return Err(bad("a transfer coding other than chunked")),
            (None, Some(n)) => Framing::Length(n),
            (None, None) => Framing::Empty,
        };
        let close = connection_has(head, "close")
            || (version == "HTTP/1.0" && !connection_has(head, "keep-alive"));
        Ok(Request {
            method,
            framing,
            close,
        })
    }
}

struct Response {
    framing: Framing,
    /// A response before the real one (`103 Early Hints`).
    interim: bool,
    /// The host is done after this one.
    close: bool,
}

impl Response {
    fn of(head: &str, method: &str) -> Response {
        let line = head.split("\r\n").next().unwrap_or("");
        let mut parts = line.split(' ');
        let version = parts.next().unwrap_or("");
        let status = parts.next().and_then(|s| s.parse::<u16>().ok());
        let Some(status) = status else {
            return Response {
                framing: Framing::Close,
                interim: false,
                close: true,
            };
        };
        if (100..200).contains(&status) && status != 101 {
            return Response {
                framing: Framing::Empty,
                interim: true,
                close: false,
            };
        }
        let framing = if status == 101 {
            Framing::Close
        } else if method.eq_ignore_ascii_case("HEAD") || status == 204 || status == 304 {
            Framing::Empty
        } else {
            match (chunked(head), content_length(head)) {
                (Some(true), _) => Framing::Chunked,
                (Some(false), _) => Framing::Close,
                (None, Ok(Some(n))) => Framing::Length(n),
                _ => Framing::Close,
            }
        };
        let close = framing == Framing::Close
            || connection_has(head, "close")
            || (version == "HTTP/1.0" && !connection_has(head, "keep-alive"));
        Response {
            framing,
            interim: false,
            close,
        }
    }
}

/// Passes a message's body on, as its framing says it is.
fn relay<S: Read>(framing: Framing, from: &mut Conn<S>, to: &mut impl Write) -> io::Result<()> {
    match framing {
        Framing::Empty => Ok(()),
        Framing::Length(length) => {
            let copied = io::copy(&mut from.by_ref().take(length), to)?;
            if copied < length {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "body cut short",
                ));
            }
            Ok(())
        }
        Framing::Chunked => loop {
            let line = from.line()?;
            to.write_all(&line)?;
            let text = String::from_utf8_lossy(&line);
            let size = text.trim().split(';').next().unwrap_or("").trim();
            let size = u64::from_str_radix(size, 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
            if size == 0 {
                // Trailers, up to the empty line.
                loop {
                    let line = from.line()?;
                    to.write_all(&line)?;
                    if line == b"\r\n" {
                        return Ok(());
                    }
                }
            }
            // A chunk cut short leaves the next line to meet the end.
            io::copy(&mut from.by_ref().take(size + 2), to)?;
        },
        Framing::Close => {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match from.read(&mut buf) {
                    Ok(0) => return Ok(()),
                    Ok(n) => to.write_all(&buf[..n])?,
                    // A host that closes without saying so has still sent it all.
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
                    Err(e) => return Err(e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use tempfile::TempDir;

    use super::*;

    const HOST: &str = "api.test.example";

    struct Host {
        port: u16,
        roots: RootCertStore,
        /// Up once a request head holds the real value.
        saw_real: Arc<AtomicBool>,
        connections: Arc<AtomicUsize>,
    }

    /// A host with a certificate from a root of its own, answering each
    /// request on a connection with the request it received: sized, or chunked
    /// for `/chunked`, or up to its close for `/close`; nothing for `HEAD`.
    fn echo_host() -> Host {
        let saw_real = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(AtomicUsize::new(0));
        let (saw, count) = (Arc::clone(&saw_real), Arc::clone(&connections));
        let root_key = KeyPair::generate().unwrap();
        let mut root = CertificateParams::default();
        root.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        root.key_usages = vec![KeyUsagePurpose::KeyCertSign];
        let root_cert = root.self_signed(&root_key).unwrap();
        let issuer = Issuer::new(root, root_key);
        let key = KeyPair::generate().unwrap();
        let leaf = CertificateParams::new(vec![HOST.to_owned()])
            .unwrap()
            .signed_by(&key, &issuer)
            .unwrap();
        let config = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der())),
            )
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                count.fetch_add(1, Ordering::SeqCst);
                let (config, saw) = (Arc::clone(&config), Arc::clone(&saw));
                std::thread::spawn(move || {
                    let conn = ServerConnection::new(config).unwrap();
                    let mut tls = Conn::new(StreamOwned::new(conn, stream.unwrap()));
                    while let Ok(Some(mut head)) = tls.head() {
                        let text = String::from_utf8_lossy(&head).into_owned();
                        let framing = Request::of(&text).unwrap().framing;
                        // The body after the head, as it came.
                        if relay(framing, &mut tls, &mut head).is_err() {
                            break;
                        }
                        if head.windows(10).any(|w| w == b"real-value") {
                            saw.store(true, Ordering::SeqCst);
                        }
                        let out = &mut tls.stream;
                        if text.starts_with("GET /close ") {
                            let _ = write!(out, "HTTP/1.1 200 OK\r\n\r\n");
                            let _ = out.write_all(&head);
                            break;
                        } else if text.starts_with("GET /chunked ") {
                            let (a, b) = head.split_at(head.len() / 2);
                            let _ = write!(
                                out,
                                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
                            );
                            for part in [a, b] {
                                let _ = write!(out, "{:x}\r\n", part.len());
                                let _ = out.write_all(part);
                                let _ = out.write_all(b"\r\n");
                            }
                            let _ = out.write_all(b"0\r\n\r\n");
                        } else {
                            let _ = write!(
                                out,
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                head.len()
                            );
                            if !text.starts_with("HEAD ") {
                                let _ = out.write_all(&head);
                            }
                        }
                        let _ = out.flush();
                        if connection_has(&text, "close") {
                            break;
                        }
                    }
                    tls.stream.conn.send_close_notify();
                    let _ = tls.stream.flush();
                });
            }
        });
        let mut roots = RootCertStore::empty();
        roots.add(root_cert.der().clone()).unwrap();
        Host {
            port,
            roots,
            saw_real,
            connections,
        }
    }

    /// Sends `requests` to `interceptor` for `host` the way a client behind
    /// the proxy does, trusting the task's authority alone, and reads until
    /// the proxy closes; what came back, or the error the client met.
    fn ask(
        interceptor: &Arc<Interceptor>,
        up: u16,
        host: &str,
        requests: &str,
    ) -> (io::Result<String>, Vec<String>) {
        let (reply, served) = ask_until(interceptor, up, host, requests, None, false);
        (reply, served.unwrap_or_default())
    }

    /// As [`ask`], but with `until`, the client reads only until the reply
    /// holds it, then goes away without a word, as a pool closing an idle
    /// connection does; with `hold`, only once the proxy is done.
    fn ask_until(
        interceptor: &Arc<Interceptor>,
        up: u16,
        host: &str,
        requests: &str,
        until: Option<&str>,
        hold: bool,
    ) -> (io::Result<String>, Result<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (proxy_end, _) = listener.accept().unwrap();
        let tunnel = lonca_net::Tunnel {
            client: proxy_end,
            upstream: TcpStream::connect(("127.0.0.1", up)).unwrap(),
            early: Vec::new(),
        };
        let (served, host_owned) = (Arc::clone(interceptor), host.to_owned());
        let serving = std::thread::spawn(move || served.serve(&host_owned, tunnel));
        let mut roots = RootCertStore::empty();
        roots.add(interceptor.ca_der.clone()).unwrap();
        let config = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let conn = ClientConnection::new(
            Arc::new(config),
            ServerName::try_from(host.to_owned()).unwrap(),
        )
        .unwrap();
        let mut tls = StreamOwned::new(conn, client);
        let reply = tls.write_all(requests.as_bytes()).and_then(|()| {
            let mut out = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let text = String::from_utf8_lossy(&out).into_owned();
                if until.is_some_and(|u| text.contains(u)) {
                    return Ok(text);
                }
                match tls.read(&mut buf) {
                    Ok(0) => return Ok(text),
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(text),
                    Err(e) => return Err(e),
                }
            }
        });
        // Held: open and quiet until the proxy gives up on it.
        let served = if hold {
            let served = serving.join().unwrap();
            drop(tls);
            served
        } else {
            let _ = tls.sock.shutdown(std::net::Shutdown::Both);
            drop(tls);
            serving.join().unwrap()
        };
        (reply, served)
    }

    fn interceptor(home: &TempDir, roots: RootCertStore) -> (Arc<Interceptor>, String) {
        crate::secrets::add_secret(
            home.path(),
            "API_KEY",
            "real-value",
            &[HOST.to_owned()],
            None,
        )
        .unwrap();
        let (ws, dir) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        let placeholder = vault.env()[0].1.clone();
        (
            Arc::new(Interceptor::new(vault, roots).unwrap()),
            placeholder,
        )
    }

    #[test]
    fn the_secret_reaches_its_host_in_place_of_the_placeholder() {
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, host.roots);
        assert!(interceptor.wants(HOST, 443) && !interceptor.wants(HOST, 8443));
        assert!(!interceptor.wants("other.example", 443));
        let head = format!(
            "GET /v1 HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {p}\r\nConnection: close\r\n\r\n"
        );
        let (reply, used) = ask(&interceptor, host.port, HOST, &head);
        let reply = reply.unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(host.saw_real.load(Ordering::SeqCst));
        // The host repeated it; the client sees it blanked out.
        assert!(
            reply.contains("Authorization: Bearer **********\r\n"),
            "{reply}"
        );
        assert!(!reply.contains(&p), "{reply}");
        assert_eq!(used, ["API_KEY"]);
    }

    #[test]
    fn requests_follow_one_another_on_a_connection() {
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, host.roots);
        let request = |method: &str, path: &str, last: &str| {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {p}\r\n{last}\r\n"
            )
        };
        let sent = [
            request("POST", "/a", "Content-Length: 3\r\n") + "abc",
            request("HEAD", "/h", ""),
            request("GET", "/chunked", ""),
            request("GET", "/close", ""),
        ]
        .concat();
        let (reply, used) = ask(&interceptor, host.port, HOST, &sent);
        let reply = reply.unwrap();
        assert_eq!(reply.matches("HTTP/1.1 200 OK").count(), 4, "{reply}");
        assert_eq!(reply.matches("Bearer **********").count(), 3, "{reply}");
        assert!(reply.contains("POST /a HTTP/1.1"), "{reply}");
        assert!(reply.contains("\r\n0\r\n\r\n"), "{reply}");
        assert!(reply.ends_with("GET /close HTTP/1.1\r\nHost: api.test.example\r\nAuthorization: Bearer **********\r\n\r\n"), "{reply}");
        assert!(
            !reply.contains("real-value") && !reply.contains(&p),
            "{reply}"
        );
        assert_eq!(host.connections.load(Ordering::SeqCst), 1);
        assert_eq!(used, ["API_KEY"]);
    }

    /// A secret goes in its credentials alone: in a body, a URL or any other
    /// header (a prompt sent to a model, say) the placeholder stays as it is,
    /// so the value never reaches what could repeat it back.
    #[test]
    fn a_secret_goes_nowhere_but_the_credentials() {
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, host.roots);
        let json = format!("{{\"prompt\":\"say {p}\"}}");
        let chunk = format!("key={p}");
        let sent = format!(
            "POST /a?key={p} HTTP/1.1\r\nHost: {HOST}\r\nX-Note: {p}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{json}\
             POST /b HTTP/1.1\r\nHost: {HOST}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{chunk}\r\n0\r\n\r\n",
            json.len(),
            chunk.len()
        );
        let (reply, used) = ask(&interceptor, host.port, HOST, &sent);
        let reply = reply.unwrap();
        assert!(!host.saw_real.load(Ordering::SeqCst), "{reply}");
        assert!(used.is_empty(), "{used:?}");
        assert!(reply.contains(&json), "{reply}");
    }

    /// To a host Kule holds a secret for, a request carrying credentials of
    /// its own (an account that is not the person's) goes nowhere.
    #[test]
    fn credentials_kule_does_not_hold_are_refused() {
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, host.roots);
        for credential in [
            "x-api-key: someone-elses-key".to_owned(),
            "Authorization: Bearer someone-elses-key".to_owned(),
        ] {
            let sent = format!(
                "POST /upload HTTP/1.1\r\nHost: {HOST}\r\n{credential}\r\nContent-Length: 4\r\nConnection: close\r\n\r\nfile"
            );
            let (reply, _) = ask(&interceptor, host.port, HOST, &sent);
            let reply = reply.unwrap_or_default();
            assert!(reply.starts_with("HTTP/1.1 403"), "{credential}: {reply}");
            assert!(!reply.contains("someone-elses-key"), "{reply}");
        }
        // Its own placeholder, or no credentials at all, go through.
        for credential in [format!("x-api-key: {p}\r\n"), String::new()] {
            let sent = format!(
                "GET /v1 HTTP/1.1\r\nHost: {HOST}\r\n{credential}Connection: close\r\n\r\n"
            );
            let (reply, _) = ask(&interceptor, host.port, HOST, &sent);
            assert!(reply.unwrap().starts_with("HTTP/1.1 200"), "{credential}");
        }
    }

    #[test]
    fn a_client_that_leaves_a_kept_connection_is_no_error() {
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (mut interceptor, p) = interceptor(&home, host.roots);
        Arc::get_mut(&mut interceptor).unwrap().idle = Duration::from_millis(300);
        let head = format!("GET /v1 HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {p}\r\n\r\n");
        // Closed by the client, or kept open and quiet past the idle time.
        for hold in [false, true] {
            let (reply, served) = ask_until(
                &interceptor,
                host.port,
                HOST,
                &head,
                Some("**********\r\n\r\n"),
                hold,
            );
            assert!(reply.unwrap().starts_with("HTTP/1.1 200 OK"));
            assert_eq!(served.unwrap(), ["API_KEY"], "held: {hold}");
        }
    }

    #[test]
    fn the_task_authority_vouches_for_no_other_host() {
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, host.roots);
        // Even made to sign for another host, the client refuses it.
        let head =
            format!("GET / HTTP/1.1\r\nHost: other.example\r\nAuthorization: Bearer {p}\r\n\r\n");
        let (reply, used) = ask(&interceptor, host.port, "other.example", &head);
        assert!(reply.is_err(), "{reply:?}");
        assert!(used.is_empty());
    }

    #[test]
    fn a_host_that_is_not_who_it_claims_gets_nothing() {
        // The real host's certificate is checked against the web's roots,
        // which do not hold the test root.
        let host = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, web_roots());
        let head = format!("GET / HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {p}\r\n\r\n");
        let (_, used) = ask(&interceptor, host.port, HOST, &head);
        assert!(!host.saw_real.load(Ordering::SeqCst));
        assert!(used.is_empty());
    }

    #[test]
    fn a_repeated_secret_is_blanked_even_split_between_reads() {
        let mut blank = Blank::new(vec!["s3cret-value".into()]);
        let mut out = blank.pass(b"error: key s3cr");
        out.extend(blank.pass(b"et-value is wrong; s3cret-value"));
        out.extend(blank.finish());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "error: key ************ is wrong; ************"
        );
    }

    #[test]
    fn what_concerns_one_hop_stays_and_a_close_is_passed_on() {
        let head = "POST /x HTTP/1.1\r\nHost: a.example\r\nConnection: keep-alive\r\nExpect: 100-continue\r\nUpgrade: websocket\r\nAccept-Encoding: gzip, br\r\nContent-Length: 2\r\n\r\n";
        assert_eq!(
            forward(head, false),
            "POST /x HTTP/1.1\r\nHost: a.example\r\nContent-Length: 2\r\n\r\n"
        );
        assert!(forward(head, true).ends_with("Content-Length: 2\r\nConnection: close\r\n\r\n"));
    }

    #[test]
    fn a_request_two_readers_could_frame_apart_is_refused() {
        let of = |extra: &str| Request::of(&format!("POST / HTTP/1.1\r\nHost: a\r\n{extra}\r\n"));
        assert!(of("Transfer-Encoding: chunked\r\nContent-Length: 3\r\n").is_err());
        assert!(of("Content-Length: 3\r\nContent-Length: 4\r\n").is_err());
        assert!(of("Content-Length: 3, 4\r\n").is_err());
        assert!(of("Content-Length: x\r\n").is_err());
        assert!(of("Transfer-Encoding: gzip\r\n").is_err());
        assert_eq!(
            of("Content-Length: 3, 3\r\n").unwrap().framing,
            Framing::Length(3)
        );
        assert_eq!(
            of("Transfer-Encoding: gzip, chunked\r\n").unwrap().framing,
            Framing::Chunked
        );
        assert_eq!(of("").unwrap().framing, Framing::Empty);
        assert!(of("Connection: Keep-Alive, close\r\n").unwrap().close);
        assert!(Request::of("GET / HTTP/1.0\r\n\r\n").unwrap().close);
    }

    #[test]
    fn a_response_ends_where_its_framing_says() {
        let of = |head: &str, method| Response::of(head, method);
        assert_eq!(
            of("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n", "HEAD").framing,
            Framing::Empty
        );
        assert_eq!(
            of(
                "HTTP/1.1 304 Not Modified\r\nContent-Length: 5\r\n\r\n",
                "GET"
            )
            .framing,
            Framing::Empty
        );
        assert_eq!(
            of("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n", "GET").framing,
            Framing::Length(5)
        );
        let no_length = of("HTTP/1.1 200 OK\r\n\r\n", "GET");
        assert!(no_length.framing == Framing::Close && no_length.close);
        assert!(of("HTTP/1.1 103 Early Hints\r\nLink: </a>\r\n\r\n", "GET").interim);
        assert!(of("HTTP/1.0 200 OK\r\nContent-Length: 1\r\n\r\n", "GET").close);
    }

    #[test]
    fn chunked_and_sized_bodies_pass_whole() {
        let sent = b"3\r\nabc\r\n2;x=1\r\nde\r\n0\r\nT: 1\r\n\r\nNEXT";
        let mut from = Conn {
            buf: sent[..5].to_vec(),
            stream: Cursor::new(sent[5..].to_vec()),
        };
        let mut out = Vec::new();
        relay(Framing::Chunked, &mut from, &mut out).unwrap();
        assert_eq!(out, &sent[..sent.len() - 4]);
        let mut rest = String::new();
        from.read_to_string(&mut rest).unwrap();
        assert_eq!(rest, "NEXT");

        let mut from = Conn {
            buf: b"he".to_vec(),
            stream: Cursor::new(b"llo world".to_vec()),
        };
        let mut out = Vec::new();
        relay(Framing::Length(5), &mut from, &mut out).unwrap();
        assert_eq!(out, b"hello");
        assert!(relay(Framing::Length(50), &mut from, &mut Vec::new()).is_err());

        let mut from = Conn {
            buf: b"5\r\nab".to_vec(),
            stream: Cursor::new(Vec::new()),
        };
        assert!(relay(Framing::Chunked, &mut from, &mut Vec::new()).is_err());
    }
}
