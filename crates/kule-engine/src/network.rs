//! A task's network: a proxy that every sandboxed command has to go through.
//!
//! The sandbox lets commands connect only to this proxy (see
//! [`Network::Proxy`](crate::Network::Proxy)). Each connection is judged by
//! the policy as a `net.connect` effect and written to the task's
//! `network.jsonl`, refused ones included. Network effects cannot be rolled
//! back, so a connection goes through only when the policy lets it pass on
//! its own or a person approves it while the command waits (see
//! [`Approvals`]).

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::approvals::{Answer, Approvals};
use crate::error::{Error, IoContext, Result};
use crate::policy::{Judgement, Policy, Verdict};
use crate::secrets::AgentKey;
use crate::task::Task;
use crate::tls::Interceptor;

/// One connection a command asked for, as the task's log keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub time_unix: u64,
    pub host: String,
    pub port: u16,
    pub verdict: Verdict,
    pub rules: Vec<String>,
    /// `refused`, `unresolved`, `private`, `unreachable` or `tunneled`.
    pub outcome: String,
    /// For a connection put to a person, how it was decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// The secrets the proxy put into the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,
}

/// The proxy of one task, serving until it is dropped. Dropping it closes
/// the port, withdraws the questions it has waiting and cuts the
/// connections still open.
pub struct Proxy {
    server: Option<lonca_net::Server>,
    asker: Option<Asker>,
    env: Vec<(String, OsString)>,
}

/// Where a proxy puts the connections that wait for a person.
#[derive(Clone)]
struct Asker {
    approvals: Approvals,
    source: u64,
    task: String,
    timeout: Duration,
}

impl Proxy {
    /// Starts the proxy on a free loopback port. Connections the policy puts
    /// to a person are refused at once: nobody is there to ask.
    pub fn start(task: &Task, policy: Policy) -> Result<Proxy> {
        Self::serve(task, policy, None, &[])
    }

    /// [`Proxy::start`], also putting an agent's `keys` into requests for it.
    pub fn start_with_keys(task: &Task, policy: Policy, keys: &[AgentKey]) -> Result<Proxy> {
        Self::serve(task, policy, None, keys)
    }

    /// [`Proxy::start`], putting the connections that wait for a person to
    /// `approvals` and holding each until it is answered or `timeout` runs
    /// out; an agent's `keys` go into requests for it.
    pub fn start_asking(
        task: &Task,
        policy: Policy,
        approvals: &Approvals,
        timeout: Duration,
        keys: &[AgentKey],
    ) -> Result<Proxy> {
        let asker = Asker {
            approvals: approvals.clone(),
            source: approvals.new_source(),
            task: task.id().to_owned(),
            timeout,
        };
        Self::serve(task, policy, Some(asker), keys)
    }

    fn serve(
        task: &Task,
        policy: Policy,
        asker: Option<Asker>,
        keys: &[AgentKey],
    ) -> Result<Proxy> {
        let addr = Path::new("127.0.0.1:0");
        let listener = lonca_net::bind("127.0.0.1:0").at(addr)?;
        let path = log_path(task);
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)
            .at(&path)?;
        let mut vault = task.vault()?;
        vault.with_keys(keys)?;
        let mut env: Vec<(String, OsString)> = vault
            .env()
            .into_iter()
            .map(|(name, placeholder)| (name, placeholder.into()))
            .collect();
        // Looked into: the hosts secrets go to, and the registries, which
        // commands may read from but never publish to.
        let read_only = READ_ONLY_REGISTRIES
            .iter()
            .map(|h| (*h).to_owned())
            .collect();
        let interceptor = Interceptor::new(vault, crate::tls::web_roots(), read_only)?;
        env.extend(trust(task, interceptor.ca_pem())?);
        let interceptor = Some(interceptor);
        let gate = Arc::new(Gate {
            policy,
            log: Mutex::new(log),
            pending: Mutex::new(HashMap::new()),
            asker: asker.clone(),
            interceptor,
        });
        let server = lonca_net::spawn(listener, gate).at(addr)?;
        Ok(Proxy {
            server: Some(server),
            asker,
            env,
        })
    }

    pub fn port(&self) -> u16 {
        self.server.as_ref().map_or(0, |s| s.local_addr().port())
    }

    /// What the commands behind this proxy need in their environment: each
    /// kept secret's placeholder by its name, and, when the proxy looks
    /// inside TLS for some host, where to find the authority to trust.
    pub fn env(&self) -> &[(String, OsString)] {
        &self.env
    }

    /// Closes the proxy and cuts the connections still open; once it
    /// returns, [`connections`] lists them all.
    pub fn stop(mut self) {
        self.shut();
    }

    fn shut(&mut self) {
        // Questions first: a connection held for an answer would keep the
        // server from finishing.
        if let Some(asker) = &self.asker {
            asker.approvals.withdraw(asker.source);
        }
        if let Some(server) = self.server.take() {
            server.stop();
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.shut();
    }
}

/// Every connection the task's commands asked for, oldest first.
pub fn connections(task: &Task) -> Result<Vec<Connection>> {
    let path = log_path(task);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(&path),
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line.at(&path)?;
        out.push(serde_json::from_str(&line).map_err(|source| Error::State {
            path: path.clone(),
            source,
        })?);
    }
    Ok(out)
}

fn log_path(task: &Task) -> PathBuf {
    task.dir().join("network.jsonl")
}

/// Package registries a command reads from but never publishes to: those
/// whose tools trust the task's authority through the variables `trust`
/// sets (npm, yarn, pnpm and bun; cargo; pip, uv and poetry; gem). Go's,
/// NuGet's and Maven's tools on macOS trust the system's roots alone, so
/// their registries pass unseen.
pub const READ_ONLY_REGISTRIES: &[&str] = &[
    "registry.npmjs.org",
    "registry.yarnpkg.com",
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    "pypi.org",
    "files.pythonhosted.org",
    "rubygems.org",
];

/// Where the system keeps the certificates it trusts, by platform.
const SYSTEM_ROOTS: &[&str] = &[
    "/etc/ssl/cert.pem",
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
];

/// Writes the proxy's authority into the task's tmp, alone and after the
/// system's roots, and names them in the variables tools read: Node adds
/// `NODE_EXTRA_CA_CERTS` to its own roots, the others replace theirs.
fn trust(task: &Task, ca_pem: &str) -> Result<Vec<(String, OsString)>> {
    let tmp = task.tmp()?;
    let ca = tmp.join("kule-ca.pem");
    std::fs::write(&ca, ca_pem).at(&ca)?;
    let mut bundle_text = SYSTEM_ROOTS
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    if !bundle_text.is_empty() && !bundle_text.ends_with('\n') {
        bundle_text.push('\n');
    }
    bundle_text.push_str(ca_pem);
    let bundle = tmp.join("kule-ca-bundle.pem");
    std::fs::write(&bundle, bundle_text).at(&bundle)?;
    let mut env = vec![("NODE_EXTRA_CA_CERTS".to_owned(), ca.into_os_string())];
    for var in [
        "SSL_CERT_FILE",
        "CURL_CA_BUNDLE",
        "REQUESTS_CA_BUNDLE",
        "GIT_SSL_CAINFO",
        "AWS_CA_BUNDLE",
        "CARGO_HTTP_CAINFO",
        "PIP_CERT",
    ] {
        env.push((var.to_owned(), bundle.clone().into_os_string()));
    }
    Ok(env)
}

struct Gate {
    policy: Policy,
    log: Mutex<File>,
    /// Judgements waiting for their outcome, with the person's answer if
    /// one was asked, by the client's address.
    pending: Mutex<HashMap<SocketAddr, Pending>>,
    asker: Option<Asker>,
    /// Looks inside TLS to the hosts the task's secrets are for.
    interceptor: Option<Interceptor>,
}

/// A judged connection waiting for its outcome: the person's answer if one
/// was asked, and the secrets put into it.
type Pending = (Judgement, Option<Answer>, Vec<String>);

impl lonca_net::Gate for Gate {
    fn decide(&self, request: &lonca_net::Request<'_>) -> lonca_net::Verdict {
        let judgement = self.policy.judge_connection(request.host, request.port);
        let rules = judgement.rules.join(", ");
        // Each connection has its own thread, so waiting here holds only
        // this one.
        let answer = match (&self.asker, judgement.verdict) {
            (Some(asker), Verdict::Ask) => Some(asker.approvals.ask(
                asker.source,
                &asker.task,
                request.host,
                request.port,
                &judgement.rules,
                asker.timeout,
            )),
            _ => None,
        };
        let mut allow = judgement.verdict == Verdict::Allow || answer == Some(Answer::Approved);
        let mut reason = match (judgement.verdict, answer) {
            (Verdict::Allow, _) | (_, Some(Answer::Approved)) => String::new(),
            (Verdict::Ask, None) => format!("waits for approval ({rules})"),
            (Verdict::Ask, Some(Answer::Denied)) => format!("denied by a person ({rules})"),
            (Verdict::Ask, Some(Answer::TimedOut)) => {
                format!("nobody approved it in time ({rules}); skip this step and go on without it")
            }
            (Verdict::Deny, _) => format!("denied ({rules})"),
        };
        // Loopback by name only, and only when the policy let it pass; the
        // rest of the LAN stays out of reach. Before anything connects, the
        // name is resolved here too and judged by Kule's own stricter
        // check (see `relay` for the address actually reached).
        let allow_private = allow && loopback_by_name(request.host);
        if allow && !allow_private && !resolves_public(request.host, request.port) {
            allow = false;
            reason = "addresses on this computer or its network are not reachable".to_owned();
        }
        lock(&self.pending).insert(request.peer, (judgement, answer, Vec::new()));
        lonca_net::Verdict {
            allow,
            allow_private,
            reason,
        }
    }

    fn relay(
        &self,
        request: &lonca_net::Request<'_>,
        tunnel: lonca_net::Tunnel,
    ) -> Option<lonca_net::Tunnel> {
        // The address the proxy connected to, judged before a byte goes
        // either way: a name may answer differently the second time.
        if !may_reach(request.host, &tunnel.upstream) {
            let _ = tunnel.upstream.shutdown(std::net::Shutdown::Both);
            let _ = tunnel.client.shutdown(std::net::Shutdown::Both);
            return None;
        }
        let Some(interceptor) = self
            .interceptor
            .as_ref()
            .filter(|i| i.wants(request.host, request.port))
        else {
            return Some(tunnel);
        };
        // A request that fails here fails for the client too; nothing is
        // passed on half filled.
        // What went in before an error went out all the same.
        let mut used = std::collections::BTreeSet::new();
        let _ = interceptor.serve_noting(request.host, tunnel, &mut used);
        if let Some(pending) = lock(&self.pending).get_mut(&request.peer) {
            pending.2 = used.into_iter().collect();
        }
        None
    }

    fn finished(&self, request: &lonca_net::Request<'_>, outcome: lonca_net::Outcome) {
        let Some((judgement, answer, secrets)) = lock(&self.pending).remove(&request.peer) else {
            return;
        };
        let record = Connection {
            time_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            host: request.host.to_owned(),
            port: request.port,
            verdict: judgement.verdict,
            rules: judgement.rules,
            outcome: format!("{outcome:?}").to_lowercase(),
            answer,
            secrets,
        };
        if let Ok(mut line) = serde_json::to_vec(&record) {
            line.push(b'\n');
            let _ = lock(&self.log).write_all(&line);
        }
    }
}

/// A poisoned lock only means another connection's thread panicked; the
/// data behind it is still usable for logging.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `ip` is on the public internet: not this computer, its network,
/// a link, a documentation or benchmarking range, multicast or reserved,
/// nor any of those inside an IPv6 address (mapped, NAT64, or the old
/// IPv4-compatible form). Stricter than the proxy library's own check,
/// which Kule's comes before and after.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            // NAT64 (64:ff9b::/96) carries an IPv4 address at its end.
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                let [a, b] = s[6].to_be_bytes();
                let [c, d] = s[7].to_be_bytes();
                return public_v4(Ipv4Addr::new(a, b, c, d));
            }
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || (s[0] & 0xffc0) == 0xfec0 // site local
                || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                || s[..6] == [0, 0, 0, 0, 0, 0]) // IPv4-compatible, deprecated
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b)) // shared address space
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (18..20).contains(&b)) // benchmarking
        || a >= 240) // reserved
}

/// Whether every address `host` resolves to is public. A name that does not
/// resolve is left to the proxy, which says so.
fn resolves_public(host: &str, port: u16) -> bool {
    use std::net::ToSocketAddrs;
    (host, port)
        .to_socket_addrs()
        .map_or(true, |mut addrs| addrs.all(|a| public_ip(a.ip())))
}

/// Whether a tunnel asked for by `host` may go on to where `upstream` is
/// connected: a public address, or this computer named as such. An address
/// that cannot be read may not.
fn may_reach(host: &str, upstream: &std::net::TcpStream) -> bool {
    loopback_by_name(host) || upstream.peer_addr().is_ok_and(|a| public_ip(a.ip()))
}

/// This computer, named as such: the one private destination a policy can
/// let a command reach.
fn loopback_by_name(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tunnel that ended up on this computer goes nowhere unless this
    /// computer is what it asked for by name.
    #[test]
    fn a_tunnel_reaches_a_private_address_only_when_asked_by_name() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        assert!(!may_reach("example.com", &upstream));
        assert!(!may_reach("127.0.0.2", &upstream));
        assert!(may_reach("localhost", &upstream));
        assert!(may_reach("127.0.0.1", &upstream));
    }

    #[test]
    fn addresses_on_this_computer_or_its_network_are_not_public() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "192.0.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "64:ff9b::7f00:1",
            "::127.0.0.1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
        ] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
