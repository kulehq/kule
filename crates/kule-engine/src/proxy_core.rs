//! The proxy's core, which serves a task's connections wherever it runs:
//! in the daemon's own process on macOS and Linux, in `kule-proxy` inside
//! an app container on Windows (see [`crate::proxy_pipe`]). It judges each
//! connection by the policy, puts the ones the policy leaves to a person
//! through [`Ask`], looks inside TLS for the hosts the task's secrets go
//! to, and reports how every connection ended through [`Log`]; where the
//! task's files are, who the person is and how to reach them are the
//! caller's, never the core's.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::approvals::{Answer, Inside, MAX_WAITING};
use crate::error::Result;
use crate::network::{Connection, normal_host, public_ip};
use crate::policy::{Judgement, Policy, Verdict};
use crate::secrets::Vault;
use crate::tls::Interceptor;

/// What a proxy needs to serve a task, apart from where it runs: the
/// policy's rules as [`Policy::to_parts`] gives them, the secrets it holds
/// and the hosts commands may only read from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProxyConfig {
    pub policy: Vec<(String, String)>,
    pub vault: Vault,
    pub read_only: Vec<String>,
    /// Whether someone is there to ask: without, a connection the policy
    /// leaves to a person is refused at once.
    pub asks: bool,
}

/// Where the proxy puts a connection the policy leaves to a person, and
/// waits for the answer.
pub(crate) trait Ask: Send + Sync + 'static {
    fn ask(&self, host: &str, port: u16, rules: &[String], inside: Option<Inside>) -> Answer;
}

/// Where the proxy reports how a connection ended.
pub(crate) trait Log: Send + Sync + 'static {
    fn record(&self, connection: Connection);
}

/// A log kept as one line of JSON per connection: a task's `network.jsonl`.
pub(crate) struct FileLog(pub Mutex<File>);

impl Log for FileLog {
    fn record(&self, connection: Connection) {
        if let Ok(mut line) = serde_json::to_vec(&connection) {
            line.push(b'\n');
            let _ = lock(&self.0).write_all(&line);
        }
    }
}

/// Serves `config` on `listener` until the server is stopped, and gives
/// the authority its commands have to trust where it looks inside TLS.
pub(crate) fn serve(
    listener: std::net::TcpListener,
    config: ProxyConfig,
    asker: Option<Box<dyn Ask>>,
    log: Box<dyn Log>,
) -> Result<(lonca_net::Server, String)> {
    let policy = Policy::from_parts(&config.policy)?;
    let read_only = config.read_only.into_iter().collect();
    let interceptor = Interceptor::new(config.vault, crate::tls::web_roots(), read_only)?;
    let ca_pem = interceptor.ca_pem().to_owned();
    let gate = Arc::new(Gate {
        policy,
        log,
        pending: Mutex::new(HashMap::new()),
        asker: asker.filter(|_| config.asks),
        interceptor: Some(interceptor),
    });
    let addr = std::path::Path::new("127.0.0.1:0");
    let server = lonca_net::spawn(listener, gate).map_err(|e| crate::Error::Io {
        path: addr.to_path_buf(),
        source: e,
    })?;
    Ok((server, ca_pem))
}

struct Gate {
    policy: Policy,
    log: Box<dyn Log>,
    /// Judgements waiting for their outcome, with the person's answer if
    /// one was asked, by the client's address.
    pending: Mutex<HashMap<SocketAddr, Pending>>,
    asker: Option<Box<dyn Ask>>,
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
                asker.ask(
                    &host,
                    request.port,
                    &judgement.rules,
                    // What the card says the person approves: only the tunnel,
                    // or what Kule does inside it.
                    self.interceptor
                        .as_ref()
                        .and_then(|i| i.inside(&host, request.port)),
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
        self.log.record(record);
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
            "its TLS hello to {host} hides the name it asks for (Encrypted Client Hello), which Kule cannot check. \
             Chromium-based browsers send one to every host unless their policy EncryptedClientHelloEnabled is false \
             (the --disable-features switch no longer turns it off); Firefox unless security.tls.ech.grease_probability is 0. \
             What works per tool: Kule's troubleshooting page, \"A connection is cut for Encrypted Client Hello\""
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
    use crate::Error;

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
            log: Box::new(FileLog(Mutex::new(File::create(&log_path).unwrap()))),
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
            log: Box::new(FileLog(Mutex::new(File::create(&log_path).unwrap()))),
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
            log: Box::new(FileLog(Mutex::new(File::create(&log_path).unwrap()))),
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
        // A hidden name is told with what turns it off, as measured (#34):
        // the switch Chromium once had does nothing now.
        let why = judge_first("a.example", 443, &hello(&["a.example"], true))
            .unwrap()
            .unwrap_err();
        assert!(
            why.contains("EncryptedClientHelloEnabled is false"),
            "{why}"
        );
        assert!(
            why.contains("security.tls.ech.grease_probability is 0"),
            "{why}"
        );
        assert!(
            !why.contains("turn it off with --disable-features"),
            "{why}"
        );
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
}
