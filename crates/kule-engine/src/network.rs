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

use crate::approvals::{AfterApproval, Answer, Approvals, MAX_WAITING};
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
    /// `refused`, `unresolved`, `private`, `unreachable`, `tunneled`,
    /// `cut` (see `cut`) or `failed` (see `failed`).
    pub outcome: String,
    /// For a connection put to a person, how it was decided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// The secrets the proxy put into the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<String>,
    /// Why Kule cut a connection it had let through, once it saw what went
    /// over it: a TLS hello for another host, a request for another host,
    /// a request Kule refused inside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cut: Option<String>,
    /// Why a connection Kule let through ended early on its own (the host
    /// or the client went away, an error): not Kule's doing, and not one
    /// that went through as asked either.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// Why Kule refused a connection the policy or a person let through,
    /// before it connected: the address the host resolves to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused: Option<String>,
}

impl Connection {
    /// How it ended, in Kule's words, for one that did not go through as
    /// asked; `None` for one that did, or that the policy refused (its
    /// rules say why).
    pub fn told(&self) -> Option<String> {
        let said = |what: &str, why: &Option<String>| match why {
            Some(why) => format!("{what}: {why}"),
            None => what.to_owned(),
        };
        Some(match self.outcome.as_str() {
            "cut" => said("cut by Kule", &self.cut),
            "failed" => said("failed on its own", &self.failed),
            "refused" if self.refused.is_some() => said("refused by Kule", &self.refused),
            "unresolved" => "the host's name did not resolve".to_owned(),
            "private" => {
                "refused by Kule: every address the host resolves to is on this computer or its network"
                    .to_owned()
            }
            "unreachable" => "the host did not accept the connection".to_owned(),
            _ => return None,
        })
    }
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
    // The tmp is the sandbox's to write to: a link put at either name is
    // replaced, never written through.
    crate::fsops::put_beneath(&tmp, "kule-ca.pem", ca_pem.as_bytes(), 0o644)?;
    let mut bundle_text = SYSTEM_ROOTS
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    if !bundle_text.is_empty() && !bundle_text.ends_with('\n') {
        bundle_text.push('\n');
    }
    bundle_text.push_str(ca_pem);
    let bundle = tmp.join("kule-ca-bundle.pem");
    crate::fsops::put_beneath(&tmp, "kule-ca-bundle.pem", bundle_text.as_bytes(), 0o644)?;
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

/// A judged connection waiting for its outcome.
struct Pending {
    judgement: Judgement,
    /// The person's answer, if one was asked.
    answer: Option<Answer>,
    /// The secrets put into it.
    secrets: Vec<String>,
    /// Why Kule cut it, if it did.
    cut: Option<String>,
    /// Why it failed on its own, if it did.
    failed: Option<String>,
    /// Why Kule refused it before it connected, if it did.
    refused: Option<String>,
}

impl lonca_net::Gate for Gate {
    fn decide(&self, request: &lonca_net::Request<'_>) -> lonca_net::Verdict {
        // `Host.` and `HOST` are the host: judged, asked about and looked
        // into as one.
        let host = normal_host(request.host);
        let judgement = self.policy.judge_connection(&host, request.port);
        let rules = judgement.rules.join(", ");
        // Each connection has its own thread, so waiting here holds only
        // this one.
        let answer = match (&self.asker, judgement.verdict) {
            (Some(asker), Verdict::Ask) => Some(
                asker.approvals.ask(
                    asker.source,
                    &asker.task,
                    &host,
                    request.port,
                    &judgement.rules,
                    // What the card says the person approves: only the tunnel,
                    // or what Kule does inside it.
                    self.interceptor
                        .as_ref()
                        .and_then(|i| i.inside(&host, request.port)),
                    asker.timeout,
                ),
            ),
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
            (Verdict::Ask, Some(Answer::Withdrawn)) => {
                format!("refused by Kule: the run ended before anyone answered ({rules})")
            }
            (Verdict::Ask, Some(Answer::TooMany)) => format!(
                "refused by Kule without asking: {MAX_WAITING} questions of this run are waiting already ({rules}); skip this step and go on without it"
            ),
            (Verdict::Deny, _) => format!("denied ({rules})"),
        };
        // Loopback by name only, and only when the policy let it pass; the
        // rest of the LAN stays out of reach. Before anything connects, the
        // name is resolved here too and judged by Kule's own stricter
        // check (see `relay` for the address actually reached).
        let allow_private = allow && loopback_by_name(&host);
        let mut refused = None;
        if allow
            && !allow_private
            && let Some(address) = private_address(&host, request.port)
        {
            allow = false;
            // The address is the person's to see: the command is told only
            // that such addresses are out of reach, nothing of the network.
            reason = "addresses on this computer or its network are not reachable".to_owned();
            let named = if host.parse::<std::net::IpAddr>().is_ok() {
                format!("{host} is")
            } else {
                format!("{host} resolves to {address},")
            };
            refused = Some(format!(
                "{named} an address on this computer or its network, which commands may not reach"
            ));
        }
        lock(&self.pending).insert(
            request.peer,
            Pending {
                judgement,
                answer,
                secrets: Vec::new(),
                cut: None,
                failed: None,
                refused,
            },
        );
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
        let host = normal_host(request.host);
        if !may_reach(&host, &tunnel.upstream) {
            let _ = tunnel.upstream.shutdown(std::net::Shutdown::Both);
            let _ = tunnel.client.shutdown(std::net::Shutdown::Both);
            self.note_cut(request, UNREACHABLE_CUT.to_owned());
            return None;
        }
        let Some(interceptor) = self
            .interceptor
            .as_ref()
            .filter(|i| i.wants(&host, request.port))
        else {
            // Not looked into: its TLS hello must ask for the host it was
            // opened to (see `clienthello`).
            if let Err(why) = pass_checked(&host, request.port, tunnel) {
                self.note_cut(request, why);
            }
            return None;
        };
        // A request that fails here fails for the client too; nothing is
        // passed on half filled.
        // What went in before an error went out all the same.
        let mut used = std::collections::BTreeSet::new();
        let served = interceptor.serve_noting(&host, tunnel, &mut used);
        self.record_served(request, served, used);
        None
    }

    fn relay_plain(
        &self,
        request: &lonca_net::Request<'_>,
        head: String,
        tunnel: lonca_net::Tunnel,
    ) -> Option<(String, lonca_net::Tunnel)> {
        let host = normal_host(request.host);
        // As a tunnel's: the address reached, judged before a byte goes.
        if !may_reach(&host, &tunnel.upstream) {
            let _ = tunnel.upstream.shutdown(std::net::Shutdown::Both);
            let _ = tunnel.client.shutdown(std::net::Shutdown::Both);
            self.note_cut(request, UNREACHABLE_CUT.to_owned());
            return None;
        }
        // Every request on the connection is read, and each must be for
        // the host it was let through to.
        match crate::tls::serve_plain(&host, request.port, head, tunnel) {
            Ok(()) => {}
            Err(crate::tls::Ended::Refused(why)) => self.note_cut(request, why),
            Err(crate::tls::Ended::Failed(why)) => self.note_failed(request, why),
        }
        None
    }

    fn finished(&self, request: &lonca_net::Request<'_>, outcome: lonca_net::Outcome) {
        let Some(Pending {
            judgement,
            answer,
            secrets,
            cut,
            failed,
            refused,
        }) = lock(&self.pending).remove(&request.peer)
        else {
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
            outcome: if cut.is_some() {
                "cut".to_owned()
            } else if failed.is_some() {
                "failed".to_owned()
            } else {
                format!("{outcome:?}").to_lowercase()
            },
            answer,
            secrets,
            cut,
            failed,
            refused,
        };
        if let Ok(mut line) = serde_json::to_vec(&record) {
            line.push(b'\n');
            let _ = lock(&self.log).write_all(&line);
        }
        // A connection the person approved that did not go through as
        // approved: told where they answered, while the run is live.
        if let (Some(asker), Some(Answer::Approved), Some(why)) =
            (&self.asker, record.answer, record.told())
        {
            asker.approvals.after_approval(&AfterApproval {
                task: asker.task.clone(),
                host: normal_host(&record.host),
                port: record.port,
                why,
            });
        }
    }
}

impl Gate {
    fn note_cut(&self, request: &lonca_net::Request<'_>, why: String) {
        if let Some(pending) = lock(&self.pending).get_mut(&request.peer) {
            pending.cut = Some(why);
        }
    }

    fn note_failed(&self, request: &lonca_net::Request<'_>, why: String) {
        if let Some(pending) = lock(&self.pending).get_mut(&request.peer) {
            pending.failed = Some(why);
        }
    }

    /// Notes how a tunnel the interceptor served ended: the secrets that
    /// went into it, and a request Kule refused (a cut, with why) or an
    /// error (failed) apart from one that went through.
    fn record_served(
        &self,
        request: &lonca_net::Request<'_>,
        served: crate::error::Result<crate::tls::Served>,
        used: std::collections::BTreeSet<String>,
    ) {
        if let Some(pending) = lock(&self.pending).get_mut(&request.peer) {
            pending.secrets = used.into_iter().collect();
        }
        match served {
            Ok(crate::tls::Served { refused: None }) => {}
            Ok(crate::tls::Served { refused: Some(why) }) => {
                self.note_cut(request, format!("Kule refused a request in it: {why}"));
            }
            Err(e) => self.note_failed(request, e.to_string()),
        }
    }
}

/// A host as the proxy judges it: in lower case, without the dot a fully
/// qualified name may end in.
pub(crate) fn normal_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// Whether a tunnel to `host` on `port` may go on, by what its client
/// sent first: `read` of it so far. `None` while the hello is not whole.
/// TLS must ask for `host` by name (none for an address), and not hide the
/// name it asks for; port 443 carries nothing but TLS. Elsewhere what is
/// not TLS (ssh, say) goes on.
fn judge_first(
    host: &str,
    port: u16,
    read: &crate::clienthello::Read,
) -> Option<std::result::Result<(), String>> {
    use crate::clienthello::Read;
    Some(match read {
        Read::More => return None,
        Read::NotTls if port == 443 => Err(format!(
            "what it sent to {host}:443 is not TLS, which Kule passes there unread"
        )),
        Read::NotTls => Ok(()),
        Read::Bad(why) => Err(format!("its TLS hello to {host} has {why}")),
        Read::Hello(hello) if hello.hidden => Err(format!(
            "its TLS hello to {host} hides the name it asks for (Encrypted Client Hello), which Kule cannot check; in Chromium, turn it off with --disable-features=EncryptedClientHello"
        )),
        Read::Hello(hello) => {
            let address = host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok();
            match &hello.names[..] {
                [] if address => Ok(()),
                [one] if !address && normal_host(one) == host => Ok(()),
                [] => Err(format!(
                    "its TLS hello to {host} asks for no name, so the host could serve any"
                )),
                names => Err(format!(
                    "its TLS hello asks for {}, and this tunnel is to {host}",
                    names.join(", ")
                )),
            }
        }
    })
}

/// Relays a tunnel to `host` Kule does not look inside, once what its
/// client sends first passes [`judge_first`]: nothing of it reaches the
/// host before. What the host sends goes back from the start, as some
/// protocols speak first. Cut, and why, otherwise.
fn pass_checked(
    host: &str,
    port: u16,
    tunnel: lonca_net::Tunnel,
) -> std::result::Result<(), String> {
    use std::io::Read;
    use std::net::Shutdown;
    let lonca_net::Tunnel {
        client,
        upstream,
        early,
    } = tunnel;
    let cut = |why: String| {
        let _ = client.shutdown(Shutdown::Both);
        let _ = upstream.shutdown(Shutdown::Both);
        Err(why)
    };
    let (Ok(mut from_host), Ok(mut to_client)) = (upstream.try_clone(), client.try_clone()) else {
        return cut(format!("the tunnel to {host} could not be relayed"));
    };
    let back = std::thread::spawn(move || {
        let _ = std::io::copy(&mut from_host, &mut to_client);
        let _ = to_client.shutdown(Shutdown::Write);
    });
    let mut first = early;
    let judged = loop {
        if let Some(judged) = judge_first(host, port, &crate::clienthello::read(&first)) {
            break judged;
        }
        let mut chunk = [0u8; 4096];
        match (&client).read(&mut chunk) {
            Ok(n) if n > 0 => first.extend_from_slice(&chunk[..n]),
            // Gone before it said anything: nothing to pass on.
            _ if first.is_empty() => break Ok(()),
            _ => break Err(format!("its TLS hello to {host} was cut short")),
        }
    };
    if let Err(why) = judged {
        let result = cut(why);
        let _ = back.join();
        return result;
    }
    let mut to_host = &upstream;
    if !first.is_empty() && to_host.write_all(&first).is_ok() {
        let _ = std::io::copy(&mut &client, &mut to_host);
    }
    let _ = upstream.shutdown(Shutdown::Write);
    let _ = back.join();
    Ok(())
}

/// A poisoned lock only means another connection's thread panicked; the
/// data behind it is still usable for logging.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `ip` is on the public internet: not this computer, its network,
/// a link, a documentation or benchmarking range, multicast or reserved,
/// nor any of those inside an IPv6 address (mapped, NAT64, 6to4). For IPv6
/// only the global unicast range (`2000::/3`) is public, less what the
/// IETF keeps there: its protocol assignments (Teredo among them, which
/// carries another address), documentation, and 6to4, judged by the IPv4
/// address it carries. Stricter than the proxy library's own check, which
/// Kule's comes before and after.
pub fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            let v4_at = |i: usize| {
                let [a, b] = s[i].to_be_bytes();
                let [c, d] = s[i + 1].to_be_bytes();
                Ipv4Addr::new(a, b, c, d)
            };
            // NAT64 (64:ff9b::/96) carries an IPv4 address at its end.
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return public_v4(v4_at(6));
            }
            // 6to4 (2002::/16) carries one after its prefix.
            if s[0] == 0x2002 {
                return public_v4(v4_at(1));
            }
            (s[0] & 0xe000) == 0x2000 // global unicast
                && !(s[0] == 0x2001 && s[1] < 0x0200) // IETF protocol assignments
                && !(s[0] == 0x2001 && s[1] == 0x0db8) // documentation
                && !(s[0] == 0x3fff && s[1] < 0x1000) // documentation
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

/// An address `host` resolves to that is not public, if any. A name that
/// does not resolve is left to the proxy, which says so.
fn private_address(host: &str, port: u16) -> Option<std::net::IpAddr> {
    use std::net::ToSocketAddrs;
    (host, port)
        .to_socket_addrs()
        .ok()?
        .map(|a| a.ip())
        .find(|ip| !public_ip(*ip))
}

/// Why a connection is cut that reached an address [`may_reach`] refuses.
const UNREACHABLE_CUT: &str =
    "it reached an address on this computer or its network, which is not reachable";

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

    /// A tunnel Kule looked into is logged as what became of it: cut, with
    /// why, when Kule refused a request in it; failed, with why, when it
    /// ended in an error; tunneled only when every request went through.
    /// The secrets that went in are kept either way.
    #[test]
    fn a_tunnel_kule_looked_into_is_logged_as_what_became_of_it() {
        use lonca_net::Gate as _;
        let dir = tempfile::TempDir::new().unwrap();
        let log_path = dir.path().join("network.jsonl");
        let gate = Gate {
            policy: Policy::from_texts(crate::DEFAULT_POLICY, None).unwrap(),
            log: Mutex::new(File::create(&log_path).unwrap()),
            pending: Mutex::new(HashMap::new()),
            asker: None,
            interceptor: None,
        };
        let ends: [crate::error::Result<crate::tls::Served>; 3] = [
            Ok(crate::tls::Served {
                refused: Some(crate::tls::REFUSED_WRITE),
            }),
            Err(Error::Invalid(
                "api.example: closed without a response".into(),
            )),
            Ok(crate::tls::Served { refused: None }),
        ];
        for (n, served) in ends.into_iter().enumerate() {
            let peer: SocketAddr = format!("127.0.0.1:{}", 40000 + n).parse().unwrap();
            let request = lonca_net::Request {
                host: "api.example",
                port: 443,
                peer,
                local: "127.0.0.1:1".parse().unwrap(),
            };
            lock(&gate.pending).insert(
                peer,
                Pending {
                    judgement: gate.policy.judge_connection("api.example", 443),
                    answer: None,
                    secrets: Vec::new(),
                    cut: None,
                    failed: None,
                    refused: None,
                },
            );
            gate.record_served(&request, served, ["API_KEY".to_owned()].into());
            gate.finished(&request, lonca_net::Outcome::Tunneled);
        }
        let logged: Vec<Connection> = std::fs::read_to_string(&log_path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        // What became of each: outcome, cut, failed, secrets.
        type Seen<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a [String]);
        let seen: Vec<Seen> = logged
            .iter()
            .map(|c| {
                (
                    c.outcome.as_str(),
                    c.cut.as_deref(),
                    c.failed.as_deref(),
                    c.secrets.as_slice(),
                )
            })
            .collect();
        let key = ["API_KEY".to_owned()];
        assert_eq!(
            seen,
            [
                (
                    "cut",
                    Some(
                        "Kule refused a request in it: a write to a host commands may only read from"
                    ),
                    None,
                    &key[..]
                ),
                (
                    "failed",
                    None,
                    Some("api.example: closed without a response"),
                    &key[..]
                ),
                ("tunneled", None, None, &key[..]),
            ]
        );
    }

    /// A plain HTTP connection is logged as cut, with why, when Kule
    /// refused a request on it (one for another host), and as failed when
    /// the host went away without an answer: Kule did not cut that one.
    #[test]
    fn a_plain_connection_is_logged_as_cut_by_kule_or_failed_on_its_own() {
        use lonca_net::Gate as _;
        use std::net::{TcpListener, TcpStream};
        let dir = tempfile::TempDir::new().unwrap();
        let log_path = dir.path().join("network.jsonl");
        let gate = Gate {
            policy: Policy::from_texts(crate::DEFAULT_POLICY, None).unwrap(),
            log: Mutex::new(File::create(&log_path).unwrap()),
            pending: Mutex::new(HashMap::new()),
            asker: None,
            interceptor: None,
        };
        for head in [
            "GET / HTTP/1.1\r\nHost: other.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost\r\n\r\n",
        ] {
            // A host that takes the connection and closes it unanswered.
            let host = TcpListener::bind("127.0.0.1:0").unwrap();
            let upstream = TcpStream::connect(host.local_addr().unwrap()).unwrap();
            drop(host.accept().unwrap());
            let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
            let _client = TcpStream::connect(proxy.local_addr().unwrap()).unwrap();
            let (client, peer) = proxy.accept().unwrap();
            let request = lonca_net::Request {
                host: "localhost",
                port: 80,
                peer,
                local: proxy.local_addr().unwrap(),
            };
            lock(&gate.pending).insert(
                peer,
                Pending {
                    judgement: gate.policy.judge_connection("localhost", 80),
                    answer: None,
                    secrets: Vec::new(),
                    cut: None,
                    failed: None,
                    refused: None,
                },
            );
            let tunnel = lonca_net::Tunnel {
                client,
                upstream,
                early: Vec::new(),
            };
            assert!(
                gate.relay_plain(&request, head.to_owned(), tunnel)
                    .is_none()
            );
            gate.finished(&request, lonca_net::Outcome::Tunneled);
        }
        let logged: Vec<Connection> = std::fs::read_to_string(&log_path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(logged.len(), 2);
        assert_eq!(logged[0].outcome, "cut", "{:?}", logged[0]);
        assert!(logged[0].cut.is_some() && logged[0].failed.is_none());
        assert_eq!(logged[1].outcome, "failed", "{:?}", logged[1]);
        // Unanswered or reset: either way the host's doing, not Kule's.
        assert!(
            logged[1].failed.is_some() && logged[1].cut.is_none(),
            "{:?}",
            logged[1]
        );
    }

    /// A connection let through by name that reached an address on this
    /// computer or its network is shut, and recorded as cut, with why:
    /// never as a tunnel that went through.
    #[test]
    fn a_connection_that_reached_a_private_address_is_recorded_as_cut() {
        use lonca_net::Gate as _;
        use std::net::{TcpListener, TcpStream};
        let dir = tempfile::TempDir::new().unwrap();
        let log_path = dir.path().join("network.jsonl");
        let rule = r#"@id("t") permit (principal, action == Kule::Action::"auto", resource) when { resource.kind == "net.connect" && resource.host == "rebind.example" };"#;
        let policy =
            Policy::from_texts(&format!("{}\n{rule}", crate::DEFAULT_POLICY), None).unwrap();
        let gate = Gate {
            policy,
            log: Mutex::new(File::create(&log_path).unwrap()),
            pending: Mutex::new(HashMap::new()),
            asker: None,
            interceptor: None,
        };
        for plain in [false, true] {
            let local = TcpListener::bind("127.0.0.1:0").unwrap();
            let upstream = TcpStream::connect(local.local_addr().unwrap()).unwrap();
            let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
            let _client = TcpStream::connect(proxy.local_addr().unwrap()).unwrap();
            let (client, peer) = proxy.accept().unwrap();
            let request = lonca_net::Request {
                host: "rebind.example",
                port: 443,
                peer,
                local: proxy.local_addr().unwrap(),
            };
            let judgement = gate.policy.judge_connection("rebind.example", 443);
            assert_eq!(judgement.verdict, Verdict::Allow);
            lock(&gate.pending).insert(
                peer,
                Pending {
                    judgement,
                    answer: None,
                    secrets: Vec::new(),
                    cut: None,
                    failed: None,
                    refused: None,
                },
            );
            let tunnel = lonca_net::Tunnel {
                client,
                upstream,
                early: Vec::new(),
            };
            if plain {
                let head = "GET / HTTP/1.1\r\nHost: rebind.example\r\n\r\n".to_owned();
                assert!(gate.relay_plain(&request, head, tunnel).is_none());
            } else {
                assert!(gate.relay(&request, tunnel).is_none());
            }
            gate.finished(&request, lonca_net::Outcome::Tunneled);
        }
        let text = std::fs::read_to_string(&log_path).unwrap();
        let records: Vec<Connection> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(records.len(), 2, "{text}");
        for record in records {
            assert_eq!(record.outcome, "cut", "{text}");
            assert!(
                record
                    .cut
                    .is_some_and(|why| why.contains("an address on this computer")),
                "{text}"
            );
        }
    }

    /// What a client sends first decides whether a tunnel Kule does not
    /// look inside goes on: TLS asking for its host by name (any case, a
    /// final dot or not), none for an address; never a hidden name; on
    /// 443 nothing but TLS.
    #[test]
    fn a_tunnel_goes_on_only_where_its_hello_asks_for_its_host() {
        use crate::clienthello::{Hello, Read};
        let hello = |names: &[&str], hidden| {
            Read::Hello(Hello {
                names: names.iter().map(|n| (*n).to_owned()).collect(),
                hidden,
            })
        };
        let ok = |host: &str, port, read: Read| judge_first(host, port, &read) == Some(Ok(()));
        assert!(ok(
            "registry.npmjs.org",
            443,
            hello(&["Registry.NPMJS.org."], false)
        ));
        assert!(ok("93.184.216.34", 443, hello(&[], false)));
        assert!(ok("github.com", 22, Read::NotTls));
        assert!(!ok(
            "registry.npmjs.org",
            443,
            hello(&["evil.example"], false)
        ));
        assert!(!ok("registry.npmjs.org", 443, hello(&[], false)));
        assert!(!ok("93.184.216.34", 443, hello(&["evil.example"], false)));
        assert!(!ok(
            "registry.npmjs.org",
            443,
            hello(&["registry.npmjs.org"], true)
        ));
        assert!(!ok(
            "registry.npmjs.org",
            443,
            hello(&["registry.npmjs.org", "evil.example"], false)
        ));
        assert!(!ok("registry.npmjs.org", 443, Read::NotTls));
        assert!(!ok(
            "registry.npmjs.org",
            8443,
            hello(&["evil.example"], false)
        ));
        assert!(!ok("registry.npmjs.org", 443, Read::Bad("x")));
        assert_eq!(judge_first("a.example", 443, &Read::More), None);
        assert_eq!(normal_host("Registry.NPMJS.org."), "registry.npmjs.org");
    }

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
            // IPv4 inside IPv6 by 6to4 and Teredo, NAT64 for local use,
            // the discard prefix, and the IETF's own ranges.
            "2002:7f00:1::1",
            "2002:a00:1::1",
            "2001:0:4136:e378:8000:63bf:80ff:fffe",
            "2001:2::1",
            "64:ff9b:1::a00:1",
            "100::1",
            "3fff::1",
            "4000::1",
            "1000::1",
        ] {
            assert!(!public_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in [
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "2a00:1450:4001:80b::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
            "2002:808:808::1",
        ] {
            assert!(public_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
