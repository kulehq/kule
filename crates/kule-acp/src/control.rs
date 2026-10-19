//! A run's handle from outside: what stops it (a person, a limit, the
//! budget), why, and which processes are its own.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cloned freely; every clone is the same run's.
#[derive(Clone)]
pub struct RunControl(Arc<Control>);

struct Control {
    reason: Mutex<Option<String>>,
    stopping: tokio::sync::Notify,
    /// The agent's process.
    agent: AtomicI32,
    /// The commands Kule runs for the agent, outside its process group.
    commands: Mutex<Vec<i32>>,
    /// When the agent last did anything, in milliseconds after `started`.
    active: AtomicU64,
    started: Instant,
    over: AtomicBool,
}

impl Default for RunControl {
    fn default() -> Self {
        RunControl::new()
    }
}

impl RunControl {
    pub fn new() -> RunControl {
        RunControl(Arc::new(Control {
            reason: Mutex::new(None),
            stopping: tokio::sync::Notify::new(),
            agent: AtomicI32::new(0),
            commands: Mutex::new(Vec::new()),
            active: AtomicU64::new(0),
            started: Instant::now(),
            over: AtomicBool::new(false),
        }))
    }

    /// Stops the run: its turn is cancelled as a person would, and the
    /// agent gets a moment to end it before it is killed. The run then
    /// fails with `reason`; the first reason given stands.
    pub fn stop(&self, reason: impl Into<String>) {
        {
            let mut kept = self
                .0
                .reason
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if kept.is_some() {
                return;
            }
            *kept = Some(reason.into());
        }
        self.0.stopping.notify_waiters();
    }

    /// Why the run was stopped, once it was.
    pub fn reason(&self) -> Option<String> {
        self.0
            .reason
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Resolves once [`RunControl::stop`] is called, also when it was
    /// before.
    pub(crate) async fn stopped(&self) {
        loop {
            let notified = self.0.stopping.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.reason().is_some() {
                return;
            }
            notified.await;
        }
    }

    /// The processes the run started itself: the agent's and the commands
    /// Kule runs for it. What they start in turn is found from them.
    pub fn roots(&self) -> Vec<i32> {
        let agent = self.0.agent.load(Ordering::SeqCst);
        let mut roots: Vec<i32> = (agent > 0).then_some(agent).into_iter().collect();
        roots.extend(
            self.0
                .commands
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .copied(),
        );
        roots
    }

    pub(crate) fn started_agent(&self, pid: i32) {
        self.0.agent.store(pid, Ordering::SeqCst);
        self.touch();
    }

    pub(crate) fn started_command(&self, pid: i32) {
        self.0
            .commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(pid);
        self.touch();
    }

    /// The agent did something: said, called or printed.
    pub fn touch(&self) {
        let now = self.0.started.elapsed().as_millis() as u64;
        self.0.active.fetch_max(now, Ordering::SeqCst);
    }

    /// How long ago the agent last did anything.
    pub fn quiet_for(&self) -> Duration {
        let active = Duration::from_millis(self.0.active.load(Ordering::SeqCst));
        self.0.started.elapsed().saturating_sub(active)
    }

    /// How long the run has gone on.
    pub fn elapsed(&self) -> Duration {
        self.0.started.elapsed()
    }

    pub(crate) fn finish(&self) {
        self.0.over.store(true, Ordering::SeqCst);
    }

    /// The run returned: its agent is gone.
    pub fn is_over(&self) -> bool {
        self.0.over.load(Ordering::SeqCst)
    }
}

/// Marks the run over however `run_pictured` returns.
pub(crate) struct Finish(pub(crate) RunControl);

impl Drop for Finish {
    fn drop(&mut self) {
        self.0.finish();
    }
}
