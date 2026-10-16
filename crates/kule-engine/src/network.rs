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
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::approvals::{Answer, Approvals};
use crate::error::{Error, IoContext, Result};
use crate::policy::{Judgement, Policy, Verdict};
use crate::task::Task;

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
}

/// The proxy of one task, serving until it is dropped. Dropping it closes
/// the port, withdraws the questions it has waiting and cuts the
/// connections still open.
pub struct Proxy {
    server: Option<lonca_net::Server>,
    asker: Option<Asker>,
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
        Self::serve(task, policy, None)
    }

    /// [`Proxy::start`], putting the connections that wait for a person to
    /// `approvals` and holding each until it is answered or `timeout` runs
    /// out.
    pub fn start_asking(
        task: &Task,
        policy: Policy,
        approvals: &Approvals,
        timeout: Duration,
    ) -> Result<Proxy> {
        let asker = Asker {
            approvals: approvals.clone(),
            source: approvals.new_source(),
            task: task.id().to_owned(),
            timeout,
        };
        Self::serve(task, policy, Some(asker))
    }

    fn serve(task: &Task, policy: Policy, asker: Option<Asker>) -> Result<Proxy> {
        let addr = Path::new("127.0.0.1:0");
        let listener = lonca_net::bind("127.0.0.1:0").at(addr)?;
        let path = log_path(task);
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .at(&path)?;
        let gate = Arc::new(Gate {
            policy,
            log: Mutex::new(log),
            pending: Mutex::new(HashMap::new()),
            asker: asker.clone(),
        });
        let server = lonca_net::spawn(listener, gate).at(addr)?;
        Ok(Proxy {
            server: Some(server),
            asker,
        })
    }

    pub fn port(&self) -> u16 {
        self.server.as_ref().map_or(0, |s| s.local_addr().port())
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

struct Gate {
    policy: Policy,
    log: Mutex<File>,
    /// Judgements waiting for their outcome, with the person's answer if
    /// one was asked, by the client's address.
    pending: Mutex<HashMap<SocketAddr, (Judgement, Option<Answer>)>>,
    asker: Option<Asker>,
}

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
        let allow = judgement.verdict == Verdict::Allow || answer == Some(Answer::Approved);
        let reason = match (judgement.verdict, answer) {
            (Verdict::Allow, _) | (_, Some(Answer::Approved)) => String::new(),
            (Verdict::Ask, None) => format!("waits for approval ({rules})"),
            (Verdict::Ask, Some(Answer::Denied)) => format!("denied by a person ({rules})"),
            (Verdict::Ask, Some(Answer::TimedOut)) => {
                format!("nobody approved it in time ({rules}); skip this step and go on without it")
            }
            (Verdict::Deny, _) => format!("denied ({rules})"),
        };
        lock(&self.pending).insert(request.peer, (judgement, answer));
        lonca_net::Verdict {
            allow,
            // Loopback by name only, and only when the policy let it pass;
            // the rest of the LAN stays out of reach.
            allow_private: allow && matches!(request.host, "localhost" | "127.0.0.1" | "::1"),
            reason,
        }
    }

    fn finished(&self, request: &lonca_net::Request<'_>, outcome: lonca_net::Outcome) {
        let Some((judgement, answer)) = lock(&self.pending).remove(&request.peer) else {
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
