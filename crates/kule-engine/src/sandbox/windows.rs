//! The sandbox on Windows: a process container (Microsoft's `mxc-sdk`,
//! measured in `docs/windows-measurements.md`) around each command. Unlike
//! Seatbelt and Landlock, which let a command read everything but what is
//! hidden, the container lets it read nothing but what is granted: the
//! task's copy and temporary folder (writable), its home layer, the
//! person's tools (Windows, Program Files, what is on `PATH` and where
//! toolchains install themselves) and what the sandbox lets through inside
//! a hidden folder. Credentials inside a grant are denied by name. Every
//! folder on the way to a grant is listed, not read, so that paths
//! resolve. The network is shut, open, or the proxy's alone, which the
//! container reaches only in the proxy's own app container (see
//! [`crate::windows::appcontainer`]). The UI stays enabled: the programs a
//! task runs load `user32.dll` and die without it; the container's job
//! keeps the clipboard and input from them all the same.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use mxc_sdk::v1::configs::{
    ProcessContainerConfig, ProcessContainerFilesystem, ProcessContainerNetwork,
};
use mxc_sdk::v1::policy::{
    ClipboardPolicy, FilesystemPolicy, NetworkAction, NetworkEgressPolicy, NetworkIngressPolicy,
    NetworkPolicy, NetworkRuntimeConfig, UiPolicy,
};
use mxc_sdk::v1::{ContainerRequest, Containment, WaitResult};

use super::start::{Environment, Io, Process, read_all, resolve_program};
use super::{Network, Sandbox};
use crate::error::{Error, Result};
use crate::osfs::plain;

/// A command to start in a container of its own (see [`super::Start`]).
pub struct Container {
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    env: Environment,
    readwrite: Vec<PathBuf>,
    readonly: Vec<PathBuf>,
    denied: Vec<PathBuf>,
    enumerate: Vec<PathBuf>,
    network: Network,
    pub(crate) io: (Io, Io, Io),
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("network", &self.network)
            .finish()
    }
}

impl Container {
    pub(crate) fn env(&mut self, name: &OsStr, value: &OsStr) {
        self.env.set(name, value);
    }

    pub(crate) fn env_remove(&mut self, name: &OsStr) {
        self.env.remove(name);
    }

    pub(crate) fn current_dir(&mut self, dir: &Path) {
        self.cwd = plain(dir);
    }

    pub(crate) fn get_envs(&self) -> Vec<(&OsStr, Option<&OsStr>)> {
        self.env.iter().map(|(n, v)| (n, Some(v))).collect()
    }

    pub(crate) fn get_current_dir(&self) -> &Path {
        &self.cwd
    }

    /// The request as the container runtime takes it.
    fn request(&self) -> io::Result<ContainerRequest> {
        let program = resolve_program(&self.program, &self.cwd).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{}: no such program on your PATH", self.program),
            )
        })?;
        let command = command_line(&program, &self.args)?;
        let text = |paths: &[PathBuf]| -> Vec<String> {
            paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        };
        let mut config = ProcessContainerConfig::default();
        let mut filesystem = ProcessContainerFilesystem::default();
        filesystem.enumerate_paths = text(&self.enumerate);
        config.filesystem = Some(filesystem);
        let (network, peer) = network_policy(self.network);
        if let Some(peer) = peer {
            let mut proxied = ProcessContainerNetwork::default();
            proxied.allowed_proxy_peer = Some(peer);
            config.network = Some(proxied);
        }
        Ok(ContainerRequest {
            filesystem: Some(FilesystemPolicy {
                readwrite_paths: text(&self.readwrite),
                readonly_paths: text(&self.readonly),
                denied_paths: text(&self.denied),
                clear_policy_on_exit: None,
            }),
            network: Some(network),
            // Disabled, every program that loads user32 dies at start
            // (measured); the job's own isolation keeps the clipboard and
            // the person's input from the command all the same.
            ui: Some(UiPolicy {
                disable: false,
                clipboard: ClipboardPolicy::None,
                allow_input_injection: false,
            }),
            timeout_ms: None,
            containment: Containment::ProcessContainer(config),
            container_name: None,
            working_directory: Some(self.cwd.to_string_lossy().into_owned()),
            environment: Some(self.env.as_text()),
            // Nothing of Kule's own environment: tokens and keys set for
            // Kule or an agent stay out, as on every platform.
            inherit_default_environment: Some(false),
            ..ContainerRequest::new(command.to_string_lossy().into_owned())
        })
    }

    pub(crate) fn spawn(&self) -> io::Result<Process> {
        let request = self.request()?;
        let mut process = mxc_sdk::v1::spawn(request, Default::default()).map_err(|e| {
            let mut text = format!("the sandbox could not start: {}", e.message);
            if let Some(remedy) = e.remediation {
                text.push_str(&format!(" ({remedy})"));
            }
            io::Error::other(text)
        })?;
        let (stdin, stdout, stderr) = (
            process.take_stdin(),
            process.take_stdout(),
            process.take_stderr(),
        );
        let mut relays = Vec::new();
        // The command's input: ours relayed to it, or closed at once.
        // Nothing to join: a relay from our input ends when ours does.
        let stdin = match (self.io.0, stdin) {
            (Io::Piped, stdin) => stdin,
            (Io::Inherit, Some(mut stdin)) => {
                std::thread::spawn(move || {
                    let _ = io::copy(&mut io::stdin().lock(), &mut stdin);
                });
                None
            }
            _ => None,
        };
        let stdout = relayed(stdout, self.io.1, || Box::new(io::stdout()), &mut relays);
        let stderr = relayed(stderr, self.io.2, || Box::new(io::stderr()), &mut relays);
        Ok(Process::contained(process, stdin, stdout, stderr, relays))
    }

    /// Runs the command to its end, its streams relayed as asked.
    pub(crate) fn status(&self) -> io::Result<ExitStatus> {
        self.spawn()?.wait()
    }
}

/// A command's output stream as asked for: kept for the caller, relayed to
/// ours on a thread, or drained (not dropped: a closed pipe would fail the
/// command's writes). The thread goes on `relays`, to be joined once the
/// command is gone.
fn relayed(
    stream: Option<Box<dyn Read + Send>>,
    io: Io,
    ours: fn() -> Box<dyn Write + Send>,
    relays: &mut Vec<std::thread::JoinHandle<()>>,
) -> Option<Box<dyn Read + Send>> {
    let mut stream = stream?;
    match io {
        Io::Piped => Some(stream),
        Io::Inherit => {
            relays.push(std::thread::spawn(move || {
                let _ = io::copy(&mut stream, &mut ours());
            }));
            None
        }
        Io::Null => {
            relays.push(std::thread::spawn(move || {
                let _ = read_all(stream).join();
            }));
            None
        }
    }
}

/// How a container's command ended, as a process status.
pub(crate) fn status_of(result: WaitResult) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    let code = match result {
        WaitResult::Exited(code) => code as u32,
        // Never asked for; a deadline spent is a failure all the same.
        _ => 1,
    };
    ExitStatus::from_raw(code)
}

/// `program` and `args` as one command line, each quoted as the C runtime
/// reads it back. A batch file is run by `cmd.exe`, which reads its
/// arguments its own way: one that holds a character `cmd` would act on
/// is refused rather than passed.
fn command_line(program: &Path, args: &[String]) -> io::Result<OsString> {
    use crate::windows::appcontainer::quoted;
    let batch = program
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"));
    let mut line = OsString::new();
    if batch {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        for text in
            std::iter::once(&*program.to_string_lossy()).chain(args.iter().map(String::as_str))
        {
            if text.contains(['&', '|', '<', '>', '^', '(', ')', '%', '!', '"', '\n', '\r']) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "{text:?} cannot be given to a batch file: cmd.exe would read it as more than text"
                    ),
                ));
            }
        }
        line.push(quoted(
            Path::new(&root).join("System32\\cmd.exe").as_os_str(),
        ));
        line.push(" /d /s /c \"");
        line.push(quoted(program.as_os_str()));
        for arg in args {
            line.push(" ");
            line.push(quoted(OsStr::new(arg)));
        }
        line.push("\"");
    } else if program
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|n| n.eq_ignore_ascii_case("cmd.exe"))
    {
        // cmd.exe reads its own command line, not as the C runtime does:
        // an argument goes as it is, in quotes where it holds a space (its
        // `/s` switch is how a script with quotes of its own is passed).
        line.push(quoted(program.as_os_str()));
        for arg in args {
            line.push(" ");
            if arg.contains([' ', '\t']) && !arg.starts_with('"') {
                line.push(format!("\"{arg}\""));
            } else {
                line.push(arg);
            }
        }
    } else {
        line.push(quoted(program.as_os_str()));
        for arg in args {
            line.push(" ");
            line.push(quoted(OsStr::new(arg)));
        }
    }
    Ok(line)
}

/// The container's network as Kule's [`Network`] says, and the app
/// container profile its proxy connections may reach, if any: egress
/// shut and the proxy named for [`Network::Proxy`], so a connection past
/// the proxy fails at once; nothing on the way to this computer's own
/// listeners (`host_loopback`) in any mode, the proxy's being reached by
/// its profile instead. Listeners inside stay allowed where the network is
/// not shut: this computer cannot reach them (measured), so they are the
/// task's alone.
fn network_policy(network: Network) -> (NetworkPolicy, Option<String>) {
    let action = |allow: bool| {
        if allow {
            NetworkAction::Allow
        } else {
            NetworkAction::Deny
        }
    };
    let (egress, ingress, proxy) = match network {
        Network::Deny => (false, false, None),
        Network::Allow => (true, true, None),
        Network::Proxy(port) => (false, true, Some(port)),
    };
    let policy = NetworkPolicy {
        egress: Some(NetworkEgressPolicy {
            default: Some(action(egress)),
            allow: None,
            deny: None,
        }),
        ingress: Some(NetworkIngressPolicy {
            default: Some(action(ingress)),
            host_loopback: Some(action(false)),
        }),
        runtime_config: proxy.map(|port| NetworkRuntimeConfig {
            network_proxy: Some(format!("http://127.0.0.1:{port}")),
        }),
    };
    let peer = proxy.map(|_| crate::windows::appcontainer::PROFILE.to_owned());
    (policy, peer)
}

/// Where the person's tools are, readable to every command: Windows and
/// the program folders, what is on `PATH` and the folders toolchains and
/// package managers install themselves in. Nothing that holds the home
/// itself.
fn tool_roots() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut roots: Vec<PathBuf> = [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
    ]
    .into_iter()
    .filter_map(var)
    .collect();
    if let Some(home) = crate::osfs::person_home() {
        for rel in [
            ".cargo", ".rustup", ".dotnet", ".bun", ".deno", ".pyenv", "scoop", "go",
        ] {
            roots.push(home.join(rel));
        }
    }
    if let Some(local) = var("LOCALAPPDATA") {
        for rel in [
            "Programs",
            "Microsoft\\WindowsApps",
            "Microsoft\\WinGet",
            "pnpm",
            "Yarn",
            "pip",
            "uv",
            "nvm",
            "Volta",
            "fnm_multishells",
        ] {
            roots.push(local.join(rel));
        }
    }
    if let Some(roaming) = var("APPDATA") {
        for rel in ["npm", "nvm", "uv", "Python"] {
            roots.push(roaming.join(rel));
        }
    }
    if let Some(data) = var("ProgramData") {
        roots.push(data.join("chocolatey"));
    }
    roots.extend(super::tools_from_env());
    let home = crate::osfs::person_home();
    roots.retain(|r| {
        r.is_absolute()
            && r.parent().is_some()
            && r.is_dir()
            && !home.as_deref().is_some_and(|h| h.starts_with(r))
    });
    roots.sort();
    roots.dedup();
    // One inside another is the other's.
    let all = roots.clone();
    roots.retain(|r| !all.iter().any(|o| o != r && r.starts_with(o)));
    roots
}

impl Sandbox {
    /// The container for `program` with `args`, from what this sandbox
    /// grants and hides (see the module's notes).
    pub(super) fn contained(&self, program: &str, args: &[String]) -> Result<Container> {
        let mut readwrite: Vec<PathBuf> = self.writable().into_iter().map(Path::to_owned).collect();
        readwrite.extend(self.layer.iter().cloned());
        let mut readonly: Vec<PathBuf> = self.see_through.clone();
        readonly.extend(self.protected.iter().cloned());
        readonly.extend(self.beside_links.iter().cloned());
        readonly.extend(tool_roots());
        // A grant that is not there grants nothing, and may be refused.
        readonly.retain(|p| p.exists());
        readwrite.sort();
        readwrite.dedup();
        readonly.sort();
        readonly.dedup();
        readonly.retain(|p| !readwrite.contains(p));
        let grants: Vec<&Path> = readwrite
            .iter()
            .chain(&readonly)
            .map(PathBuf::as_path)
            .collect();
        // Hidden inside a grant is denied by name; hidden on the way to a
        // grant (Kule's home above the copy) is simply not granted, and a
        // denial there would take the grant away.
        let mut denied: Vec<PathBuf> = self
            .hidden
            .iter()
            .filter(|h| grants.iter().any(|g| h.starts_with(g)))
            .filter(|h| !grants.iter().any(|g| *g != h.as_path() && g.starts_with(h)))
            .cloned()
            .collect();
        denied.sort();
        denied.dedup();
        // Every folder on the way to a grant, listed and not read, so that
        // paths resolve (measured: without, `dir` and absolute batch files
        // fail).
        let mut enumerate: Vec<PathBuf> = grants
            .iter()
            .flat_map(|g| g.ancestors().skip(1))
            .filter(|a| a.parent().is_some() || a.is_absolute())
            .filter(|a| !grants.contains(a))
            .map(Path::to_owned)
            .collect();
        enumerate.sort();
        enumerate.dedup();
        // Resolved paths carry the `\\?\` prefix, which the programs a
        // task runs do not take: the container gets them plain.
        let plain_all =
            |paths: Vec<PathBuf>| -> Vec<PathBuf> { paths.iter().map(|p| plain(p)).collect() };
        let mut env = Environment::default();
        for (name, value) in self.environment()? {
            let value = if value.to_string_lossy().starts_with("\\\\?\\") {
                plain(Path::new(&value)).into_os_string()
            } else {
                value
            };
            env.set(OsStr::new(&name), &value);
        }
        Ok(Container {
            program: program.to_owned(),
            args: args.to_vec(),
            cwd: plain(&self.copy),
            env,
            readwrite: plain_all(readwrite),
            readonly: plain_all(readonly),
            denied: plain_all(denied),
            enumerate: plain_all(enumerate),
            network: self.network,
            io: (Io::Inherit, Io::Inherit, Io::Inherit),
        })
    }

    /// Why no command runs in a sandbox on this computer (see
    /// [`crate::windows::preconditions`]), or nothing.
    pub(super) fn refusals(&self) -> Vec<String> {
        if let Some(reason) = &self.refused_for_test {
            return vec![reason.clone()];
        }
        crate::windows::preconditions::check()
            .err()
            .unwrap_or_default()
    }
}

/// The refusal a command gets where [`Sandbox::refusals`] names reasons.
pub(super) fn refused(reasons: &[String]) -> Error {
    Error::Invalid(format!(
        "sandboxed commands are refused on this computer: {}. Kule does not run them less isolated",
        reasons.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_file_runs_under_cmd_and_takes_only_plain_text() {
        let line = command_line(Path::new("C:\\t\\npx.cmd"), &["a b".to_owned()]).unwrap();
        let line = line.to_string_lossy();
        assert!(
            line.contains("cmd.exe /d /s /c \"C:\\t\\npx.cmd \"a b\"\""),
            "{line}"
        );
        for bad in ["a&b", "x|y", "%PATH%", "q\"q"] {
            assert!(command_line(Path::new("C:\\t\\run.bat"), &[bad.to_owned()]).is_err());
        }
        let plain = command_line(Path::new("C:\\t\\node.exe"), &["a b\"c".to_owned()]).unwrap();
        assert_eq!(plain.to_string_lossy(), "C:\\t\\node.exe \"a b\\\"c\"");
        let shell = command_line(
            Path::new("C:\\Windows\\System32\\cmd.exe"),
            &[
                "/d".to_owned(),
                "/s".to_owned(),
                "/c".to_owned(),
                "type \"a b.txt\"".to_owned(),
            ],
        )
        .unwrap();
        assert_eq!(
            shell.to_string_lossy(),
            "C:\\Windows\\System32\\cmd.exe /d /s /c \"type \"a b.txt\"\""
        );
    }

    #[test]
    fn the_proxy_mode_shuts_egress_and_names_the_proxys_profile() {
        let (policy, peer) = network_policy(Network::Proxy(4321));
        assert_eq!(peer.as_deref(), Some("Kule.Proxy"));
        assert_eq!(
            policy.runtime_config.unwrap().network_proxy.as_deref(),
            Some("http://127.0.0.1:4321")
        );
        assert!(matches!(
            policy.egress.unwrap().default,
            Some(NetworkAction::Deny)
        ));
        let (shut, none) = network_policy(Network::Deny);
        assert!(none.is_none());
        assert!(matches!(
            shut.ingress.unwrap().default,
            Some(NetworkAction::Deny)
        ));
    }
}
