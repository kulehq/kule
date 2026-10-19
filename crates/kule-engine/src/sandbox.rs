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

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::caches::{self, Sources};
use crate::error::{Error, IoContext, Result};
use crate::protect::{Kept, PutBack};
use crate::task::Task;

/// Credentials and keys that commands in a task never get to read.
/// Environment variables passed on to everything the sandbox runs: what
/// shells and toolchains need to find themselves, and the person's locale
/// and terminal settings. Nothing that carries a credential or a socket.
pub const PASSED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TZ",
    "TERM",
    "COLORTERM",
    "NO_COLOR",
    "FORCE_COLOR",
    "EDITOR",
    "PAGER",
    "__CF_USER_TEXT_ENCODING",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_STATE_HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "GOPATH",
    "GOROOT",
    "JAVA_HOME",
    "NVM_DIR",
    "NVM_BIN",
    "PNPM_HOME",
    "VOLTA_HOME",
    "PYENV_ROOT",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
    "DOTNET_ROOT",
    "ANDROID_HOME",
    "SDKROOT",
    "DEVELOPER_DIR",
    "HOMEBREW_PREFIX",
    "HOMEBREW_CELLAR",
    "HOMEBREW_REPOSITORY",
];

fn passes(name: &str) -> bool {
    PASSED_ENV.contains(&name) || name.starts_with("LC_")
}

/// Credentials under the home directory, hidden from every command: no agent
/// profile may open them.
pub const HIDDEN_IN_HOME: &[&str] = &[
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
    ".config/hub",
    ".config/op",
    ".terraform.d/credentials.tfrc.json",
    ".vault-token",
    ".password-store",
    ".local/share/keyrings",
    "Library/Keychains",
    // Agents' logins; an agent's profile shows it its own.
    ".claude/.credentials.json",
    ".codex/auth.json",
    ".gemini/gemini-credentials.json",
    ".gemini/oauth_creds.json",
];

/// macOS services that act for a caller outside its sandbox: opening apps
/// and documents, Apple events, settings, the pasteboard, privacy grants,
/// background items and the window server. A command never reaches them.
#[cfg(target_os = "macos")]
const OUTSIDE_SERVICES: &[&str] = &[
    "com.apple.coreservices.launchservicesd",
    "com.apple.CoreServices.coreservicesd",
    "com.apple.coreservices.appleevents",
    "com.apple.cfprefsd.daemon",
    "com.apple.pasteboard.1",
    "com.apple.tccd",
    "com.apple.tccd.system",
    "com.apple.backgroundtaskmanagementagent",
    "com.apple.backgroundtaskmanagement.agent",
    "com.apple.windowserver.active",
];

/// What every sandboxed command may do on macOS, on top of nothing: the
/// profile starts from `(deny default)`. Processes and signals within the
/// sandbox; reading files (what is hidden is taken away after); the system
/// facts and services that toolchains need. Writes, the network and the GPU
/// are allowed after it, each as narrowly as the task needs.
#[cfg(target_os = "macos")]
const SEATBELT_BASE: &str = r#"(allow process-exec process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow file-read* file-map-executable file-ioctl)
(allow system-fsctl)
(allow system-socket)
(allow network*)
(allow ipc-posix-sem ipc-posix-shm*)
(allow user-preference-read)
(allow pseudo-tty)
(allow system-mac-syscall (mac-policy-name "vnguard"))
(allow system-mac-syscall (require-all (mac-policy-name "Sandbox") (mac-syscall-number 67)))
(allow iokit-open (iokit-registry-entry-class "RootDomainUserClient"))
(allow iokit-open (iokit-user-client-class "IOSurfaceRootUserClient"))
(allow sysctl-write (sysctl-name "kern.grade_cputype"))
(allow sysctl-read
  (sysctl-name-prefix "hw.")
  (sysctl-name-prefix "machdep.cpu.")
  (sysctl-name-prefix "kern.proc.pgrp.")
  (sysctl-name-prefix "kern.proc.pid.")
  (sysctl-name-prefix "net.routetable")
  (sysctl-name "kern.argmax")
  (sysctl-name "kern.bootargs")
  (sysctl-name "kern.boottime")
  (sysctl-name "kern.iossupportversion")
  (sysctl-name "security.mac.lockdown_mode_state")
  (sysctl-name "kern.hostname")
  (sysctl-name "kern.maxfiles")
  (sysctl-name "kern.maxfilesperproc")
  (sysctl-name "kern.maxproc")
  (sysctl-name "kern.ngroups")
  (sysctl-name "kern.osproductversion")
  (sysctl-name "kern.osrelease")
  (sysctl-name "kern.ostype")
  (sysctl-name "kern.osvariant_status")
  (sysctl-name "kern.osversion")
  (sysctl-name "kern.safeboot")
  (sysctl-name "kern.secure_kernel")
  (sysctl-name "kern.sysv.semmns")
  (sysctl-name "kern.usrstack64")
  (sysctl-name "kern.version")
  (sysctl-name "sysctl.proc_cputype")
  (sysctl-name "sysctl.proc_translated")
  (sysctl-name "vm.loadavg")
  (sysctl-name "vm.pagesize"))
(allow mach-lookup
  (global-name "com.apple.system.opendirectoryd.libinfo")
  (global-name "com.apple.system.opendirectoryd.membership")
  (global-name "com.apple.system.DirectoryService.libinfo_v1")
  (global-name "com.apple.bsd.dirhelper")
  (global-name "com.apple.PowerManagement.control")
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.logd")
  (global-name "com.apple.logd.events")
  (global-name "com.apple.diagnosticd")
  (global-name "com.apple.analyticsd")
  (global-name "com.apple.secinitd")
  (global-name "com.apple.SecurityServer")
  (global-name "com.apple.trustd")
  (global-name "com.apple.trustd.agent")
  (global-name "com.apple.TrustEvaluationAgent")
  (global-name "com.apple.ocspd")
  (global-name "com.apple.networkd")
  (global-name "com.apple.SystemConfiguration.configd")
  (global-name "com.apple.SystemConfiguration.DNSConfiguration")
  (global-name "com.apple.dnssd.service")
  (global-name "com.apple.cfprefsd.agent")
  (local-name "com.apple.cfprefsd.agent")
  (global-name "com.apple.FSEvents"))
"#;

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
    /// Readable but never writable, even inside a writable path.
    protected: Vec<PathBuf>,
    network: Network,
    /// Environment variables passed on beyond [`PASSED_ENV`].
    passed_env: Vec<String>,
    /// Variables the sandbox sets itself, over the person's.
    env: Vec<(String, OsString)>,
    /// Where Kule notes protected paths it puts back after a run.
    kept: PathBuf,
    /// The task's home layer, the commands' `HOME` (see [`crate::home`]).
    layer: Option<PathBuf>,
    /// Whether commands may use the GPU.
    gpu: bool,
    /// The largest file a command may write, in bytes.
    file_size: Option<u64>,
    /// Marks this sandbox's refusals where the platform reports them (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    tag: String,
    /// The task's id: what every sandbox of the task carries (macOS), to
    /// find its processes by.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    task: String,
}

/// The service name no service has that a task's sandboxes, and only
/// they, may look up: how its processes are told from every other.
#[cfg(target_os = "macos")]
fn task_mark(task: &str) -> String {
    format!("dev.kule.task.{task}")
}

/// One operation the sandbox refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Denial {
    /// The name of the process that tried.
    pub process: String,
    /// What it tried, in the platform's words (`file-write-create`, …).
    pub operation: String,
    pub path: String,
    /// For a path Kule put back: where what it held instead was moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved: Option<String>,
}

impl Denial {
    /// A protected path the command changed, which Kule put back after the
    /// run (see [`Sandbox::keep_protected`]).
    pub fn put_back(put_back: &PutBack) -> Denial {
        Denial {
            process: "kule".to_owned(),
            operation: "file-write (put back)".to_owned(),
            path: put_back.path.display().to_string(),
            saved: put_back.saved.as_ref().map(|p| p.display().to_string()),
        }
    }
}

impl Sandbox {
    /// Writes only to the task's copy and temporary directory, network
    /// allowed; hidden is only Kule's home, apart from that copy and tmp.
    pub fn new(task: &Task) -> Result<Sandbox> {
        let tmp = task.tmp()?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        Ok(Sandbox {
            copy: real(&task.copy())?,
            tmp: real(&tmp)?,
            writable: Vec::new(),
            // Kule's home, but for the task's own copy and tmp: other tasks'
            // copies, the policy, the key that signs every task's history and
            // the daemon's token are none of a task's business.
            hidden: vec![real_or_planned(&task.home())?],
            protected: Vec::new(),
            network: Network::Allow,
            passed_env: Vec::new(),
            env: Vec::new(),
            kept: task.home().join("kept"),
            layer: None,
            gpu: true,
            file_size: None,
            tag: format!("kule-{}-{nanos:x}", task.id()),
            task: task.id().to_owned(),
        })
    }

    /// What the sandbox refused the commands it ran within the last
    /// `window`. `None` where the platform does not report refusals to an
    /// unprivileged process (Linux); the command still sees each refusal as
    /// an error.
    pub fn denials(&self, window: Duration) -> Result<Option<Vec<Denial>>> {
        self.platform_denials(window)
    }

    /// [`Sandbox::new`] plus the defaults: `/tmp` writable, package caches
    /// of the task's own, credentials in the home directory hidden. Shells
    /// and tools write to `/tmp` by hard-coded path, whatever `TMPDIR`
    /// says, and nothing there belongs to the workspace.
    pub fn for_task(task: &Task) -> Result<Sandbox> {
        Sandbox::for_task_with(task, Sources::from_env())
    }

    /// [`Sandbox::for_task`] for the person whose home and caches
    /// `sources` names.
    pub fn for_task_with(task: &Task, sources: Option<Sources>) -> Result<Sandbox> {
        let mut sandbox = Sandbox::new(task)?;
        let limits = task.limits_in_force()?;
        if !limits.gpu {
            sandbox.deny_gpu();
        }
        // No one file outgrows the disk limit between two measures of it.
        if let Some(mb) = limits.disk_mb {
            sandbox.limit_file_size(mb.saturating_mul(1024 * 1024));
        }
        let tmp = Path::new("/tmp");
        if tmp.is_dir() {
            sandbox.allow_write(tmp)?;
        }
        if let Some(sources) = sources {
            for secret in HIDDEN_IN_HOME {
                sandbox.hide(&sources.home.join(secret))?;
            }
            sandbox.use_task_caches(&sources)?;
        }
        Ok(sandbox)
    }

    /// Points package managers at caches in the task's tmp, and hides the
    /// registry credentials kept next to cargo's. The person's own caches
    /// stay read-only (see the `caches` module).
    pub fn use_task_caches(&mut self, sources: &Sources) -> Result<()> {
        for file in ["credentials.toml", "credentials"] {
            self.hide(&sources.cargo_home.join(file))?;
        }
        self.env = caches::prepare(&self.tmp, sources)?;
        Ok(())
    }

    /// Commands cannot use the GPU: on macOS they cannot open its devices
    /// (Metal, and OpenCL through it); on Linux `/dev/dri` and NVIDIA's
    /// devices stay read-only, which leaves them unusable.
    pub fn deny_gpu(&mut self) {
        self.gpu = false;
    }

    /// A command cannot write a file larger than `bytes`: the write fails
    /// (`EFBIG`) past it.
    pub fn limit_file_size(&mut self, bytes: u64) {
        self.file_size = Some(bytes);
    }

    pub fn allow_write(&mut self, path: &Path) -> Result<()> {
        self.writable.push(real(path)?);
        Ok(())
    }

    /// Keeps `path` readable but not writable, even inside a writable path:
    /// an agent's own settings, where hooks and MCP servers that run outside
    /// the sandbox later are configured. A path that does not exist yet is
    /// protected too. A link is protected where it is as well as where it
    /// leads: settings linked in from a dotfiles repository cannot be
    /// swapped for a file of the command's own.
    pub fn protect(&mut self, path: &Path) -> Result<()> {
        let resolved = real_or_planned(path)?;
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            let here = real_or_planned(parent)?.join(name);
            if here != resolved {
                self.protected.push(here);
            }
        }
        self.protected.push(resolved);
        Ok(())
    }

    /// Notes the protected paths this platform cannot keep read-only, to be
    /// put back when the run is over: on Linux, those inside a writable
    /// directory that cannot be mounted read-only (see [`Kept`] and
    /// [`protects_in_place`]). Call it before the command starts, and
    /// restore once everything it started is gone.
    ///
    /// A path too large to note (a plugin directory of hundreds of
    /// megabytes) is hidden from the command instead: it cannot change what
    /// it cannot reach, though it does without it.
    pub fn keep_protected(&mut self) -> Result<Kept> {
        let mounted = self.mounted_read_only();
        let (paths, too_large): (Vec<PathBuf>, Vec<PathBuf>) = self
            .protected_in_writable()
            .filter(|p| cfg!(target_os = "linux") && !mounted.contains(p))
            .cloned()
            .partition(|p| crate::protect::fits(p));
        for path in too_large {
            self.hidden.push(path);
        }
        Kept::new(&self.kept, paths)
    }

    /// Protected paths strictly inside a writable root, which the sandbox's
    /// rules alone cannot keep read-only on Linux.
    fn protected_in_writable(&self) -> impl Iterator<Item = &PathBuf> {
        let writable = self.writable();
        self.protected.iter().filter(move |p| {
            writable
                .iter()
                .any(|root| p.starts_with(root) && *p != root)
        })
    }

    /// The protected paths a command's mount namespace makes read-only:
    /// those inside a writable root that exist and are not links, where the
    /// kernel allows the namespace. Linux only; empty elsewhere.
    fn mounted_read_only(&self) -> Vec<PathBuf> {
        #[cfg(target_os = "linux")]
        if crate::namespace::mounts_available() {
            return self
                .protected_in_writable()
                .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| !m.file_type().is_symlink()))
                .cloned()
                .collect();
        }
        Vec::new()
    }

    /// Hides `path` from reading and writing. A path that does not exist yet
    /// is hidden too, in case something creates it.
    pub fn hide(&mut self, path: &Path) -> Result<()> {
        self.hidden.push(real_or_planned(path)?);
        Ok(())
    }

    /// Takes `path` off the hidden list, for a command that needs it (an
    /// agent reading its own login from the keychain). Hidden paths inside
    /// it stay hidden.
    pub fn reveal(&mut self, path: &Path) -> Result<()> {
        let path = real_or_planned(path)?;
        self.hidden.retain(|hidden| hidden != &path);
        Ok(())
    }

    pub fn set_network(&mut self, network: Network) {
        self.network = network;
    }

    /// Points the commands' `HOME` at the task's home layer over `real`
    /// (and `XDG_CONFIG_HOME`, where it is the real home's `.config`), and
    /// lets them write there.
    pub fn use_home(&mut self, layer: &Path, real_home: &Path) -> Result<()> {
        let layer = real(layer)?;
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
            && Path::new(&xdg) == real_home.join(".config")
        {
            self.env
                .push(("XDG_CONFIG_HOME".to_owned(), layer.join(".config").into()));
        }
        self.env.push(("HOME".to_owned(), layer.clone().into()));
        self.layer = Some(layer);
        Ok(())
    }

    /// Sends the network through `proxy`, with what its commands need in
    /// their environment: the placeholders of the task's secrets, and the
    /// authority to trust where the proxy looks inside TLS.
    pub fn use_proxy(&mut self, proxy: &crate::Proxy) {
        self.network = Network::Proxy(proxy.port());
        self.env.extend(proxy.env().iter().cloned());
    }

    /// Passes environment variable `name` on to what the sandbox runs, such
    /// as an agent's own API key; others are left out but for the few tools
    /// need (see [`PASSED_ENV`]).
    pub fn pass_env(&mut self, name: &str) {
        self.passed_env.push(name.to_owned());
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

    /// Ends what still runs in this sandbox: a process that left its
    /// command's session and parent, which ending the command's process
    /// group does not reach. Returns how many it ended. A process of the
    /// person's own, outside the sandbox, is never one of them.
    pub fn end_strays(&self) -> usize {
        let strays = self.strays();
        crate::resources::kill(&strays);
        strays.len()
    }

    /// macOS knows which processes a task's sandboxes hold: those whose
    /// sandbox lets them look up the task's mark, which only its own do.
    #[cfg(target_os = "macos")]
    fn strays(&self) -> Vec<i32> {
        unsafe extern "C" {
            fn sandbox_check(
                pid: libc::pid_t,
                operation: *const libc::c_char,
                filter: libc::c_int,
                ...
            ) -> libc::c_int;
        }
        const FILTER_NONE: libc::c_int = 0;
        const FILTER_GLOBAL_NAME: libc::c_int = 2;
        const NO_REPORT: libc::c_int = 0x4000_0000;
        let Ok(mark) = std::ffi::CString::new(task_mark(&self.task)) else {
            return Vec::new();
        };
        let me = std::process::id() as i32;
        crate::ports::macos::all_pids()
            .into_iter()
            .filter(|&pid| pid > 1 && pid != me)
            .filter(|&pid| {
                // SAFETY: sandbox_check only reads its arguments.
                unsafe {
                    sandbox_check(pid, std::ptr::null(), FILTER_NONE | NO_REPORT) == 1
                        && sandbox_check(
                            pid,
                            c"mach-lookup".as_ptr(),
                            FILTER_GLOBAL_NAME | NO_REPORT,
                            mark.as_ptr(),
                        ) == 0
                }
            })
            .collect()
    }

    /// Linux keeps no list of a Landlock domain's processes: a stray is a
    /// process of ours under no-new-privileges (as every sandboxed one is,
    /// and a person's shell is not) that works in the task's folders, by
    /// its directory, its program or a file it holds open.
    #[cfg(target_os = "linux")]
    fn strays(&self) -> Vec<i32> {
        use std::fs;
        let dirs: Vec<PathBuf> = [&self.copy, &self.tmp]
            .into_iter()
            .chain(self.layer.iter())
            .filter_map(|d| fs::canonicalize(d).ok())
            .collect();
        let inside = |link: PathBuf| {
            fs::read_link(link).is_ok_and(|to| dirs.iter().any(|d| to.starts_with(d)))
        };
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() }.to_string();
        let me = std::process::id() as i32;
        let Ok(procs) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        procs
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
            .filter(|&pid| pid > 1 && pid != me)
            .filter(|&pid| {
                let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
                    return false;
                };
                let field = |name: &str| {
                    status
                        .lines()
                        .find_map(|l| l.strip_prefix(name))
                        .map(|v| v.split_whitespace().next().unwrap_or("").to_owned())
                };
                if field("Uid:").as_deref() != Some(uid.as_str())
                    || field("NoNewPrivs:").as_deref() != Some("1")
                {
                    return false;
                }
                let base = PathBuf::from(format!("/proc/{pid}"));
                inside(base.join("cwd"))
                    || inside(base.join("exe"))
                    || fs::read_dir(base.join("fd"))
                        .is_ok_and(|fds| fds.flatten().any(|fd| inside(fd.path())))
            })
            .collect()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn strays(&self) -> Vec<i32> {
        Vec::new()
    }

    /// A command that runs `program` inside the sandbox, in the copy, with
    /// `TMPDIR` pointing at the task's temporary directory.
    pub fn command(&self, program: &str, args: &[String]) -> Result<Command> {
        use std::os::unix::process::CommandExt;

        let mut command = self.platform_command(program, args)?;
        // A session of its own: no controlling terminal, so nothing it runs
        // can push keystrokes into the person's terminal (TIOCSTI), and its
        // whole process group can be stopped when it is done.
        // SAFETY: setsid is async-signal-safe and touches nothing but the
        // child between fork and exec.
        let file_size = self.file_size;
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some(bytes) = file_size {
                    let limit = libc::rlimit {
                        rlim_cur: bytes as libc::rlim_t,
                        rlim_max: bytes as libc::rlim_t,
                    };
                    if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    // Past it the write fails rather than the program dying
                    // of SIGXFSZ.
                    libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
                }
                Ok(())
            });
        }
        // Only what tools need to find themselves and the person's
        // settings, and what was passed on purpose: tokens and agent sockets
        // in the environment (GITHUB_TOKEN, AWS_*, SSH_AUTH_SOCK) stay out.
        command.env_clear();
        for (name, value) in std::env::vars_os() {
            let Some(text) = name.to_str() else { continue };
            if passes(text) || self.passed_env.iter().any(|p| p == text) {
                command.env(&name, value);
            }
        }
        command.current_dir(&self.copy).env("TMPDIR", &self.tmp);
        command.envs(self.env.iter().map(|(k, v)| (k, v)));
        // Servers listen on this computer only (see `loopback`).
        let set: Vec<(OsString, OsString)> = command
            .get_envs()
            .filter_map(|(k, v)| Some((k.to_owned(), v?.to_owned())))
            .collect();
        let current = |name: &str| set.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        command.envs(crate::loopback::hooks(&self.tmp, current)?);
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
            // Node's built-in fetch ignores the variables above unless told
            // to (Node 24+); without the proxy its connections just fail.
            command.env("NODE_USE_ENV_PROXY", "1");
        }
        Ok(command)
    }
}

/// Whether a sandbox with [`Network::Proxy`] or [`Network::Deny`] keeps
/// every connection off the network but the proxy's. On Linux that takes a
/// network namespace of its own, which some systems refuse to unprivileged
/// users; there only the port is checked, any address, and datagrams pass.
pub fn isolates_network() -> bool {
    #[cfg(target_os = "linux")]
    return crate::namespace::available();
    #[cfg(not(target_os = "linux"))]
    return cfg!(target_os = "macos");
}

/// Whether a protected path inside a writable directory stays read-only
/// while the command runs, rather than being put back after it. On Linux
/// that takes a mount namespace of its own, refused where user namespaces
/// are (see [`isolates_network`]); a protected path that is a link, or does
/// not exist yet, is put back there either way.
pub fn protects_in_place() -> bool {
    #[cfg(target_os = "linux")]
    return crate::namespace::mounts_available();
    #[cfg(not(target_os = "linux"))]
    return cfg!(target_os = "macos");
}

/// Appends refusals to the task's `sandbox.jsonl`.
pub fn record_denials(task: &Task, denials: &[Denial]) -> Result<()> {
    if denials.is_empty() {
        return Ok(());
    }
    let path = denials_path(task);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .at(&path)?;
    for denial in denials {
        let line = serde_json::to_string(denial).map_err(|source| Error::State {
            path: path.clone(),
            source,
        })?;
        writeln!(file, "{line}").at(&path)?;
    }
    Ok(())
}

/// Every refusal recorded for the task, oldest first.
pub fn denials(task: &Task) -> Result<Vec<Denial>> {
    let path = denials_path(task);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(&path),
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            serde_json::from_str(line).map_err(|source| Error::State {
                path: path.clone(),
                source,
            })
        })
        .collect()
}

fn denials_path(task: &Task) -> PathBuf {
    task.dir().join("sandbox.jsonl")
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
        // A task's id is hex; anything else would not reach the profile.
        let mark = if self.task.chars().all(|c| c.is_ascii_alphanumeric()) {
            format!(
                "(allow mach-lookup (global-name \"{}\"))\n",
                task_mark(&self.task)
            )
        } else {
            String::new()
        };
        let mut text = format!(
            "(version 1)\n\
             (deny default {tag})\n\
             {SEATBELT_BASE}\
             {mark}\
             (deny file-write* {tag})\n\
             (allow file-write*\n  \
               (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/dtracehelper\")\n  \
               (literal \"/dev/tty\") (regex #\"^/dev/fd/\")",
        );
        for (i, path) in self.writable().into_iter().enumerate() {
            text.push_str(&format!("\n  (subpath (param \"W{i}\"))"));
            params.push((format!("W{i}"), path));
        }
        text.push_str(")\n");
        // Unix sockets lead to daemons running as the user (Kule's own,
        // Docker, ssh-agent, gpg-agent): none of them may take orders from a
        // task. Sockets in the copy and tmp, such as a test server's, stay.
        // Before the IP rules, which win for TCP.
        text.push_str(&format!(
            "(deny network-outbound (remote unix-socket) {tag})\n\
             (allow network-outbound (remote unix-socket (subpath (param \"C\"))))\n\
             (allow network-outbound (remote unix-socket (subpath (param \"T\"))))\n"
        ));
        params.push(("C".to_owned(), self.copy.as_path()));
        params.push(("T".to_owned(), self.tmp.as_path()));
        // Terminals: its own controlling one, and those it makes; never the
        // person's other terminals, to read their keys or write into them.
        text.push_str(&format!(
            "(deny file-read* file-write* file-ioctl (regex #\"^/dev/ttys[0-9]+$\") {tag})\n\
             (allow pseudo-tty)\n\
             (allow file-read* file-write* file-ioctl (literal \"/dev/ptmx\"))\n\
             (allow file-read* file-write* file-ioctl\n  \
               (require-all (regex #\"^/dev/ttys[0-9]+$\") (extension \"com.apple.sandbox.pty\")))\n"
        ));
        // What the system's services would do for it outside the sandbox:
        // open apps and documents, drive other apps, write other programs'
        // settings, read or fill the pasteboard, ask for privacy grants,
        // reach the window server.
        text.push_str(&format!(
            "(deny lsopen {tag})\n(deny appleevent-send {tag})\n(deny user-preference-write {tag})\n"
        ));
        for service in OUTSIDE_SERVICES {
            text.push_str(&format!(
                "(deny mach-lookup (global-name \"{service}\") {tag})\n"
            ));
        }
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
        // The GPU's user clients: Apple silicon's (`AGXDeviceUserClient` is
        // one) and the IOAccelerator family's of Intel Macs. A class matches
        // its subclasses too.
        // Metal also compiles its shaders in a service of its own.
        let gpu = "(iokit-user-client-class \"IOGPUDeviceUserClient\" \"IOAccelDevice2\" \"IOAccelSharedUserClient2\" \"IOAccelContext2\" \"IOAccelCommandQueue2\")";
        if self.gpu {
            text.push_str(&format!(
                "(allow iokit-open {gpu})\n\
                 (allow mach-lookup (xpc-service-name \"com.apple.MTLCompilerService\"))\n"
            ));
        } else {
            text.push_str(&format!("(deny iokit-open {gpu} {tag})\n"));
        }
        if self.network != Network::Allow {
            // Name lookups would carry data out past the proxy, which
            // resolves names itself.
            text.push_str(&format!(
                "(deny mach-lookup (global-name \"com.apple.dnssd.service\") {tag})\n"
            ));
        }
        for (i, path) in self.hidden.iter().enumerate() {
            text.push_str(&format!(
                "(deny file-read* file-write* (subpath (param \"H{i}\")) {tag})\n"
            ));
            params.push((format!("H{i}"), path.as_path()));
        }
        // The copy and tmp sit in Kule's home, which is hidden: they come back
        // last, so a hidden path never takes them away. So does the home
        // layer, which is the task's too.
        text.push_str(
            "(allow file-read* file-write* (subpath (param \"C\")) (subpath (param \"T\")))\n",
        );
        if let Some(layer) = &self.layer {
            text.push_str("(allow file-read* file-write* (subpath (param \"L\")))\n");
            params.push(("L".to_owned(), layer.as_path()));
        }
        for (i, path) in self.protected.iter().enumerate() {
            text.push_str(&format!(
                "(deny file-write* (subpath (param \"P{i}\")) {tag})\n"
            ));
            params.push((format!("P{i}"), path.as_path()));
        }
        // Resolving the working directory (realpath) looks at every directory
        // on the way to the copy and tmp, inside the hidden home too; what
        // those directories list stays hidden.
        let ancestors: std::collections::BTreeSet<&Path> = self
            .copy
            .ancestors()
            .skip(1)
            .chain(self.tmp.ancestors().skip(1))
            .chain(self.layer.iter().flat_map(|l| l.ancestors().skip(1)))
            .collect();
        for (i, dir) in ancestors.into_iter().enumerate() {
            text.push_str(&format!(
                "(allow file-read-metadata (literal (param \"A{i}\")))\n"
            ));
            params.push((format!("A{i}"), dir));
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
/// on the next line). A denial the kernel already reported comes as
/// `1 duplicate report for Sandbox: …`, and is often the only one left in
/// the log.
#[cfg(target_os = "macos")]
fn parse_denial(message: &str) -> Option<Denial> {
    let first = message.lines().next()?;
    let line = &first[first.find("Sandbox: ")? + "Sandbox: ".len()..];
    let (process, rest) = line.split_once('(')?;
    let (_, rest) = rest.split_once(") deny(")?;
    let (_, rest) = rest.split_once(") ")?;
    let (operation, path) = rest.split_once(' ').unwrap_or((rest, ""));
    Some(Denial {
        process: process.to_owned(),
        operation: operation.to_owned(),
        path: path.to_owned(),
        saved: None,
    })
}

#[cfg(target_os = "linux")]
impl Sandbox {
    fn platform_command(&self, program: &str, args: &[String]) -> Result<Command> {
        use std::os::unix::process::CommandExt;

        let mut ruleset = Some(self.landlock_ruleset()?);
        let mut command = Command::new(program);
        command.args(args);
        // Namespaces of its own first: its uid map is written under /proc,
        // and Landlock would refuse that afterwards, and any mount.
        use crate::namespace::{Isolation, Net};
        let net = match self.network {
            Network::Allow => Net::Shared,
            _ if !crate::namespace::available() => Net::Shared,
            Network::Deny => Net::Own(None),
            Network::Proxy(port) => Net::Own(Some(port)),
        };
        let read_only = self.mounted_read_only();
        let network_shared = matches!(net, Net::Shared);
        if matches!(net, Net::Own(_)) || !read_only.is_empty() {
            let isolation = Isolation::new(net, &read_only).at(Path::new("/proc/self/ns"))?;
            // SAFETY: enter only makes system calls on buffers and
            // descriptors prepared before the fork.
            unsafe {
                command.pre_exec(move || isolation.enter());
            }
        }
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
        // Without a network of its own, the command's servers are held to
        // loopback by Kule doing their binds (see `bind_filter`); after
        // Landlock, which set no_new_privs, as seccomp needs.
        if network_shared && crate::bind_filter::available() {
            let filter = crate::bind_filter::BindFilter::new().at(Path::new("/proc/self"))?;
            // SAFETY: enter only makes system calls on data prepared
            // before the fork.
            unsafe {
                command.pre_exec(move || filter.enter());
            }
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
        // only inside its existing children. Protected paths are not carved,
        // for that very cost: an agent writes its state next to its
        // settings. A protected path that is a writable root, or holds one,
        // stays read-only; one inside a writable directory is mounted
        // read-only for the command, or put back after the run where it
        // cannot be (`keep_protected`).
        // The copy and tmp sit in Kule's home, which is hidden: they come
        // back after the carving, as on macOS.
        let mut writable = carve_all(
            self.writable
                .iter()
                .filter(|root| !self.protected.iter().any(|p| root.starts_with(p)))
                .cloned()
                .collect(),
        );
        writable.extend([self.copy.clone(), self.tmp.clone()]);
        writable.extend(self.layer.clone());
        let mut devices: Vec<PathBuf> = [
            "/dev/null",
            "/dev/zero",
            "/dev/full",
            "/dev/tty",
            "/dev/ptmx",
            "/dev/pts",
        ]
        .map(PathBuf::from)
        .to_vec();
        if self.gpu {
            devices.extend(gpu_devices());
        }
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

/// The GPU's devices: DRM's (`/dev/dri`, render and card nodes) and
/// NVIDIA's, those this computer has.
#[cfg(target_os = "linux")]
fn gpu_devices() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    if Path::new("/dev/dri").is_dir() {
        found.push(PathBuf::from("/dev/dri"));
    }
    if let Ok(dir) = std::fs::read_dir("/dev") {
        found.extend(
            dir.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("nvidia"))
                .map(|e| e.path()),
        );
    }
    found
}

/// `roots` with `hidden` taken out: a root that is an ancestor of `hidden`
/// is replaced by its children, recursively, and `hidden` itself dropped.
#[cfg(target_os = "linux")]
fn carve(roots: Vec<PathBuf>, hidden: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        // Inside `hidden` too: an earlier carve for a path within it may
        // have left its children here.
        if root.starts_with(hidden) {
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn denials_are_read_also_as_duplicate_reports() {
        let plain = "Sandbox: bash(89964) deny(1) file-write-create /Users/me/.zshrc\nkule-1-2";
        let repeated = "1 duplicate report for Sandbox: node(60494) deny(1) file-write-create /Users/me/.qwen\nkule-1-2";
        for (message, process, path) in [
            (plain, "bash", "/Users/me/.zshrc"),
            (repeated, "node", "/Users/me/.qwen"),
        ] {
            let denial = parse_denial(message).unwrap();
            assert_eq!(
                (
                    denial.process.as_str(),
                    denial.operation.as_str(),
                    denial.path.as_str()
                ),
                (process, "file-write-create", path)
            );
        }
        assert!(parse_denial("something else").is_none());
    }
}
