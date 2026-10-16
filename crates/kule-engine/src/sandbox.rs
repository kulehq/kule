//! The operating system's sandbox around everything a task runs.
//!
//! Writes are allowed only in the task's copy, its private temporary
//! directory and paths added on purpose (package caches by default). Secret
//! paths cannot be read at all, and the network can be shut. Child processes
//! inherit all of it. When the sandbox cannot be set up, nothing runs: there
//! is no silent fallback to running unsandboxed.
//!
//! macOS uses Seatbelt (`sandbox-exec`), Linux uses Landlock. Both need real
//! paths: Seatbelt matches resolved paths, and Landlock works on file
//! descriptors, so a symlink in the copy cannot lead a write outside it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::task::Task;

/// Credentials and keys that commands in a task never get to read.
const HIDDEN_IN_HOME: &[&str] = &[
    ".ssh",
    ".aws",
    ".azure",
    ".gnupg",
    ".kube",
    ".docker",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".git-credentials",
    ".config/gh",
    ".config/gcloud",
    ".config/git/credentials",
    "Library/Keychains",
];

/// Package caches: tools write here while building, and what lands here is
/// not part of the workspace. Only the ones that exist are opened.
const CACHES_IN_HOME: &[&str] = &[
    ".cargo/registry",
    ".cargo/git",
    ".npm",
    ".nuget/packages",
    ".cache",
    "Library/Caches",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Allow,
    /// No TCP connections or listeners, localhost included.
    Deny,
    /// TCP connections only to the task's proxy on this loopback port; the
    /// proxy variables point there. Local listeners stay allowed. Landlock
    /// matches the port only, not the address.
    Proxy(u16),
}

#[derive(Debug, Clone)]
pub struct Sandbox {
    copy: PathBuf,
    tmp: PathBuf,
    writable: Vec<PathBuf>,
    hidden: Vec<PathBuf>,
    network: Network,
    /// Marks this sandbox's refusals where the platform reports them.
    tag: String,
}

/// One operation the sandbox refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Denial {
    /// The name of the process that tried.
    pub process: String,
    /// What it tried, in the platform's words (`file-write-create`, …).
    pub operation: String,
    pub path: String,
}

impl Sandbox {
    /// Writes only to the task's copy and temporary directory, nothing
    /// hidden, network allowed.
    pub fn new(task: &Task) -> Result<Sandbox> {
        let tmp = task.tmp()?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        Ok(Sandbox {
            copy: real(&task.copy())?,
            tmp: real(&tmp)?,
            writable: Vec::new(),
            hidden: Vec::new(),
            network: Network::Allow,
            tag: format!("kule-{}-{nanos:x}", task.id()),
        })
    }

    /// What the sandbox refused the commands it ran within the last
    /// `window`. `None` where the platform does not report refusals to an
    /// unprivileged process (Linux); the command still sees each refusal as
    /// an error.
    pub fn denials(&self, window: Duration) -> Result<Option<Vec<Denial>>> {
        self.platform_denials(window)
    }

    /// [`Sandbox::new`] plus the defaults for the user's home directory:
    /// package caches writable, credentials hidden.
    pub fn for_task(task: &Task) -> Result<Sandbox> {
        let mut sandbox = Sandbox::new(task)?;
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for cache in CACHES_IN_HOME {
                let path = home.join(cache);
                if path.is_dir() {
                    sandbox.allow_write(&path)?;
                }
            }
            for secret in HIDDEN_IN_HOME {
                sandbox.hide(&home.join(secret))?;
            }
        }
        Ok(sandbox)
    }

    pub fn allow_write(&mut self, path: &Path) -> Result<()> {
        self.writable.push(real(path)?);
        Ok(())
    }

    /// Hides `path` from reading and writing. A path that does not exist yet
    /// is hidden too, in case something creates it.
    pub fn hide(&mut self, path: &Path) -> Result<()> {
        self.hidden.push(real_or_planned(path)?);
        Ok(())
    }

    pub fn set_network(&mut self, network: Network) {
        self.network = network;
    }

    /// Everything the sandbox lets commands write to.
    pub fn writable(&self) -> Vec<&Path> {
        [&self.copy, &self.tmp]
            .into_iter()
            .chain(&self.writable)
            .map(PathBuf::as_path)
            .collect()
    }

    pub fn hidden(&self) -> &[PathBuf] {
        &self.hidden
    }

    /// A command that runs `program` inside the sandbox, in the copy, with
    /// `TMPDIR` pointing at the task's temporary directory.
    pub fn command(&self, program: &str, args: &[String]) -> Result<Command> {
        let mut command = self.platform_command(program, args)?;
        command.current_dir(&self.copy).env("TMPDIR", &self.tmp);
        if let Network::Proxy(port) = self.network {
            let url = format!("http://127.0.0.1:{port}");
            for var in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                command.env(var, &url);
            }
            command.env_remove("NO_PROXY").env_remove("no_proxy");
        }
        Ok(command)
    }
}

/// The resolved form of an existing path.
fn real(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).at(path)
}

/// The resolved form of a path that may not exist: its nearest existing
/// ancestor resolved, the rest appended.
fn real_or_planned(path: &Path) -> Result<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match std::fs::canonicalize(existing) {
            Ok(resolved) => {
                return Ok(missing.iter().rev().fold(resolved, |p, part| p.join(part)));
            }
            Err(_) => match (existing.parent(), existing.file_name()) {
                (Some(parent), Some(name)) => {
                    missing.push(name.to_owned());
                    existing = parent;
                }
                _ => {
                    return Err(Error::Invalid(format!("cannot resolve {}", path.display())));
                }
            },
        }
    }
}

#[cfg(target_os = "macos")]
impl Sandbox {
    fn platform_command(&self, program: &str, args: &[String]) -> Result<Command> {
        let (profile, params) = self.seatbelt_profile();
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command.arg("-p").arg(profile);
        for (key, value) in params {
            command.arg("-D").arg(format!("{key}={}", value.display()));
        }
        command.arg(program).args(args);
        Ok(command)
    }

    /// Paths travel as `-D` parameters, never inside the profile text, so no
    /// path can change the profile's meaning. Later rules win, so the hidden
    /// paths come last and override any allowance. Every deny carries this
    /// sandbox's tag, so its refusals can be told apart in the system log.
    fn seatbelt_profile(&self) -> (String, Vec<(String, &Path)>) {
        let tag = format!("(with message \"{}\")", self.tag);
        let mut params = Vec::new();
        let mut text = format!(
            "(version 1)\n\
             (allow default)\n\
             (deny file-write* {tag})\n\
             (allow file-write*\n  \
               (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/dtracehelper\")\n  \
               (regex #\"^/dev/tty\") (regex #\"^/dev/fd/\")",
        );
        for (i, path) in self.writable().into_iter().enumerate() {
            text.push_str(&format!("\n  (subpath (param \"W{i}\"))"));
            params.push((format!("W{i}"), path));
        }
        text.push_str(")\n");
        match self.network {
            Network::Allow => {}
            Network::Deny => {
                text.push_str(&format!(
                    "(deny network-outbound (remote ip) {tag})\n(deny network-bind (local ip) {tag})\n"
                ));
            }
            Network::Proxy(port) => {
                text.push_str(&format!(
                    "(deny network-outbound (remote ip) {tag})\n(allow network-outbound (remote ip \"localhost:{port}\"))\n"
                ));
            }
        }
        for (i, path) in self.hidden.iter().enumerate() {
            text.push_str(&format!(
                "(deny file-read* file-write* (subpath (param \"H{i}\")) {tag})\n"
            ));
            params.push((format!("H{i}"), path.as_path()));
        }
        (text, params)
    }

    fn platform_denials(&self, window: Duration) -> Result<Option<Vec<Denial>>> {
        let seconds = window.as_secs() + 2;
        let predicate = format!(
            "sender == \"Sandbox\" AND eventMessage CONTAINS \"{}\"",
            self.tag
        );
        let log = Path::new("/usr/bin/log");
        let out = Command::new(log)
            .args([
                "show",
                "--last",
                &format!("{seconds}s"),
                "--style",
                "ndjson",
            ])
            .args(["--predicate", &predicate])
            .output()
            .at(log)?;
        let mut denials = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(denial) = entry["eventMessage"].as_str().and_then(parse_denial) else {
                continue;
            };
            if !denials.contains(&denial) {
                denials.push(denial);
            }
        }
        Ok(Some(denials))
    }
}

/// `Sandbox: bash(89964) deny(1) file-write-create /some/path` (and the tag
/// on the next line).
#[cfg(target_os = "macos")]
fn parse_denial(message: &str) -> Option<Denial> {
    let line = message.lines().next()?.strip_prefix("Sandbox: ")?;
    let (process, rest) = line.split_once('(')?;
    let (_, rest) = rest.split_once(") deny(")?;
    let (_, rest) = rest.split_once(") ")?;
    let (operation, path) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(Denial {
        process: process.to_owned(),
        operation: operation.to_owned(),
        path: path.to_owned(),
    })
}

#[cfg(target_os = "linux")]
impl Sandbox {
    fn platform_command(&self, program: &str, args: &[String]) -> Result<Command> {
        use std::os::unix::process::CommandExt;

        let mut ruleset = Some(self.landlock_ruleset()?);
        let mut command = Command::new(program);
        command.args(args);
        // SAFETY: the closure runs between fork and exec. It only makes the
        // two system calls that restrict the process (no_new_privs and
        // landlock_restrict_self) on a ruleset prepared before the fork.
        unsafe {
            command.pre_exec(move || {
                let ruleset = ruleset
                    .take()
                    .ok_or_else(|| std::io::Error::other("ruleset used twice"))?;
                let status = ruleset.restrict_self().map_err(std::io::Error::other)?;
                if status.ruleset != landlock::RulesetStatus::FullyEnforced {
                    return Err(std::io::Error::other(
                        "Landlock could not be fully enforced",
                    ));
                }
                Ok(())
            });
        }
        Ok(command)
    }

    /// Reads are allowed everywhere except the hidden paths. Landlock only
    /// allows, so the hidden paths are carved out: each ancestor of a hidden
    /// path is replaced by its children, the hidden one left out.
    fn landlock_ruleset(&self) -> Result<landlock::RulesetCreated> {
        use landlock::{
            ABI, Access, AccessFs, AccessNet, CompatLevel, Compatible, Ruleset, RulesetAttr,
            RulesetCreatedAttr, path_beneath_rules,
        };

        // ABI 3 (Linux 6.2) is the first to cover truncation; anything older
        // would let a command empty files outside the copy.
        let abi = ABI::V3;
        let fail = |e: &dyn std::fmt::Display| Error::Invalid(format!("Landlock: {e}"));
        let mut ruleset = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(abi))
            .map_err(|e| fail(&e))?;
        // TCP rules need ABI 4 (Linux 6.7); HardRequirement makes an older
        // kernel refuse instead of leaving the network open.
        let net = match self.network {
            Network::Allow => None,
            Network::Deny => Some(AccessNet::BindTcp | AccessNet::ConnectTcp),
            Network::Proxy(_) => Some(AccessNet::ConnectTcp.into()),
        };
        if let Some(net) = net {
            ruleset = ruleset.handle_access(net).map_err(|e| fail(&e))?;
        }
        let carve_all = |roots: Vec<PathBuf>| {
            self.hidden
                .iter()
                .fold(roots, |roots, hidden| carve(roots, hidden))
        };
        let readable = carve_all(vec![PathBuf::from("/")]);
        // Writable roots are carved the same way: an allowance for a
        // directory would otherwise reach a hidden path inside it. The cost
        // is that nothing new can be created directly in such a directory,
        // only inside its existing children.
        let writable = carve_all(self.writable().into_iter().map(Path::to_path_buf).collect());
        let devices = [
            "/dev/null",
            "/dev/zero",
            "/dev/full",
            "/dev/tty",
            "/dev/ptmx",
            "/dev/pts",
        ];
        let mut created = ruleset
            .create()
            .map_err(|e| fail(&e))?
            .set_compatibility(CompatLevel::BestEffort)
            .add_rules(path_beneath_rules(&readable, AccessFs::from_read(abi)))
            .map_err(|e| fail(&e))?
            .add_rules(path_beneath_rules(devices, AccessFs::from_all(abi)))
            .map_err(|e| fail(&e))?
            .add_rules(path_beneath_rules(&writable, AccessFs::from_all(abi)))
            .map_err(|e| fail(&e))?;
        if let Network::Proxy(port) = self.network {
            created = created
                .add_rule(landlock::NetPort::new(port, AccessNet::ConnectTcp))
                .map_err(|e| fail(&e))?;
        }
        Ok(created)
    }
}

/// `roots` with `hidden` taken out: a root that is an ancestor of `hidden`
/// is replaced by its children, recursively, and `hidden` itself dropped.
#[cfg(target_os = "linux")]
fn carve(roots: Vec<PathBuf>, hidden: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        if root == hidden {
            continue;
        }
        if !hidden.starts_with(&root) {
            out.push(root);
            continue;
        }
        let children = std::fs::read_dir(&root)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        out.extend(carve(children, hidden));
    }
    out
}

#[cfg(not(target_os = "macos"))]
impl Sandbox {
    fn platform_denials(&self, _window: Duration) -> Result<Option<Vec<Denial>>> {
        Ok(None)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl Sandbox {
    fn platform_command(&self, _program: &str, _args: &[String]) -> Result<Command> {
        Err(Error::Invalid("no sandbox on this platform yet".into()))
    }
}
