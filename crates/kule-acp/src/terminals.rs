//! Commands an agent asks Kule to run for it (ACP's `terminal/*`), so their
//! output reaches the person as it comes. Each runs in the agent's own
//! sandbox, behind the same proxy, in the task's copy, in a session of its
//! own; everything still running when the run ends is stopped with it.

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use kule_engine::Sandbox;

use crate::{Event, RunControl};

/// The most output kept for one command, whatever the agent asks for.
pub const MAX_TERMINAL_OUTPUT: usize = 1 << 20;
/// How often output is passed on to the window while a command prints.
const EVERY: Duration = Duration::from_millis(200);

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<u32>,
    pub signal: Option<String>,
}

/// One command and what it printed.
struct Term {
    output: Mutex<Output>,
    exit: Mutex<Option<Exit>>,
    ended: tokio::sync::Notify,
    /// The session the command leads, to stop it and all it started.
    group: i32,
}

struct Output {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
    /// The tool call the agent shows this command in, once it says.
    call: Option<String>,
    sent: Option<Instant>,
}

impl Output {
    /// The output as text, from the first whole character.
    fn text(&self) -> String {
        let start = self
            .bytes
            .iter()
            .position(|b| (b & 0xC0) != 0x80)
            .unwrap_or(self.bytes.len());
        String::from_utf8_lossy(&self.bytes[start..]).into_owned()
    }
}

/// The commands of one run.
pub(crate) struct Terminals {
    sandbox: Sandbox,
    copy: PathBuf,
    next: AtomicU64,
    terms: Mutex<HashMap<String, Arc<Term>>>,
    /// The sessions of commands the agent is done with, stopped with the run.
    released: Mutex<Vec<i32>>,
    events: UnboundedSender<Event>,
    /// The run's: told of each command, and of what it prints.
    control: RunControl,
}

/// Variables an agent may not set for a command: what loads code into
/// every program it starts, and what the sandbox itself sets.
fn refused_variable(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("DYLD_")
        || upper.starts_with("LD_")
        || upper.ends_with("_PROXY")
        || matches!(
            upper.as_str(),
            "TMPDIR"
                | "HOME"
                | "NODE_USE_ENV_PROXY"
                | "SSL_CERT_FILE"
                | "NODE_EXTRA_CA_CERTS"
                | "REQUESTS_CA_BUNDLE"
                | "CURL_CA_BUNDLE"
                | "GIT_SSL_CAINFO"
        )
}

impl Terminals {
    pub(crate) fn new(
        sandbox: Sandbox,
        copy: PathBuf,
        events: UnboundedSender<Event>,
        control: RunControl,
    ) -> Self {
        Terminals {
            sandbox,
            copy,
            next: AtomicU64::new(1),
            terms: Mutex::new(HashMap::new()),
            released: Mutex::new(Vec::new()),
            events,
            control,
        }
    }

    /// Starts `program` with `args`; returns the terminal's id.
    pub(crate) fn create(
        self: &Arc<Self>,
        program: &str,
        args: &[String],
        env: &[(String, String)],
        cwd: Option<&Path>,
        limit: Option<u64>,
    ) -> Result<String, String> {
        if let Some((name, _)) = env.iter().find(|(n, _)| refused_variable(n)) {
            return Err(format!("{name} is set by Kule's sandbox, not by the agent"));
        }
        let cwd = match cwd {
            None => self.copy.clone(),
            Some(dir) => {
                let real =
                    std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                let copy = std::fs::canonicalize(&self.copy).map_err(|e| e.to_string())?;
                if !real.starts_with(&copy) {
                    return Err(format!(
                        "{}: commands run in the task's copy",
                        dir.display()
                    ));
                }
                real
            }
        };
        let mut command = self
            .sandbox
            .command(program, args)
            .map_err(|e| e.to_string())?;
        command
            .current_dir(&cwd)
            .envs(env.iter().map(|(n, v)| (n, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start {program}: {e}"))?;
        self.control.started_command(child.id() as i32);
        let id = format!("term-{}", self.next.fetch_add(1, Ordering::SeqCst));
        let limit = limit
            .map_or(MAX_TERMINAL_OUTPUT, |l| l as usize)
            .min(MAX_TERMINAL_OUTPUT);
        let term = Arc::new(Term {
            output: Mutex::new(Output {
                bytes: Vec::new(),
                limit,
                truncated: false,
                call: None,
                sent: None,
            }),
            exit: Mutex::new(None),
            ended: tokio::sync::Notify::new(),
            group: child.id() as i32,
        });
        self.terms
            .lock()
            .unwrap()
            .insert(id.clone(), Arc::clone(&term));
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut stream| {
            let (term, events) = (Arc::clone(&term), self.events.clone());
            let control = self.control.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    control.touch();
                    let mut out = term.output.lock().unwrap();
                    out.bytes.extend_from_slice(&buf[..n]);
                    if out.bytes.len() > out.limit {
                        let cut = out.bytes.len() - out.limit;
                        out.bytes.drain(..cut);
                        out.truncated = true;
                    }
                    if out.sent.is_none_or(|t| t.elapsed() >= EVERY) {
                        send(&events, &mut out);
                    }
                }
            })
        })
        .collect();
        let (term, events) = (Arc::clone(&term), self.events.clone());
        std::thread::spawn(move || {
            let status = child.wait();
            // What it printed last is read first; but a process it left in
            // the background (a server) can hold the output open for good,
            // and the command has ended all the same.
            let until = Instant::now() + Duration::from_millis(300);
            while !readers.iter().all(|r| r.is_finished()) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
            }
            send(&events, &mut term.output.lock().unwrap());
            *term.exit.lock().unwrap() = Some(exit_of(status));
            term.ended.notify_waiters();
        });
        Ok(id)
    }

    fn term(&self, id: &str) -> Result<Arc<Term>, String> {
        self.terms
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("no terminal {id}"))
    }

    /// What the command printed so far, whether some was cut from its
    /// start, and how it ended if it has.
    pub(crate) fn output(&self, id: &str) -> Result<(String, bool, Option<Exit>), String> {
        let term = self.term(id)?;
        let out = term.output.lock().unwrap();
        Ok((out.text(), out.truncated, term.exit.lock().unwrap().clone()))
    }

    /// Waits for the command to end.
    pub(crate) async fn wait(&self, id: &str) -> Result<Exit, String> {
        let term = self.term(id)?;
        loop {
            // Registered before the look, so an end between the two is not
            // missed.
            let ended = term.ended.notified();
            tokio::pin!(ended);
            ended.as_mut().enable();
            if let Some(exit) = term.exit.lock().unwrap().clone() {
                return Ok(exit);
            }
            ended.await;
        }
    }

    /// Stops the command and all it started; its output stays readable.
    pub(crate) fn kill(&self, id: &str) -> Result<(), String> {
        let term = self.term(id)?;
        stop(term.group);
        Ok(())
    }

    /// Stops the command if it still runs and forgets it. What it left in
    /// the background keeps running until the run ends.
    pub(crate) fn release(&self, id: &str) -> Result<(), String> {
        let term = self.terms.lock().unwrap().remove(id);
        let term = term.ok_or_else(|| format!("no terminal {id}"))?;
        if term.exit.lock().unwrap().is_none() {
            stop(term.group);
        }
        self.released.lock().unwrap().push(term.group);
        Ok(())
    }

    /// The agent shows command `id` in tool call `call`: what it printed
    /// goes there, from now on and so far.
    pub(crate) fn attach(&self, id: &str, call: &str) {
        let Ok(term) = self.term(id) else { return };
        let mut out = term.output.lock().unwrap();
        if out.call.as_deref() != Some(call) {
            out.call = Some(call.to_owned());
            send(&self.events, &mut out);
        }
    }
}

impl Drop for Terminals {
    /// The run is over: nothing it started keeps running.
    fn drop(&mut self) {
        // Every command's session, ended or not: what it left in the
        // background (a server) is still in it.
        for term in self.terms.lock().unwrap().values() {
            stop(term.group);
        }
        for group in self.released.lock().unwrap().iter() {
            stop(*group);
        }
    }
}

/// Passes the output on, to the tool call that shows it.
fn send(events: &UnboundedSender<Event>, out: &mut Output) {
    let Some(call) = out.call.clone() else { return };
    out.sent = Some(Instant::now());
    let text = out.text();
    let text = if text.len() > crate::MAX_OUTPUT {
        crate::tail(text)
    } else {
        text
    };
    let _ = events.unbounded_send(Event::ToolOutput { id: call, text });
}

fn stop(group: i32) {
    // SAFETY: kill only sends a signal; a negative pid names the session's
    // process group, which the command leads.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

fn exit_of(status: std::io::Result<std::process::ExitStatus>) -> Exit {
    match status {
        Ok(s) => Exit {
            code: s.code().map(|c| c as u32),
            signal: s.signal().map(signal_name),
        },
        Err(_) => Exit {
            code: None,
            signal: None,
        },
    }
}

fn signal_name(n: i32) -> String {
    match n {
        libc::SIGKILL => "SIGKILL".into(),
        libc::SIGTERM => "SIGTERM".into(),
        libc::SIGINT => "SIGINT".into(),
        libc::SIGSEGV => "SIGSEGV".into(),
        libc::SIGABRT => "SIGABRT".into(),
        libc::SIGPIPE => "SIGPIPE".into(),
        libc::SIGHUP => "SIGHUP".into(),
        n => format!("signal {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_that_load_code_or_undo_the_sandbox_are_refused() {
        for name in [
            "DYLD_INSERT_LIBRARIES",
            "LD_PRELOAD",
            "https_proxy",
            "ALL_PROXY",
            "HOME",
            "TMPDIR",
            "SSL_CERT_FILE",
        ] {
            assert!(refused_variable(name), "{name}");
        }
        for name in ["RUST_LOG", "NODE_ENV", "CI", "PATHEXT"] {
            assert!(!refused_variable(name), "{name}");
        }
    }

    #[test]
    fn output_is_text_from_its_first_whole_character() {
        let out = Output {
            bytes: "çok".as_bytes()[1..].to_vec(),
            limit: 10,
            truncated: true,
            call: None,
            sent: None,
        };
        assert_eq!(out.text(), "ok");
    }
}
