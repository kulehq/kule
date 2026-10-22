//! The person's own shell in a task's copy, inside the task's sandbox: what
//! `kule exec` gives a command, for as long as the shell is open. What the
//! agent wrote there runs only as sandboxed as the agent ran, whatever the
//! shell's prompt or the person reads or runs.

use std::os::fd::BorrowedFd;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use kule_engine::{Denial, OpenTerminal, Policy, Proxy, Sandbox, record_denials};

pub use kule_engine::Task;

use crate::Result;

fn text(e: kule_engine::Error) -> String {
    e.to_string()
}

/// The task's sandbox as `kule exec` gives a command: the agents' own
/// state hidden, and the home a layer of the task's, where what the
/// command writes is caught for review (`kule home`) rather than refused.
/// The network is left to the caller.
pub fn sandbox(home: &Path, task: &Task) -> Result<Sandbox> {
    let mut sandbox = Sandbox::for_task(task).map_err(text)?;
    if let Some(person) = std::env::var_os("HOME") {
        let person = Path::new(&person);
        kule_acp::Agents::load(home)
            .map_err(text)?
            .hide_agents(&mut sandbox, person)
            .map_err(text)?;
        task.prepare_home(person, &[], sandbox.hidden(), &[])
            .map_err(text)?;
        sandbox.use_home(task, person).map_err(text)?;
    }
    Ok(sandbox)
}

/// A shell's sandbox on a task's copy, open until [`Sandboxed::end`]: the
/// task holds no agent's work, commit, sync or home apply meanwhile (see
/// `Task::terminal_opened`), and its connections go through the task's
/// proxy, judged by its policy; a host the policy would ask about is
/// refused, as nobody is there to be asked.
pub struct Sandboxed {
    task: Task,
    sandbox: Sandbox,
    proxy: Proxy,
    started: Instant,
    _open: OpenTerminal,
}

impl Sandboxed {
    /// For task `id` in Kule's `home`; refused while an agent is at work
    /// there.
    pub fn new(home: &Path, id: &str) -> Result<Sandboxed> {
        let task = Task::open(home, id).map_err(text)?;
        // Marked before anything else, so no run starts while it is made.
        let open = task.terminal_opened().map_err(text)?;
        let mut sandbox = sandbox(home, &task)?;
        let proxy = Proxy::start(&task, Policy::for_task(&task).map_err(text)?).map_err(text)?;
        sandbox.use_proxy(&proxy);
        Ok(Sandboxed {
            task,
            sandbox,
            proxy,
            started: Instant::now(),
            _open: open,
        })
    }

    pub fn task(&self) -> &Task {
        &self.task
    }

    /// `program` on `terminal` (a pseudo-terminal's follower end), in the
    /// sandbox, with the terminal as its own (see `Sandbox::terminal_command`).
    pub fn command(
        &self,
        program: &str,
        args: &[String],
        terminal: BorrowedFd<'_>,
    ) -> Result<Command> {
        let mut command = self
            .sandbox
            .terminal_command(program, args, terminal)
            .map_err(text)?;
        command.env("KULE_TASK", self.task.id());
        Ok(command)
    }

    /// Once the shell ended: ends what it left running, cuts what it left
    /// connected, notes what the sandbox refused it, and lets the task's
    /// work go on. The refusals, where the platform reports them.
    pub fn end(self) -> Result<Vec<Denial>> {
        // What the shell left running goes with it, before its terminal
        // can be another's (see `Sandbox::end_own`).
        let ended = self.sandbox.end_own().map_err(text);
        self.proxy.stop();
        let denials = self
            .sandbox
            .denials(self.started.elapsed())
            .map_err(text)?
            .unwrap_or_default();
        record_denials(&self.task, &denials).map_err(text)?;
        ended?;
        Ok(denials)
    }
}
