//! The proxy as a process of its own, `kule-proxy`, and the pipes between
//! it and Kule. On Windows the proxy runs inside an app container (see
//! [`crate::windows::appcontainer`]): a process that can reach the network
//! and nothing of the person's, not the task's files, not Kule's home, so
//! what it needs comes in over its pipes and nothing else.
//!
//! Kule writes the proxy's configuration as one line of JSON to its
//! standard input, then reads lines from its standard output: `ready` with
//! the port and the authority its commands trust, then each `ask` (a
//! connection the policy leaves to a person, answered on the same pipe)
//! and `logged` (a connection that ended, which Kule writes to the task's
//! log). `stop`, or the end of its input, ends it; the process's own end
//! (the job it is in on Windows) ends it when Kule goes away without a
//! word.

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::approvals::{Answer, Inside};
use crate::error::{Error, Result};
use crate::network::Connection;
use crate::proxy_core::{self, Ask, Log, ProxyConfig};

/// What Kule sends the proxy after its configuration.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ToProxy {
    /// The answer to the question `id` asked.
    Answer {
        id: u64,
        answer: Answer,
    },
    Stop,
}

/// What the proxy sends Kule.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum FromProxy {
    /// Serving on `port`; commands trust `ca_pem` where the proxy looks
    /// inside TLS.
    Ready { port: u16, ca_pem: String },
    /// Not serving, and why.
    Failed { why: String },
    /// A connection the policy leaves to a person; answered by its `id`.
    Ask {
        id: u64,
        host: String,
        port: u16,
        rules: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inside: Option<Inside>,
    },
    /// A connection that ended, for the task's log.
    Logged { connection: Connection },
}

/// How long Kule waits for the proxy to say it is ready.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long Kule waits for the proxy to end once told to stop.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------- the proxy

/// Serves as the proxy program: the configuration from `input`, the
/// messages above on `output`, until told to stop or `input` ends. The
/// exit code: 0 once it served, 1 when it could not start.
pub(crate) fn run_child(
    input: impl Read + Send + 'static,
    output: impl Write + Send + 'static,
) -> i32 {
    let output: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(output)));
    let mut lines = BufReader::new(input).lines();
    let failed = |output: &Arc<Mutex<Box<dyn Write + Send>>>, why: String| {
        send(output, &FromProxy::Failed { why });
        1
    };
    let config: ProxyConfig = match lines.next() {
        Some(Ok(line)) => match serde_json::from_str(&line) {
            Ok(config) => config,
            Err(e) => return failed(&output, format!("the configuration could not be read: {e}")),
        },
        _ => return failed(&output, "no configuration arrived".to_owned()),
    };
    let questions = Arc::new(Questions::default());
    let asker: Box<dyn Ask> = Box::new(PipeAsk {
        output: Arc::clone(&output),
        questions: Arc::clone(&questions),
    });
    let log: Box<dyn Log> = Box::new(PipeLog {
        output: Arc::clone(&output),
        questions: Arc::clone(&questions),
    });
    let listener = match lonca_net::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(e) => return failed(&output, format!("no loopback port to listen on: {e}")),
    };
    let (server, ca_pem) = match proxy_core::serve(listener, config, Some(asker), log) {
        Ok(served) => served,
        Err(e) => return failed(&output, e.to_string()),
    };
    send(
        &output,
        &FromProxy::Ready {
            port: server.local_addr().port(),
            ca_pem,
        },
    );
    for line in lines {
        let Ok(line) = line else { break };
        match serde_json::from_str::<ToProxy>(&line) {
            Ok(ToProxy::Answer { id, answer }) => questions.answer(id, answer),
            Ok(ToProxy::Stop) | Err(_) => break,
        }
    }
    // Questions first: a connection held for an answer would keep the
    // server from finishing. Each one withdrawn is refused with the reason
    // before the server cuts what is open, so the command is told why.
    questions.close();
    questions.settle(Duration::from_secs(2));
    server.stop();
    0
}

/// Writes one message as a line; a pipe that is gone is Kule gone, which
/// the process's end takes care of.
fn send<T: Serialize>(output: &Mutex<Box<dyn Write + Send>>, message: &T) {
    if let Ok(mut line) = serde_json::to_vec(message) {
        line.push(b'\n');
        let mut output = lock(output);
        let _ = output.write_all(&line).and_then(|()| output.flush());
    }
}

/// The questions the proxy has asked Kule and is waiting on.
#[derive(Default)]
struct Questions {
    next: AtomicU64,
    /// Each waiting question's answer once it came; `None` while it waits.
    answers: Mutex<HashMap<u64, Option<Answer>>>,
    /// Set once the proxy is stopping: whatever still waits is withdrawn.
    closed: Mutex<bool>,
    answered: Condvar,
    /// Connections withdrawn whose refusal has not been logged yet: still
    /// being told why.
    withdrawing: AtomicUsize,
}

impl Questions {
    fn open(&self) -> u64 {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        lock(&self.answers).insert(id, None);
        id
    }

    fn answer(&self, id: u64, answer: Answer) {
        {
            let mut answers = lock(&self.answers);
            if let Some(slot @ None) = answers.get_mut(&id) {
                *slot = Some(answer);
                // Counted before the waiter wakes, so a stop that follows
                // the answer at once still waits for the refusal to be told.
                if answer == Answer::Withdrawn {
                    self.withdrawing.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        self.answered.notify_all();
    }

    /// Waits for the answer to `id`; withdrawn when the proxy stops first.
    fn wait(&self, id: u64) -> Answer {
        let mut answers = lock(&self.answers);
        loop {
            if let Some(Some(answer)) = answers.get(&id) {
                let answer = *answer;
                answers.remove(&id);
                return answer;
            }
            if *lock(&self.closed) {
                answers.remove(&id);
                return Answer::Withdrawn;
            }
            answers = self
                .answered
                .wait(answers)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    /// A withdrawn connection has been refused and logged.
    fn withdrawn_told(&self) {
        self.withdrawing.fetch_sub(1, Ordering::SeqCst);
    }

    fn close(&self) {
        let answers = lock(&self.answers);
        let still_waiting = answers.values().filter(|a| a.is_none()).count();
        self.withdrawing.fetch_add(still_waiting, Ordering::SeqCst);
        *lock(&self.closed) = true;
        drop(answers);
        self.answered.notify_all();
    }

    /// Waits up to `timeout` for every withdrawn connection to be told.
    fn settle(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while self.withdrawing.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

struct PipeAsk {
    output: Arc<Mutex<Box<dyn Write + Send>>>,
    questions: Arc<Questions>,
}

impl Ask for PipeAsk {
    fn ask(&self, host: &str, port: u16, rules: &[String], inside: Option<Inside>) -> Answer {
        let id = self.questions.open();
        send(
            &self.output,
            &FromProxy::Ask {
                id,
                host: host.to_owned(),
                port,
                rules: rules.to_vec(),
                inside,
            },
        );
        self.questions.wait(id)
    }
}

struct PipeLog {
    output: Arc<Mutex<Box<dyn Write + Send>>>,
    questions: Arc<Questions>,
}

impl Log for PipeLog {
    fn record(&self, connection: Connection) {
        let withdrawn = connection.answer == Some(Answer::Withdrawn);
        send(&self.output, &FromProxy::Logged { connection });
        if withdrawn {
            self.questions.withdrawn_told();
        }
    }
}

// ----------------------------------------------------------------- Kule's side

/// A started proxy process, before Kule has spoken to it.
pub(crate) struct Launched {
    pub stdin: Box<dyn Write + Send>,
    pub stdout: Box<dyn Read + Send>,
    /// What the process prints as errors, to be passed on as Kule's own.
    pub stderr: Option<Box<dyn Read + Send>>,
    pub process: Box<dyn ChildProcess>,
}

/// The process behind [`Launched`], however it was started.
pub(crate) trait ChildProcess: Send {
    fn pid(&self) -> u32;
    /// Waits up to `timeout` for the process to end; whether it has.
    fn wait_for(&mut self, timeout: Duration) -> bool;
    fn kill(&mut self);
}

/// The `kule-proxy` program at `exe` started as a plain child process of
/// this one, outside any container: for the tests of the pipes alone.
pub(crate) fn launch_plain(exe: &std::path::Path) -> io::Result<Launched> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(exe);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn()?;
    let stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    Ok(Launched {
        stdin: Box::new(stdin),
        stdout: Box::new(stdout),
        stderr: Some(Box::new(stderr)),
        process: Box::new(child),
    })
}

impl ChildProcess for std::process::Child {
    fn pid(&self) -> u32 {
        self.id()
    }

    fn wait_for(&mut self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            match self.try_wait() {
                Ok(Some(_)) | Err(_) => return true,
                Ok(None) if Instant::now() >= deadline => return false,
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
    }

    fn kill(&mut self) {
        let _ = std::process::Child::kill(self);
        let _ = self.wait();
    }
}

/// A proxy served by a process of its own, which ends with this.
pub(crate) struct ChildProxy {
    stdin: Arc<Mutex<Box<dyn Write + Send>>>,
    process: Box<dyn ChildProcess>,
    port: u16,
    ca_pem: String,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl ChildProxy {
    /// Starts the process `launch` gives, hands it `config`, puts the
    /// questions it asks to `ask` and what it logs to `log`, and waits for
    /// it to serve; a process that fails to, or says nothing in time, is
    /// ended and told of.
    pub fn spawn(
        launch: impl FnOnce() -> io::Result<Launched>,
        config: ProxyConfig,
        ask: Option<Box<dyn Ask>>,
        log: Box<dyn Log>,
    ) -> Result<ChildProxy> {
        let not_started =
            |why: String| Error::Invalid(format!("Kule's proxy did not start: {why}"));
        let Launched {
            stdin,
            stdout,
            stderr,
            mut process,
        } = launch().map_err(|e| not_started(e.to_string()))?;
        let stdin: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(stdin));
        let mut line = serde_json::to_vec(&config).map_err(|e| not_started(e.to_string()))?;
        line.push(b'\n');
        if let Err(e) = {
            let mut stdin = lock(&stdin);
            stdin.write_all(&line).and_then(|()| stdin.flush())
        } {
            process.kill();
            return Err(not_started(format!("it took no configuration: {e}")));
        }
        // Its errors are Kule's to show, under its name.
        if let Some(stderr) = stderr {
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(io::Result::ok) {
                    eprintln!("kule-proxy: {line}");
                }
            });
        }
        let (ready_tx, ready_rx) = sync_channel::<std::result::Result<(u16, String), String>>(1);
        let ask: Option<Arc<dyn Ask>> = ask.map(Arc::from);
        let reader = std::thread::spawn({
            let stdin = Arc::clone(&stdin);
            move || {
                let mut ready = Some(ready_tx);
                for line in BufReader::new(stdout).lines().map_while(io::Result::ok) {
                    let Ok(message) = serde_json::from_str::<FromProxy>(&line) else {
                        continue;
                    };
                    match message {
                        FromProxy::Ready { port, ca_pem } => {
                            if let Some(ready) = ready.take() {
                                let _ = ready.send(Ok((port, ca_pem)));
                            }
                        }
                        FromProxy::Failed { why } => {
                            if let Some(ready) = ready.take() {
                                let _ = ready.send(Err(why));
                            }
                        }
                        FromProxy::Ask {
                            id,
                            host,
                            port,
                            rules,
                            inside,
                        } => {
                            // Each question waits on a thread of its own, as
                            // each connection does in the proxy.
                            let stdin = Arc::clone(&stdin);
                            let ask = ask.clone();
                            std::thread::spawn(move || {
                                let answer = match &ask {
                                    Some(ask) => ask.ask(&host, port, &rules, inside),
                                    None => Answer::Withdrawn,
                                };
                                send(&stdin, &ToProxy::Answer { id, answer });
                            });
                        }
                        FromProxy::Logged { connection } => log.record(connection),
                    }
                }
                if let Some(ready) = ready.take() {
                    let _ = ready.send(Err("it ended before it was ready".to_owned()));
                }
            }
        });
        let (port, ca_pem) = match ready_rx.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(ready)) => ready,
            Ok(Err(why)) => {
                process.kill();
                return Err(not_started(why));
            }
            Err(RecvTimeoutError::Timeout) => {
                process.kill();
                return Err(not_started(format!(
                    "it said nothing in {} seconds",
                    READY_TIMEOUT.as_secs()
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                process.kill();
                return Err(not_started("it ended before it was ready".to_owned()));
            }
        };
        Ok(ChildProxy {
            stdin,
            process,
            port,
            ca_pem,
            reader: Some(reader),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn ca_pem(&self) -> &str {
        &self.ca_pem
    }

    pub fn pid(&self) -> u32 {
        self.process.pid()
    }

    /// Tells the process to stop and waits for it; ends it when it does
    /// not stop in time. Once this returns everything it logged has been
    /// recorded.
    pub fn stop(mut self) {
        self.shut();
    }

    fn shut(&mut self) {
        send(&self.stdin, &ToProxy::Stop);
        if !self.process.wait_for(STOP_TIMEOUT) {
            self.process.kill();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

impl Drop for ChildProxy {
    fn drop(&mut self) {
        self.shut();
    }
}

/// A poisoned lock only means another thread panicked; what it guards is
/// still usable.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
