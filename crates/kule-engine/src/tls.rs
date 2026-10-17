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
//! One request a connection: the proxy asks the host to close after the
//! response, so every request is read, filled and passed on whole, then the
//! response is streamed back as it comes. HTTP/2 is not offered; a client
//! falls back to HTTP/1.1.

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
    /// authority, TLS with the host, one request filled with the secrets
    /// bound there. Returns the names of the secrets that went in.
    pub fn serve(&self, host: &str, tunnel: lonca_net::Tunnel) -> Result<Vec<String>> {
        let io = |e: io::Error| Error::Invalid(format!("{host}: {e}"));
        tunnel
            .client
            .set_read_timeout(Some(Duration::from_secs(60)))
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
        let mut client = StreamOwned::new(
            conn,
            Early {
                early: Cursor::new(tunnel.early),
                inner: tunnel.client,
            },
        );
        let name = ServerName::try_from(host.to_owned()).map_err(tls_error)?;
        let conn = ClientConnection::new(Arc::clone(&self.upstream), name).map_err(tls_error)?;
        let mut upstream = StreamOwned::new(conn, tunnel.upstream);

        let (head, rest) = read_head(&mut client).map_err(io)?;
        let Ok(head) = String::from_utf8(head) else {
            return Err(Error::Invalid(format!(
                "{host}: a request head that is not text"
            )));
        };
        let (head, used) = self.vault.fill(host, &close_after(&head));
        upstream.write_all(head.as_bytes()).map_err(io)?;
        let mut body = Body {
            pending: Cursor::new(rest),
            stream: &mut client,
        };
        relay_body(&head, &mut body, &mut upstream).map_err(io)?;
        upstream.flush().map_err(io)?;
        // The response, as it comes, until the host closes, with any secret
        // it repeats blanked out (an echo, an error quoting the key).
        let mut blank = Blank::new(self.vault.values());
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            match upstream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    client.write_all(&blank.pass(&buf[..n])).map_err(io)?;
                    client.flush().map_err(io)?;
                }
                // A host that closes without saying so has still sent it all.
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(io(e)),
            }
        }
        client.write_all(&blank.finish()).map_err(io)?;
        client.conn.send_close_notify();
        let _ = client.flush();
        Ok(used)
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

/// The request's line and headers, and what came after them.
fn read_head(r: &mut impl Read) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((buf, rest));
        }
        if buf.len() > MAX_HEAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request head too large",
            ));
        }
        let n = r.read(&mut chunk)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no request"));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// `head` asking the host to close after its response, without waiting
/// for a go-ahead before the body.
fn close_after(head: &str) -> String {
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
        // Without `Accept-Encoding` the response comes as it is, where a
        // secret it repeats can be blanked out.
        let dropped = [
            "connection",
            "proxy-connection",
            "keep-alive",
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
    out.push_str("Connection: close\r\n\r\n");
    out
}

fn header<'a>(head: &'a str, wanted: &str) -> Option<&'a str> {
    head.split("\r\n").skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case(wanted)
            .then(|| value.trim())
    })
}

/// The request's body as the client sends it: first what came with the
/// head, then the stream.
struct Body<'a, S: Read> {
    pending: Cursor<Vec<u8>>,
    stream: &'a mut S,
}

impl<S: Read> Read for Body<'_, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.pending.read(buf)?;
        if n > 0 {
            return Ok(n);
        }
        self.stream.read(buf)
    }
}

impl<S: Read> Body<'_, S> {
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

/// Passes the request's body on, as long as its head says it is.
fn relay_body<S: Read>(head: &str, body: &mut Body<'_, S>, to: &mut impl Write) -> io::Result<()> {
    let chunked = header(head, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        loop {
            let line = body.line()?;
            to.write_all(&line)?;
            let text = String::from_utf8_lossy(&line);
            let size = text.trim().split(';').next().unwrap_or("").trim();
            let size = u64::from_str_radix(size, 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
            if size == 0 {
                // Trailers, up to the empty line.
                loop {
                    let line = body.line()?;
                    to.write_all(&line)?;
                    if line == b"\r\n" {
                        return Ok(());
                    }
                }
            }
            io::copy(&mut body.by_ref().take(size + 2), to)?;
        }
    }
    let length = header(head, "content-length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let copied = io::copy(&mut body.by_ref().take(length), to)?;
    if copied < length {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "body cut short",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use tempfile::TempDir;

    use super::*;

    const HOST: &str = "api.test.example";

    /// A host with a certificate from a root of its own, answering every
    /// request with the head it received; the flag goes up once a head
    /// holds the real value.
    fn echo_host() -> (u16, RootCertStore, Arc<std::sync::atomic::AtomicBool>) {
        let saw_real = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let saw = Arc::clone(&saw_real);
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
                let conn = ServerConnection::new(Arc::clone(&config)).unwrap();
                let mut tls = StreamOwned::new(conn, stream.unwrap());
                let Ok((head, _)) = read_head(&mut tls) else {
                    continue;
                };
                if head.windows(10).any(|w| w == b"real-value") {
                    saw.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                let _ = write!(
                    tls,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    head.len()
                );
                let _ = tls.write_all(&head);
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        let mut roots = RootCertStore::empty();
        roots.add(root_cert.der().clone()).unwrap();
        (port, roots, saw_real)
    }

    /// Asks `interceptor` for `host` the way a client behind the proxy
    /// does, trusting the task's authority alone; the response, or the
    /// error the client met.
    fn ask(
        interceptor: &Arc<Interceptor>,
        up: u16,
        host: &str,
        head: &str,
    ) -> (io::Result<String>, Vec<String>) {
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
        let reply = tls.write_all(head.as_bytes()).and_then(|()| {
            let mut out = String::new();
            match tls.read_to_string(&mut out) {
                Ok(_) => Ok(out),
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(out),
                Err(e) => Err(e),
            }
        });
        drop(tls);
        (reply, serving.join().unwrap().unwrap_or_default())
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
        let (up, roots, saw_real) = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, roots);
        assert!(interceptor.wants(HOST, 443) && !interceptor.wants(HOST, 8443));
        assert!(!interceptor.wants("other.example", 443));
        let head = format!(
            "GET /v1 HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {p}\r\nConnection: keep-alive\r\n\r\n"
        );
        let (reply, used) = ask(&interceptor, up, HOST, &head);
        let reply = reply.unwrap();
        assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        assert!(saw_real.load(std::sync::atomic::Ordering::SeqCst));
        // The host repeated it; the client sees it blanked out.
        assert!(
            reply.contains("Authorization: Bearer **********\r\n"),
            "{reply}"
        );
        assert!(!reply.contains(&p), "{reply}");
        assert!(reply.ends_with("Connection: close\r\n\r\n"), "{reply}");
        assert_eq!(used, ["API_KEY"]);
    }

    #[test]
    fn the_task_authority_vouches_for_no_other_host() {
        let (up, roots, _) = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, roots);
        // Even made to sign for another host, the client refuses it.
        let head =
            format!("GET / HTTP/1.1\r\nHost: other.example\r\nAuthorization: Bearer {p}\r\n\r\n");
        let (reply, used) = ask(&interceptor, up, "other.example", &head);
        assert!(reply.is_err(), "{reply:?}");
        assert!(used.is_empty());
    }

    #[test]
    fn a_host_that_is_not_who_it_claims_gets_nothing() {
        // The real host's certificate is checked against the web's roots,
        // which do not hold the test root.
        let (up, _, saw_real) = echo_host();
        let home = TempDir::new().unwrap();
        let (interceptor, p) = interceptor(&home, web_roots());
        let head = format!("GET / HTTP/1.1\r\nHost: {HOST}\r\nAuthorization: Bearer {p}\r\n\r\n");
        let (_, used) = ask(&interceptor, up, HOST, &head);
        assert!(!saw_real.load(std::sync::atomic::Ordering::SeqCst));
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
    fn a_request_is_asked_to_close_and_its_go_ahead_dropped() {
        let head = "POST /x HTTP/1.1\r\nHost: a.example\r\nConnection: keep-alive\r\nExpect: 100-continue\r\nAccept-Encoding: gzip, br\r\nContent-Length: 2\r\n\r\n";
        assert_eq!(
            close_after(head),
            "POST /x HTTP/1.1\r\nHost: a.example\r\nContent-Length: 2\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn chunked_and_sized_bodies_pass_whole() {
        let head = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        let sent = b"3\r\nabc\r\n2;x=1\r\nde\r\n0\r\nT: 1\r\n\r\nNEXT";
        let mut stream = Cursor::new(sent[5..].to_vec());
        let mut body = Body {
            pending: Cursor::new(sent[..5].to_vec()),
            stream: &mut stream,
        };
        let mut out = Vec::new();
        relay_body(head, &mut body, &mut out).unwrap();
        assert_eq!(out, &sent[..sent.len() - 4]);

        let mut stream = Cursor::new(b"llo world".to_vec());
        let mut body = Body {
            pending: Cursor::new(b"he".to_vec()),
            stream: &mut stream,
        };
        let mut out = Vec::new();
        relay_body(
            "POST / HTTP/1.1\r\ncontent-length: 5\r\n\r\n",
            &mut body,
            &mut out,
        )
        .unwrap();
        assert_eq!(out, b"hello");
    }
}
