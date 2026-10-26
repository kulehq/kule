//! The person's own shell in a task's copy, inside the task's sandbox: what
//! `kule exec` gives a command, for as long as the shell is open. What the
//! agent wrote there runs only as sandboxed as the agent ran, whatever the
//! shell's prompt or the person reads or runs.

#[cfg(unix)]
use std::os::fd::BorrowedFd;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;
use std::time::Instant;

use kule_engine::{Denial, OpenTerminal, Policy, Proxy, Sandbox, record_denials};

pub use kule_engine::Task;

use crate::Result;

/// What a shell's end tells the person (see [`Sandboxed::end`]).
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize)]
pub struct ShellEnd {
    /// What the sandbox refused the shell's commands.
    pub denials: Vec<Denial>,
    /// The platform does not report refusals (see
    /// `kule_engine::UNREPORTED`).
    pub unreported: bool,
    /// The system log did not show all of them (see
    /// `kule_engine::INCOMPLETE`).
    pub incomplete: bool,
    /// When the system log dropped the kernel's messages while the shell
    /// ran (see `kule_engine::Refusals::lost_note`): told apart from
    /// [`ShellEnd::lines`], quietly, as macOS does it every few seconds.
    pub lost: Option<String>,
    /// What it left running, which Kule ended with it, by name.
    pub ended: Vec<String>,
    /// What Kule could not do or read of it.
    pub error: Option<String>,
}

impl ShellEnd {
    /// What the person is told as the shell ends, a line each: what was
    /// ended, what Kule could not do, that the refusals listed may not be
    /// all, and on Linux, when the shell failed (`failed`), that refusals
    /// are not reported. The refusals themselves are listed apart.
    pub fn notes(&self, failed: bool) -> Vec<String> {
        let mut notes = Vec::new();
        if !self.ended.is_empty() {
            let ended = kule_engine::Ended {
                processes: self.ended.clone(),
            };
            notes.push(format!(
                "Kule ended {}, which the shell left running in its sandbox",
                ended.listed()
            ));
        }
        notes.extend(self.error.clone());
        if self.incomplete {
            notes.push(kule_engine::INCOMPLETE.to_owned());
        }
        if self.unreported && failed {
            notes.push(kule_engine::UNREPORTED.to_owned());
        }
        notes
    }

    /// [`ShellEnd::notes`] after what the sandbox refused, a line each,
    /// the same refusal once with how many times (the terminals a shell
    /// probes as one, see [`Denial::grouped`]), then the hints: for a
    /// terminal that shows them where the shell was.
    pub fn lines(&self, failed: bool) -> Vec<String> {
        let refused = Denial::grouped(&self.denials);
        let mut hints: Vec<String> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        for (d, count) in refused {
            let line = format!(
                "sandbox: refused {} {} ({})",
                d.operation, d.path, d.process
            );
            lines.push(match count {
                1 => line,
                _ => format!("{line} ×{count}"),
            });
            if let Some(hint) = d.hint
                && !hints.contains(&hint)
            {
                hints.push(hint);
            }
        }
        lines.extend(hints.into_iter().map(|h| format!("  {h}")));
        lines.extend(self.notes(failed));
        lines
    }
}

/// Makes task `task`'s package caches before a terminal's sandbox does, so
/// `tell` can say so while that takes a while (see `Task::prepare_caches`).
pub fn prepare_caches(home: &Path, task: &str, tell: impl FnMut(&str)) -> Result<()> {
    let Some(sources) = kule_engine::CacheSources::from_env() else {
        return Ok(());
    };
    Task::open(home, task)
        .map_err(text)?
        .prepare_caches(&sources, tell)
        .map_err(text)
}

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
        task.prepare_home(person, &[], sandbox.hidden(), sandbox.shown(), &[])
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

    /// What Kule withheld from this shell, to tell the person before it
    /// starts (see `Task::home_withheld_notes`).
    pub fn notes(&self) -> Vec<String> {
        self.task.home_withheld_notes().unwrap_or_default()
    }

    pub fn task(&self) -> &Task {
        &self.task
    }

    /// `program` on `terminal` (a pseudo-terminal's follower end), in the
    /// sandbox, with the terminal as its own (see `Sandbox::terminal_command`).
    #[cfg(unix)]
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
    /// work go on. What of it the person is told; nothing is skipped
    /// because another part failed.
    pub fn end(self) -> ShellEnd {
        let mut end = ShellEnd::default();
        let mut failed = Vec::new();
        // What the shell left running goes with it, before its terminal
        // can be another's (see `Sandbox::end_own`).
        match self.sandbox.end_own() {
            Ok(ended) => end.ended = ended.processes,
            Err(e) => failed.push(text(e)),
        }
        self.proxy.stop();
        match self.sandbox.denials(self.started.elapsed()) {
            Ok(Some(refused)) => {
                end.lost = refused.lost_note();
                end.denials = refused.denials;
                end.incomplete = refused.incomplete;
            }
            Ok(None) => end.unreported = true,
            Err(e) => failed.push(format!("what the sandbox refused cannot be read: {e}")),
        }
        if let Err(e) = record_denials(&self.task, &end.denials) {
            failed.push(format!(
                "what the sandbox refused could not be recorded: {e}"
            ));
        }
        end.error = (!failed.is_empty()).then(|| failed.join("; "));
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(process: &str, path: &str) -> Denial {
        Denial {
            process: process.into(),
            operation: "file-read-data".into(),
            path: path.into(),
            saved: None,
            hint: None,
        }
    }

    /// A shell that probed the terminals around it is told in one line,
    /// counted, before what Kule ended with it.
    #[test]
    fn a_shell_s_end_is_told_a_line_each_the_same_once() {
        let end = ShellEnd {
            denials: vec![
                refused("zsh", "/dev/ttys004"),
                refused("zsh", "/dev/ttys004"),
                refused("cat", "/etc/secret"),
                refused("zsh", "/dev/ttys5"),
            ],
            ended: vec!["sleep".into(), "sleep".into()],
            ..ShellEnd::default()
        };
        assert_eq!(
            end.lines(false),
            [
                "sandbox: refused file-read-data /dev/ttys* (zsh) ×3",
                "sandbox: refused file-read-data /etc/secret (cat)",
                "Kule ended sleep ×2, which the shell left running in its sandbox",
            ]
        );
        let linux = ShellEnd {
            unreported: true,
            ..ShellEnd::default()
        };
        assert!(linux.lines(false).is_empty());
        assert_eq!(linux.lines(true), [kule_engine::UNREPORTED]);
        // Told whether the shell failed or not: the list may not be all.
        let late = ShellEnd {
            incomplete: true,
            ..ShellEnd::default()
        };
        assert_eq!(late.lines(false), [kule_engine::INCOMPLETE]);
        // What the log dropped is told apart, quietly: not among the lines.
        let dropped = ShellEnd {
            lost: Some("dropped".to_owned()),
            ..ShellEnd::default()
        };
        assert!(dropped.lines(true).is_empty());
    }
}
