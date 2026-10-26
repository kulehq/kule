//! What starts a command in a sandbox, whatever the platform makes of it:
//! a process under Seatbelt or Landlock on macOS and Linux, a process
//! container on Windows (see `sandbox::windows`). Callers set the
//! environment, the directory and the streams the same way on each, start
//! it, and get a [`Process`] with the streams they asked for, which they
//! wait for or end.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};

/// How a command's standard stream is set up: ours, a pipe to Kule, or
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Io {
    Inherit,
    Piped,
    Null,
}

impl From<Io> for Stdio {
    fn from(io: Io) -> Stdio {
        match io {
            Io::Inherit => Stdio::inherit(),
            Io::Piped => Stdio::piped(),
            Io::Null => Stdio::null(),
        }
    }
}

/// A command ready to start in its sandbox (see [`super::Sandbox::start`]).
pub enum Start {
    /// A plain process whose sandbox is set up between fork and exec
    /// (macOS, Linux).
    Command(Command),
    /// A process in a container of its own (Windows).
    #[cfg(windows)]
    Container(Box<super::windows::Container>),
}

impl std::fmt::Debug for Start {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Start::Command(command) => command.fmt(f),
            #[cfg(windows)]
            Start::Container(container) => container.fmt(f),
        }
    }
}

impl Start {
    pub fn env(&mut self, name: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> &mut Start {
        match self {
            Start::Command(command) => {
                command.env(name, value);
            }
            #[cfg(windows)]
            Start::Container(container) => container.env(name.as_ref(), value.as_ref()),
        }
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Start
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (name, value) in vars {
            self.env(name, value);
        }
        self
    }

    pub fn env_remove(&mut self, name: impl AsRef<OsStr>) -> &mut Start {
        match self {
            Start::Command(command) => {
                command.env_remove(name);
            }
            #[cfg(windows)]
            Start::Container(container) => container.env_remove(name.as_ref()),
        }
        self
    }

    pub fn current_dir(&mut self, dir: impl AsRef<Path>) -> &mut Start {
        match self {
            Start::Command(command) => {
                command.current_dir(dir);
            }
            #[cfg(windows)]
            Start::Container(container) => container.current_dir(dir.as_ref()),
        }
        self
    }

    pub fn stdin(&mut self, io: Io) -> &mut Start {
        match self {
            Start::Command(command) => {
                command.stdin(io);
            }
            #[cfg(windows)]
            Start::Container(container) => container.io.0 = io,
        }
        self
    }

    pub fn stdout(&mut self, io: Io) -> &mut Start {
        match self {
            Start::Command(command) => {
                command.stdout(io);
            }
            #[cfg(windows)]
            Start::Container(container) => container.io.1 = io,
        }
        self
    }

    pub fn stderr(&mut self, io: Io) -> &mut Start {
        match self {
            Start::Command(command) => {
                command.stderr(io);
            }
            #[cfg(windows)]
            Start::Container(container) => container.io.2 = io,
        }
        self
    }

    /// The variables set on the command so far, as [`Command::get_envs`]
    /// gives them.
    pub fn get_envs(&self) -> Vec<(&OsStr, Option<&OsStr>)> {
        match self {
            Start::Command(command) => command.get_envs().collect(),
            #[cfg(windows)]
            Start::Container(container) => container.get_envs(),
        }
    }

    pub fn get_current_dir(&self) -> Option<&Path> {
        match self {
            Start::Command(command) => command.get_current_dir(),
            #[cfg(windows)]
            Start::Container(container) => Some(container.get_current_dir()),
        }
    }

    /// Starts the command; its streams are those asked for, [`Io::Inherit`]
    /// for each unless said otherwise.
    pub fn spawn(&mut self) -> io::Result<Process> {
        match self {
            Start::Command(command) => {
                let mut child = command.spawn()?;
                Ok(Process {
                    id: child.id(),
                    stdin: child
                        .stdin
                        .take()
                        .map(|s| Box::new(s) as Box<dyn Write + Send>),
                    stdout: child
                        .stdout
                        .take()
                        .map(|s| Box::new(s) as Box<dyn Read + Send>),
                    stderr: child
                        .stderr
                        .take()
                        .map(|s| Box::new(s) as Box<dyn Read + Send>),
                    waiter: Waiter::Child(child),
                    relays: Vec::new(),
                })
            }
            #[cfg(windows)]
            Start::Container(container) => container.spawn(),
        }
    }

    /// Runs the command to its end on the streams asked for, and tells how
    /// it ended.
    pub fn status(&mut self) -> io::Result<ExitStatus> {
        match self {
            Start::Command(command) => command.status(),
            #[cfg(windows)]
            Start::Container(container) => container.status(),
        }
    }

    /// Runs the command to its end with its output and errors collected and
    /// nothing on its input, as [`Command::output`] does.
    pub fn output(&mut self) -> io::Result<Output> {
        match self {
            Start::Command(command) => command.output(),
            #[cfg(windows)]
            Start::Container(container) => {
                container.io = (Io::Null, Io::Piped, Io::Piped);
                let mut process = container.spawn()?;
                let stdout = process.stdout.take().map(read_all);
                let stderr = process.stderr.take().map(read_all);
                let status = process.wait()?;
                Ok(Output {
                    status,
                    stdout: stdout
                        .map(|t| t.join().unwrap_or_default())
                        .unwrap_or_default(),
                    stderr: stderr
                        .map(|t| t.join().unwrap_or_default())
                        .unwrap_or_default(),
                })
            }
        }
    }
}

/// Reads `stream` to its end on a thread of its own.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn read_all(mut stream: Box<dyn Read + Send>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        bytes
    })
}

/// How long a command's relayed output is waited for once it ended.
const RELAY_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// A started command (see [`Start::spawn`]). The streams asked for as
/// [`Io::Piped`] are here to take; the others are `None`.
pub struct Process {
    id: u32,
    pub stdin: Option<Box<dyn Write + Send>>,
    pub stdout: Option<Box<dyn Read + Send>>,
    pub stderr: Option<Box<dyn Read + Send>>,
    waiter: Waiter,
    /// Threads relaying the command's output to ours (Windows; see
    /// [`Io::Inherit`]), joined once the command is gone.
    relays: Vec<std::thread::JoinHandle<()>>,
}

enum Waiter {
    Child(Child),
    #[cfg(windows)]
    Contained(mxc_sdk::v1::MxcProcess),
}

impl std::fmt::Debug for Process {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Process").field("id", &self.id).finish()
    }
}

impl Process {
    #[cfg(windows)]
    pub(crate) fn contained(
        process: mxc_sdk::v1::MxcProcess,
        stdin: Option<Box<dyn Write + Send>>,
        stdout: Option<Box<dyn Read + Send>>,
        stderr: Option<Box<dyn Read + Send>>,
        relays: Vec<std::thread::JoinHandle<()>>,
    ) -> Process {
        Process {
            id: process.id(),
            stdin,
            stdout,
            stderr,
            waiter: Waiter::Contained(process),
            relays,
        }
    }

    /// The process's id: the command's own, inside its sandbox.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Waits for the command to end. Where the platform ends what it left
    /// running with it (a process container), that is over too.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = match &mut self.waiter {
            Waiter::Child(child) => child.wait()?,
            #[cfg(windows)]
            Waiter::Contained(process) => super::windows::status_of(process.wait()?),
        };
        // What it printed last, relayed to the end; a process it left
        // holding its output ends with its container, so this ends too, but
        // not for ever should that fail.
        let until = std::time::Instant::now() + RELAY_GRACE;
        while !self.relays.iter().all(|r| r.is_finished()) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for relay in self.relays.drain(..).filter(|r| r.is_finished()) {
            let _ = relay.join();
        }
        Ok(status)
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        match &mut self.waiter {
            Waiter::Child(child) => child.try_wait(),
            #[cfg(windows)]
            Waiter::Contained(process) => {
                use std::os::windows::process::ExitStatusExt;
                Ok(process
                    .try_wait()?
                    .map(|code| ExitStatus::from_raw(code as u32)))
            }
        }
    }

    /// Ends the command at once, and on Windows everything in its
    /// container with it.
    pub fn kill(&mut self) -> io::Result<()> {
        match &mut self.waiter {
            Waiter::Child(child) => child.kill(),
            #[cfg(windows)]
            Waiter::Contained(process) => process.kill(),
        }
    }

    /// What the platform warns of about this command's sandbox, for the
    /// run's notes; nothing where it holds everything asked of it.
    pub fn warnings(&self) -> Vec<String> {
        match &self.waiter {
            Waiter::Child(_) => Vec::new(),
            #[cfg(windows)]
            Waiter::Contained(process) => process.warnings(),
        }
    }
}

/// The variables a contained command starts with, by name, in the order
/// they were set; a name set again keeps its place with the new value.
/// Windows reads names without regard to case, so this does too.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub(crate) struct Environment {
    vars: Vec<(OsString, OsString)>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Environment {
    fn position(&self, name: &OsStr) -> Option<usize> {
        let name = name.to_string_lossy().to_ascii_uppercase();
        self.vars
            .iter()
            .position(|(n, _)| n.to_string_lossy().to_ascii_uppercase() == name)
    }

    pub(crate) fn set(&mut self, name: &OsStr, value: &OsStr) {
        match self.position(name) {
            Some(at) => self.vars[at].1 = value.to_owned(),
            None => self.vars.push((name.to_owned(), value.to_owned())),
        }
    }

    pub(crate) fn remove(&mut self, name: &OsStr) {
        if let Some(at) = self.position(name) {
            self.vars.remove(at);
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.vars
            .iter()
            .map(|(n, v)| (n.as_os_str(), v.as_os_str()))
    }

    /// The variables as text, sorted by name as Windows wants an
    /// environment block; a name or value that is not text is left out.
    pub(crate) fn as_text(&self) -> Vec<(String, String)> {
        let mut map: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in &self.vars {
            if let (Some(name), Some(value)) = (name.to_str(), value.to_str()) {
                map.insert(name.to_owned(), value.to_owned());
            }
        }
        map.into_iter().collect()
    }
}

/// Where a program is, so that a command starts what the person has on
/// their `PATH` and never what a task's copy holds under that name: a
/// name with no folder in it is looked for on `PATH` alone, with the
/// extensions Windows runs; one with a folder is taken from there,
/// against `cwd` when relative.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn resolve_program(program: &str, cwd: &Path) -> Option<PathBuf> {
    let found = |path: PathBuf| {
        crate::osfs::executable_candidates(&path)
            .into_iter()
            .find(|c| crate::osfs::executable_at(c))
    };
    if program.contains(['\\', '/']) {
        let path = Path::new(program);
        return found(if path.is_absolute() {
            path.to_owned()
        } else {
            cwd.join(path)
        });
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .find_map(|dir| found(dir.join(program)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_environment_keeps_one_value_a_name_whatever_its_case() {
        let mut env = Environment::default();
        env.set(OsStr::new("Path"), OsStr::new("a"));
        env.set(OsStr::new("PATH"), OsStr::new("b"));
        env.set(OsStr::new("HOME"), OsStr::new("h"));
        assert_eq!(env.iter().count(), 2);
        assert_eq!(
            env.iter().next(),
            Some((OsStr::new("Path"), OsStr::new("b")))
        );
        env.remove(OsStr::new("home"));
        assert_eq!(env.as_text(), vec![("Path".to_owned(), "b".to_owned())]);
    }

    /// A bare name is never taken from the working directory, where a
    /// task's copy could hold a program of that name.
    #[test]
    fn a_bare_program_name_is_never_taken_from_the_working_directory() {
        let dir = tempfile::TempDir::new().unwrap();
        let name = if cfg!(windows) {
            "planted.exe"
        } else {
            "planted"
        };
        std::fs::write(dir.path().join(name), b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                dir.path().join(name),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        assert_eq!(resolve_program("planted", dir.path()), None);
        let explicit = format!(".{}{name}", std::path::MAIN_SEPARATOR);
        assert_eq!(
            resolve_program(&explicit, dir.path()),
            Some(dir.path().join(".").join(name))
        );
    }
}
