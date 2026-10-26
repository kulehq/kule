//! A task's network: a proxy that every sandboxed command has to go through.
//!
//! The sandbox lets commands connect only to this proxy (see
//! [`Network::Proxy`](crate::Network::Proxy)). Each connection is judged by
//! the policy as a `net.connect` effect and written to the task's
//! `network.jsonl`, refused ones included. Network effects cannot be rolled
//! back, so a connection goes through only when the policy lets it pass on
//! its own or a person approves it while the command waits (see
//! [`Approvals`]).

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::approvals::{AfterApproval, Answer, Approvals, Inside};
use crate::error::{Error, IoContext, Result};
use crate::policy::{Policy, Verdict};
use crate::proxy_core::{self, Ask, FileLog, Log, ProxyConfig};
use crate::secrets::AgentKey;
use crate::task::Task;

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
    served: Option<Served>,
    asker: Option<Asker>,
    env: Vec<(String, OsString)>,
}

/// Where a proxy serves.
enum Served {
    /// In this process: macOS and Linux.
    #[cfg(not(windows))]
    Here(lonca_net::Server),
    /// In a process of its own, `kule-proxy`, reached over its pipes: on
    /// Windows inside an app container ([`crate::proxy_pipe`]).
    Child(crate::proxy_pipe::ChildProxy),
}

/// Where a proxy puts the connections that wait for a person.
#[derive(Clone)]
struct Asker {
    approvals: Approvals,
    source: u64,
    task: String,
    timeout: Duration,
}

impl proxy_core::Ask for Asker {
    fn ask(&self, host: &str, port: u16, rules: &[String], inside: Option<Inside>) -> Answer {
        self.approvals.ask(
            self.source,
            &self.task,
            host,
            port,
            rules,
            inside,
            self.timeout,
        )
    }
}

/// The task's `network.jsonl`, and the person told of a connection they
/// approved that did not go through as approved, where they answered.
struct Recorder {
    log: FileLog,
    asker: Option<Asker>,
}

impl proxy_core::Log for Recorder {
    fn record(&self, record: Connection) {
        let after = match (&self.asker, record.answer, record.told()) {
            (Some(asker), Some(Answer::Approved), Some(why)) => Some(AfterApproval {
                task: asker.task.clone(),
                host: normal_host(&record.host),
                port: record.port,
                why,
            }),
            _ => None,
        };
        self.log.record(record);
        // Told while the run is live, once the log has it.
        if let (Some(asker), Some(after)) = (&self.asker, after) {
            asker.approvals.after_approval(&after);
        }
    }
}

impl Proxy {
    /// Starts the proxy on a free loopback port. Connections the policy puts
    /// to a person are refused at once: nobody is there to ask.
    pub fn start(task: &Task, policy: Policy) -> Result<Proxy> {
        Self::serve(task, policy, None, &[], Self::run_here)
    }

    /// [`Proxy::start`], also putting an agent's `keys` into requests for it.
    pub fn start_with_keys(task: &Task, policy: Policy, keys: &[AgentKey]) -> Result<Proxy> {
        Self::serve(task, policy, None, keys, Self::run_here)
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
        let asker = Self::asker(task, approvals, timeout);
        Self::serve(task, policy, Some(asker), keys, Self::run_here)
    }

    /// [`Proxy::start_asking`] served by the `kule-proxy` program at `exe`,
    /// started as a plain process of this user's, with no container around
    /// it: how the pipes between Kule and its proxy are tested on every
    /// platform. Not for use beyond that.
    #[doc(hidden)]
    pub fn start_through(
        exe: &Path,
        task: &Task,
        policy: Policy,
        approvals: Option<(&Approvals, Duration)>,
        keys: &[AgentKey],
    ) -> Result<Proxy> {
        let asker = approvals.map(|(approvals, timeout)| Self::asker(task, approvals, timeout));
        let exe = exe.to_path_buf();
        Self::serve(task, policy, asker, keys, move |config, ask, log| {
            let child = crate::proxy_pipe::ChildProxy::spawn(
                || crate::proxy_pipe::launch_plain(&exe),
                config,
                ask,
                log,
            )?;
            let ca_pem = child.ca_pem().to_owned();
            Ok((Served::Child(child), ca_pem))
        })
    }

    fn asker(task: &Task, approvals: &Approvals, timeout: Duration) -> Asker {
        Asker {
            approvals: approvals.clone(),
            source: approvals.new_source(),
            task: task.id().to_owned(),
            timeout,
        }
    }

    /// Where a proxy serves on this platform: in this process on macOS and
    /// Linux, in `kule-proxy` inside an app container on Windows.
    #[cfg(not(windows))]
    fn run_here(
        config: ProxyConfig,
        ask: Option<Box<dyn Ask>>,
        log: Box<dyn Log>,
    ) -> Result<(Served, String)> {
        let addr = Path::new("127.0.0.1:0");
        let listener = lonca_net::bind("127.0.0.1:0").at(addr)?;
        let (server, ca_pem) = proxy_core::serve(listener, config, ask, log)?;
        Ok((Served::Here(server), ca_pem))
    }

    #[cfg(windows)]
    fn run_here(
        config: ProxyConfig,
        ask: Option<Box<dyn Ask>>,
        log: Box<dyn Log>,
    ) -> Result<(Served, String)> {
        let child = crate::proxy_pipe::ChildProxy::spawn(
            crate::windows::appcontainer::launch_proxy,
            config,
            ask,
            log,
        )?;
        let ca_pem = child.ca_pem().to_owned();
        Ok((Served::Child(child), ca_pem))
    }

    fn serve(
        task: &Task,
        policy: Policy,
        asker: Option<Asker>,
        keys: &[AgentKey],
        run: impl FnOnce(ProxyConfig, Option<Box<dyn Ask>>, Box<dyn Log>) -> Result<(Served, String)>,
    ) -> Result<Proxy> {
        let path = log_path(task);
        let log = crate::osfs::append_private(&path).at(&path)?;
        let mut vault = task.vault()?;
        vault.with_keys(keys)?;
        let mut env: Vec<(String, OsString)> = vault
            .env()
            .into_iter()
            .map(|(name, placeholder)| (name, placeholder.into()))
            .collect();
        // Looked into: the hosts secrets go to, and the registries, which
        // commands may read from but never publish to.
        let config = ProxyConfig {
            policy: policy.to_parts()?,
            vault,
            read_only: READ_ONLY_REGISTRIES
                .iter()
                .map(|h| (*h).to_owned())
                .collect(),
            asks: asker.is_some(),
        };
        let recorder = Box::new(Recorder {
            log: FileLog(Mutex::new(log)),
            asker: asker.clone(),
        });
        let ask = asker.clone().map(|asker| Box::new(asker) as Box<dyn Ask>);
        let (served, ca_pem) = run(config, ask, recorder)?;
        env.extend(trust(task, &ca_pem)?);
        Ok(Proxy {
            served: Some(served),
            asker,
            env,
        })
    }

    pub fn port(&self) -> u16 {
        match &self.served {
            #[cfg(not(windows))]
            Some(Served::Here(server)) => server.local_addr().port(),
            Some(Served::Child(child)) => child.port(),
            None => 0,
        }
    }

    /// The process serving this proxy when it is one of its own, for the
    /// tests that watch it end.
    #[doc(hidden)]
    pub fn child_pid(&self) -> Option<u32> {
        match &self.served {
            Some(Served::Child(child)) => Some(child.pid()),
            _ => None,
        }
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
        match self.served.take() {
            #[cfg(not(windows))]
            Some(Served::Here(server)) => server.stop(),
            Some(Served::Child(child)) => child.stop(),
            None => {}
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

/// Where the system keeps the certificates it trusts, on macOS and Linux.
#[cfg(not(windows))]
const SYSTEM_ROOTS: &[&str] = &[
    "/etc/ssl/cert.pem",
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
];

/// The certificates the system trusts, as PEM text; empty when they
/// cannot be read, so the bundle holds the proxy's authority alone.
fn system_roots() -> String {
    #[cfg(not(windows))]
    {
        SYSTEM_ROOTS
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
    }
    #[cfg(windows)]
    {
        crate::windows::roots::pem().unwrap_or_default()
    }
}

/// Writes the proxy's authority into the task's tmp, alone and after the
/// system's roots, and names them in the variables tools read: Node adds
/// `NODE_EXTRA_CA_CERTS` to its own roots, the others replace theirs.
fn trust(task: &Task, ca_pem: &str) -> Result<Vec<(String, OsString)>> {
    let tmp = task.tmp()?;
    let ca = tmp.join("kule-ca.pem");
    // The tmp is the sandbox's to write to: a link put at either name is
    // replaced, never written through.
    crate::fsops::put_beneath(&tmp, "kule-ca.pem", ca_pem.as_bytes(), 0o644)?;
    let mut bundle_text = system_roots();
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

/// A host as the proxy judges it: in lower case, without the dot a fully
/// qualified name may end in.
pub(crate) fn normal_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
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

#[cfg(test)]
mod tests {
    use super::*;

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
