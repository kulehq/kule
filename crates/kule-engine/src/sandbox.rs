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

/// The folders [`Sandbox::hide_personal`] hides for `workspace` (resolved)
/// in `home`: [`PERSONAL_IN_HOME`], and the folder the workspace lies in
/// where that is one of the person's own.
pub(crate) fn personal_folders(workspace: &Path, home: &Path) -> Result<Vec<PathBuf>> {
    let home = real_or_planned(home)?;
    let mut folders: Vec<PathBuf> = PERSONAL_IN_HOME
        .iter()
        .map(|name| real_or_planned(&home.join(name)))
        .collect::<Result<_>>()?;
    if let Some(parent) = workspace.parent()
        && let Ok(within) = parent.strip_prefix(&home)
        && let Some(first) = within.components().next()
        && first.as_os_str() != "Library"
        && !within
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        folders.push(parent.to_owned());
    }
    Ok(folders)
}

/// Where the person's tools are, as the variables passed to commands name
/// them: each `PATH` entry, and the others that hold a path (`JAVA_HOME`,
/// `GOROOT`, …).
fn tools_from_env() -> Vec<PathBuf> {
    let mut tools: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    tools.extend(
        PASSED_ENV
            .iter()
            .filter(|name| **name != "PATH")
            .filter_map(std::env::var_os)
            .map(PathBuf::from),
    );
    tools.retain(|t| t.is_absolute());
    tools
}

/// The git directories of a workspace that is a linked worktree or keeps
/// its repository elsewhere (`.git` a file naming it), with the common
/// one a worktree's names; none for a repository of its own.
fn git_dirs(workspace: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(workspace.join(".git")) else {
        return Vec::new();
    };
    let Some(named) = text.lines().next().and_then(|l| l.strip_prefix("gitdir: ")) else {
        return Vec::new();
    };
    let Ok(dir) = std::fs::canonicalize(workspace.join(named.trim())) else {
        return Vec::new();
    };
    let mut dirs = vec![dir.clone()];
    if let Ok(common) = std::fs::read_to_string(dir.join("commondir"))
        && let Ok(common) = std::fs::canonicalize(dir.join(common.trim()))
    {
        dirs.push(common);
    }
    dirs
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
    ".config/rclone",
    ".m2/settings.xml",
    ".gradle/gradle.properties",
    ".gem/credentials",
    ".pgpass",
    ".my.cnf",
    ".oci",
    ".databrickscfg",
    ".cache/huggingface/token",
    ".huggingface/token",
    // What the person typed, in shells and other tools.
    ".zsh_history",
    ".bash_history",
    ".python_history",
    ".node_repl_history",
    ".psql_history",
    ".mysql_history",
    ".sqlite_history",
    ".lesshst",
    ".viminfo",
    ".local/share/fish/fish_history",
    // Browsers, mail and messages: sessions, cookies, what was read.
    "Library/Cookies",
    "Library/Safari",
    "Library/Mail",
    "Library/Messages",
    "Library/Application Support/Google/Chrome",
    "Library/Application Support/Firefox",
    "Library/Application Support/Microsoft Edge",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Arc",
    ".config/google-chrome",
    ".config/chromium",
    ".config/microsoft-edge",
    ".config/BraveSoftware",
    ".mozilla",
    ".thunderbird",
    // Editors' stores of signed-in sessions and extension secrets.
    "Library/Application Support/Code/User/globalStorage",
    ".config/Code/User/globalStorage",
    // Agents' logins; an agent's profile shows it its own.
    ".claude/.credentials.json",
    ".codex/auth.json",
    ".gemini/gemini-credentials.json",
    ".gemini/oauth_creds.json",
];

/// The person's own files under the home directory, and those kept in step
/// with other computers, hidden from every command: a task works on its
/// copy. A folder of tools inside one stays readable (see
/// [`Sandbox::hide_personal`]).
pub const PERSONAL_IN_HOME: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Movies",
    "Music",
    "Pictures",
    "Videos",
    "Library/Mobile Documents",
    "Library/CloudStorage",
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
///
/// Shared memory goes by name, and any process of the person's may hold a
/// segment: a command reads the system's notification table and makes the
/// segments that Python and PostgreSQL name at random, no other. The
/// keychain's service (`com.apple.SecurityServer`) is not here: it opens
/// items for its caller outside the sandbox, so it comes only with the
/// keychain's files (see [`Sandbox::reveal`]).
#[cfg(target_os = "macos")]
const SEATBELT_BASE: &str = r#"(allow process-exec process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow file-read* file-map-executable file-ioctl)
(allow system-fsctl)
(allow system-socket)
(allow network*)
(allow ipc-posix-sem)
(allow ipc-posix-shm-read* (ipc-posix-name "apple.shm.notification_center"))
(allow ipc-posix-shm*
  (ipc-posix-name-regex #"^/psm_[0-9a-f]+$")
  (ipc-posix-name-regex #"^/PostgreSQL\.[0-9]+$"))
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
  (global-name "com.apple.system.notification_center")
  (global-name "com.apple.system.logger")
  (global-name "com.apple.logd")
  (global-name "com.apple.logd.events")
  (global-name "com.apple.diagnosticd")
  (global-name "com.apple.analyticsd")
  (global-name "com.apple.secinitd")
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
    /// Readable, never writable, inside a hidden path: tools and the
    /// workspace's git directory in a folder hidden as the person's own
    /// (see [`Sandbox::hide_personal`]). A path hidden inside one of these,
    /// or one itself, stays hidden.
    see_through: Vec<PathBuf>,
    /// The folders hidden as the person's own, among `hidden`.
    personal: Vec<PathBuf>,
    /// The links next to the copy to the projects opened beside it, which
    /// commands may look at (macOS; what they lead to is in `see_through`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    beside_links: Vec<PathBuf>,
    /// The task's folder, resolved.
    workspace: PathBuf,
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
    /// Whether commands may listen for connections while their network
    /// goes through the proxy (macOS; elsewhere they always may, held to
    /// this computer).
    servers: bool,
    /// Devices given beyond those every command gets (Linux; see
    /// [`Sandbox::allow_device`]).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    devices: Vec<PathBuf>,
    /// Whether the keychain's files were revealed, and with them its
    /// service (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    keychain: bool,
    /// The largest file a command may write, in bytes.
    file_size: Option<u64>,
    /// Marks this sandbox's refusals where the platform reports them (macOS).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    tag: String,
    /// The task's id: what every sandbox of the task carries (macOS), to
    /// find its processes by.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    task: String,
    /// What this sandbox alone carries beside the task's mark (macOS), to
    /// find its own processes by (see [`Sandbox::end_own`]).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    own: String,
    /// The person accepted running with what this computer gives, where it
    /// gives less than Kule means to (see [`crate::isolation_gaps`]).
    accept_weaker: bool,
    /// The terminal Kule made for the command, which it may drive as its
    /// own (macOS; see [`Sandbox::terminal_command`]).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    terminal: Option<PathBuf>,
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
    /// Why the sandbox refuses it and what to do instead, where a tool
    /// cannot work without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// What a person can do about a refusal that keeps a tool from working.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn hint(operation: &str, path: &str) -> Option<String> {
    match (operation, path) {
        ("mach-lookup", "com.apple.trustd.agent") => Some(
            "macOS checks certificates in this service, which also fetches the \
             addresses a certificate names past the proxy, so it stays closed while \
             the network goes through it. A tool that checks certificates only \
             there cannot use HTTPS in a task; pip 24.2 and later works with \
             --use-deprecated=legacy-certs (or PIP_USE_DEPRECATED=legacy-certs \
             in the agent's profile)."
                .to_owned(),
        ),
        ("mach-lookup", "com.apple.windowserver.active" | "com.apple.windowserver") => Some(
            "A program wanted the screen (macOS's window server). Programs with \
             windows do not run in a task: its commands stay away from the screen, \
             the keyboard and the clipboard. LibreOffice is one, even with --headless; \
             macOS may say it quit unexpectedly. Read or convert such files with a \
             command-line tool instead (pandoc, or Python's zipfile for .odt and \
             .docx)."
                .to_owned(),
        ),
        ("network-bind", _) => Some(
            "A program wanted to listen for connections. On macOS a task's \
             programs do not while its network goes through the proxy: the \
             sandbox sees the port a server listens on, never the address, so \
             it cannot hold one to this computer. The person can allow servers \
             for the task (Settings → Limits, or `kule limits --task ID \
             servers=on`), which is weaker isolation they accept; a Unix socket \
             in the copy or its tmp works without."
                .to_owned(),
        ),
        (op, path) if op.starts_with("file-write") && path.starts_with("/private/tmp/") => Some(
            "On macOS a task's /tmp would be the system's, shared with the \
             person's programs and the other tasks, so its commands do not write \
             there. Write under $TMPDIR, the task's own temporary directory."
                .to_owned(),
        ),
        _ => None,
    }
}

/// Where the hints of a run's refusals wait for the agent's next prompt.
const HINTS_FILE: &str = "refused-hints.json";

impl Denial {
    /// A protected path the command changed, which Kule put back after the
    /// run (see [`Sandbox::keep_protected`]).
    pub fn put_back(put_back: &PutBack) -> Denial {
        Denial {
            process: "kule".to_owned(),
            operation: "file-write (put back)".to_owned(),
            path: put_back.path.display().to_string(),
            saved: put_back.saved.as_ref().map(|p| p.display().to_string()),
            hint: None,
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
            // the daemon's token are none of a task's business. And the
            // folder itself: the copy is what a command works on, with the
            // secrets in it masked, which the folder's are not.
            hidden: vec![
                real_or_planned(&task.home())?,
                real_or_planned(task.workspace())?,
            ],
            see_through: Vec::new(),
            personal: Vec::new(),
            beside_links: Vec::new(),
            workspace: real_or_planned(task.workspace())?,
            protected: Vec::new(),
            network: Network::Allow,
            passed_env: Vec::new(),
            env: Vec::new(),
            kept: task.home().join("kept"),
            layer: None,
            gpu: true,
            servers: false,
            devices: Vec::new(),
            keychain: false,
            file_size: None,
            tag: format!("kule-{}-{nanos:x}", task.id()),
            task: task.id().to_owned(),
            own: format!("{nanos:x}"),
            // A choice that cannot be read is no choice.
            accept_weaker: crate::isolation::isolation(&task.home()).is_ok_and(|i| i.accept_weaker),
            terminal: None,
        })
    }

    /// What the sandbox refused the commands it ran within the last
    /// `window`. `None` where the platform does not report refusals to an
    /// unprivileged process (Linux); the command still sees each refusal as
    /// an error.
    pub fn denials(&self, window: Duration) -> Result<Option<Vec<Denial>>> {
        self.platform_denials(window)
    }

    /// [`Sandbox::new`] plus the defaults: on Linux `/tmp` writable (a
    /// `/tmp` of the task's own; see [`Sandbox::platform_command`]), package
    /// caches of the task's own, credentials in the home directory hidden.
    /// Shells and tools write to `/tmp` by hard-coded path, whatever
    /// `TMPDIR` says, and nothing there belongs to the workspace. On macOS
    /// `/tmp` would be the system's, shared with the person's programs and
    /// the other tasks, so it stays closed: `TMPDIR` and `TMPPREFIX` point
    /// at the task's own.
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
        if limits.servers {
            sandbox.allow_servers();
        }
        // No one file outgrows the disk limit between two measures of it.
        if let Some(mb) = limits.disk_mb {
            sandbox.limit_file_size(mb.saturating_mul(1024 * 1024));
        }
        let tmp = Path::new("/tmp");
        if cfg!(target_os = "linux") && tmp.is_dir() {
            sandbox.allow_write(tmp)?;
        }
        if let Some(sources) = sources {
            for secret in HIDDEN_IN_HOME {
                sandbox.hide(&sources.home.join(secret))?;
            }
            // Kule's home where it is by default, when this task's is another
            // one (`--home`, KULE_HOME): its tasks, logs and keys are no more
            // this task's business than the ones beside it. The task's own
            // home is hidden already, with its copy and tmp let through.
            let default_home = real_or_planned(&sources.home.join(".kule"))?;
            let own_home = real_or_planned(&task.home())?;
            if !own_home.starts_with(&default_home) && !default_home.starts_with(&own_home) {
                sandbox.hide(&default_home)?;
            }
            sandbox.hide_personal(&sources.home, &tools_from_env())?;
            // The projects beside the workspace the person opened for its
            // tasks; checked again here, so one that changed stays closed.
            // A list that cannot be read stops the command.
            // Each gets a link next to the copy where it lies next to the
            // workspace, so `../shared` leads there from the copy too.
            let workspace = sandbox.workspace.clone();
            let mut opened = Vec::new();
            for path in crate::beside::besides_of(&task.home(), &workspace)? {
                if sandbox.open_beside(&path) {
                    opened.push(path);
                }
            }
            sandbox.beside_links = crate::beside::place_links(task, &workspace, &opened)?;
            sandbox.use_task_caches(&sources)?;
            task.note_caches_made(&sandbox.tmp.join("caches"))?;
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
    /// devices stay out of reach, as every device not given is.
    pub fn deny_gpu(&mut self) {
        self.gpu = false;
    }

    /// Commands may listen for connections while their network goes
    /// through the proxy. On macOS that is weaker isolation, which the
    /// person accepts (see [`crate::isolation_gaps`]): Seatbelt cannot hold
    /// a server to this computer, so Kule keeps it off the network from
    /// outside (see `shield`). Elsewhere every server is held to loopback
    /// whatever this says.
    pub fn allow_servers(&mut self) {
        self.servers = true;
    }

    /// Gives commands `path`, a device or a folder of them under `/dev`,
    /// beyond those every command gets: to read, write and drive (Linux,
    /// where devices are files; on macOS a command reaches hardware through
    /// the system's services, and this gives nothing). Never one through
    /// which a command could read or drive what is the person's: a disk,
    /// memory, the keyboard and mouse, a console or another terminal.
    pub fn allow_device(&mut self, path: &Path) -> Result<()> {
        use std::os::unix::fs::FileTypeExt;
        let refuse = |why: &str| {
            Err(Error::Invalid(format!(
                "{} cannot be given to a task's commands: {why}",
                path.display()
            )))
        };
        let Some(name) = path
            .strip_prefix("/dev")
            .ok()
            .and_then(|rest| rest.components().next())
            .and_then(|first| first.as_os_str().to_str())
        else {
            return refuse("only a device under /dev can");
        };
        if path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
        {
            return refuse("its path goes back up");
        }
        let console = name
            .strip_prefix("tty")
            .is_some_and(|n| n.is_empty() || n.starts_with(|c: char| c.is_ascii_digit()));
        if NEVER_DEVICES.contains(&name)
            || ["hidraw", "vcs"].iter().any(|p| name.starts_with(p))
            || console
        {
            return refuse("through it a command reaches what is yours");
        }
        let kind = std::fs::symlink_metadata(path).at(path)?.file_type();
        if kind.is_symlink() {
            return refuse("it is a link");
        }
        if kind.is_block_device() {
            return refuse("it is a disk");
        }
        if kind.is_dir() {
            let holds_disk = std::fs::read_dir(path).at(path)?.flatten().any(|e| {
                e.file_type()
                    .is_ok_and(|t| t.is_block_device() || t.is_symlink())
            });
            if holds_disk {
                return refuse("it holds disks or links");
            }
        } else if !kind.is_char_device() {
            return refuse("it is not a device");
        }
        self.devices.push(path.to_owned());
        Ok(())
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

    /// Hides the person's own files under `home` ([`PERSONAL_IN_HOME`]) and
    /// the folder the workspace lies in, with the other projects beside
    /// it. That folder stays readable where it is `home` itself, outside
    /// `home`, or among the settings in it (a name starting with `.`, or
    /// `Library`), where toolchains live. Inside a hidden folder, the
    /// workspace's git directory (a linked worktree's repository beside
    /// it) and the folders `tools` lead to (`PATH`, an SDK's variable)
    /// stay readable: a tool's folder whole, the git directory alone. Both
    /// are taken from the person's side, never from the copy.
    pub fn hide_personal(&mut self, home: &Path, tools: &[PathBuf]) -> Result<()> {
        let home = real_or_planned(home)?;
        let folders = personal_folders(&self.workspace, &home)?;
        // Not the home directory or above it: that would leave everything.
        let tools: Vec<PathBuf> = tools
            .iter()
            .filter(|t| t.is_absolute())
            .filter_map(|t| std::fs::canonicalize(t).ok())
            .filter(|t| !home.starts_with(t))
            .collect();
        let git = git_dirs(&self.workspace);
        for folder in folders {
            // A folder a tool is in, or on the way to one, is left alone.
            if tools.iter().any(|t| folder.starts_with(t)) {
                continue;
            }
            for tool in &tools {
                if let Ok(within) = tool.strip_prefix(&folder)
                    && let Some(first) = within.components().next()
                {
                    self.see_through.push(folder.join(first));
                }
            }
            for dir in git.iter().filter(|d| d.starts_with(&folder)) {
                self.see_through.push(dir.clone());
            }
            self.personal.push(folder.clone());
            self.hidden.push(folder);
        }
        self.see_through.sort();
        self.see_through.dedup();
        Ok(())
    }

    /// Takes `path` off the hidden list, for a command that needs it (an
    /// agent reading its own login from the keychain). Hidden paths inside
    /// it stay hidden. `path` is taken as it is written, not where a link
    /// there leads: a link an agent put in place of its login reveals
    /// nothing. Kule's home and the task's folder are never revealed.
    ///
    /// Revealing the keychain's files (`~/Library/Keychains`) opens the
    /// keychain's service on macOS too, which reads items for its caller:
    /// without them it opens none, unlocked keychains included.
    ///
    /// A path inside a folder hidden as the person's own (a project beside
    /// the workspace that it builds from) becomes readable, not writable;
    /// what is hidden inside it stays hidden.
    pub fn reveal(&mut self, path: &Path) -> Result<()> {
        let here = here(path)?;
        if std::fs::symlink_metadata(&here).is_ok_and(|m| m.file_type().is_symlink()) {
            return Ok(());
        }
        if self.open_beside(&here) {
            return Ok(());
        }
        let mut i = 0;
        let mut revealed = false;
        self.hidden.retain(|hidden| {
            i += 1;
            let keep = i <= ALWAYS_HIDDEN || hidden != &here;
            revealed |= !keep;
            keep
        });
        if revealed && here.ends_with("Library/Keychains") {
            self.keychain = true;
        }
        Ok(())
    }

    /// Lets commands read `path`, a folder inside one hidden as the person's
    /// own (a project beside the workspace that it builds from), never
    /// write it; what is hidden inside it stays hidden. Only a path as it
    /// is, resolved and not a link: whether it was opened.
    pub fn open_beside(&mut self, path: &Path) -> bool {
        let as_is = std::fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_symlink())
            && std::fs::canonicalize(path).is_ok_and(|real| real == path);
        let inside = self
            .personal
            .iter()
            .any(|folder| path.starts_with(folder) && path != folder);
        if as_is && inside {
            self.see_through.push(path.to_owned());
        }
        as_is && inside
    }

    /// Lets commands write in `path` as it is written: a link there is not
    /// followed, so a link an agent put in place of its state directory
    /// opens nothing more (the link itself can be replaced, no more).
    pub fn allow_write_here(&mut self, path: &Path) -> Result<()> {
        self.writable.push(here(path)?);
        Ok(())
    }

    pub fn set_network(&mut self, network: Network) {
        self.network = network;
    }

    /// Points the commands' `HOME` at `task`'s home layer over `real_home`
    /// (and `XDG_CONFIG_HOME`, where it is the real home's `.config`), and
    /// lets them write there. The real files whose secrets the layer holds
    /// masked are hidden: commands read the masked clones.
    pub fn use_home(&mut self, task: &Task, real_home: &Path) -> Result<()> {
        for masked in task.home_masked()? {
            self.hide(&masked)?;
        }
        let layer = real(&task.home_layer())?;
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
    /// group does not reach. Looks again until a look finds none, since one
    /// may start another while it is being ended; returns how many it
    /// ended, or an error when they keep coming. A process of the person's
    /// own, outside the sandbox, is never one of them.
    pub fn end_strays(&self) -> Result<usize> {
        self.end_found(|| self.strays())
    }

    /// Ends what `find` finds until a look finds none (see
    /// [`Sandbox::end_strays`]).
    fn end_found(&self, find: impl Fn() -> Vec<i32>) -> Result<usize> {
        let mut ended = std::collections::HashSet::new();
        for _ in 0..STRAY_LOOKS {
            // One already sent SIGKILL may still be there, on its way out.
            let found: Vec<i32> = find().into_iter().filter(|p| !ended.contains(p)).collect();
            if found.is_empty() {
                return Ok(ended.len());
            }
            crate::resources::kill(&found);
            ended.extend(found);
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(Error::Invalid(format!(
            "processes in task {}'s sandbox kept starting others while they were ended",
            self.task
        )))
    }

    /// Ends what still runs in any of `task`'s sandboxes, its home layer's
    /// included, before Kule reads what they wrote: an error when that
    /// cannot be done, and nothing is read beside them.
    pub fn end_all(task: &Task) -> Result<usize> {
        Sandbox::finder(task)?.end_strays()
    }

    /// A sandbox of `task`'s, its home layer's included, only to find the
    /// processes the task's sandboxes hold by: no command starts from it.
    pub fn finder(task: &Task) -> Result<Sandbox> {
        let mut sandbox = Sandbox::new(task)?;
        sandbox.layer = Some(task.home_layer()).filter(|l| l.is_dir());
        Ok(sandbox)
    }

    /// What runs in this sandbox, or on macOS in any of the task's, now:
    /// however it was started and whatever became of its parent (see
    /// [`Sandbox::end_strays`]).
    pub fn processes(&self) -> Vec<i32> {
        self.strays()
    }

    /// Ends what still runs in this sandbox alone, not in the task's
    /// others: what a terminal's shell left behind once it ended, while
    /// the task's other terminals go on. On macOS such a process could
    /// otherwise reach the terminal device the shell had, once the system
    /// gives it to another of the person's terminals. Elsewhere a command
    /// reaches none of the person's terminals, and what it leaves is ended
    /// with its PID namespace, or with the task (see
    /// [`Sandbox::end_strays`]).
    pub fn end_own(&self) -> Result<usize> {
        #[cfg(target_os = "macos")]
        {
            let mark = format!("{}.{}", task_mark(&self.task), self.own);
            self.end_found(|| self.marked(&mark))
        }
        #[cfg(not(target_os = "macos"))]
        Ok(0)
    }

    /// macOS knows which processes a task's sandboxes hold: those whose
    /// sandbox lets them look up the task's mark, which only its own do,
    /// and refuses them a name beside it that only Kule's profile refuses.
    /// A sandbox that allows everything (another tool's) allows the mark
    /// too, but not that refusal.
    #[cfg(target_os = "macos")]
    fn strays(&self) -> Vec<i32> {
        self.marked(&task_mark(&self.task))
    }

    /// Every process `task`'s sandboxes hold now, however it was started
    /// and whatever became of its parent (macOS; see [`Sandbox::strays`]).
    #[cfg(target_os = "macos")]
    pub fn processes_of(task: &str) -> Vec<i32> {
        Sandbox::processes_of_tasks(&[task.to_owned()])
            .into_iter()
            .map(|(pid, _)| pid)
            .collect()
    }

    /// [`Sandbox::processes_of`] for each of `tasks` in one look at the
    /// processes: each with the task it belongs to.
    #[cfg(target_os = "macos")]
    pub fn processes_of_tasks(tasks: &[String]) -> Vec<(i32, String)> {
        let marks: Vec<(String, String)> =
            tasks.iter().map(|t| (task_mark(t), t.clone())).collect();
        marked_in(&marks)
    }

    #[cfg(target_os = "macos")]
    fn marked(&self, mark: &str) -> Vec<i32> {
        marked_in(&[(mark.to_owned(), self.task.clone())])
            .into_iter()
            .map(|(pid, _)| pid)
            .collect()
    }
}

/// The processes whose sandbox lets them look up one of `marks`, a name
/// beside its task's (the second of each pair) that only Kule's profiles
/// allow (see [`Sandbox::strays`]); each with that task.
#[cfg(target_os = "macos")]
fn marked_in(marks: &[(String, String)]) -> Vec<(i32, String)> {
    let marks = marks_of(marks);
    let me = std::process::id() as i32;
    crate::ports::macos::all_pids()
        .into_iter()
        .filter(|&pid| pid > 1 && pid != me)
        .filter_map(|pid| classify(pid, &marks).map(|task| (pid, task)))
        .collect()
}

/// Each mark with the name beside its task's that only Kule's profiles
/// refuse, and the task.
#[cfg(target_os = "macos")]
type Marks = Vec<(std::ffi::CString, std::ffi::CString, String)>;

#[cfg(target_os = "macos")]
fn marks_of(marks: &[(String, String)]) -> Marks {
    marks
        .iter()
        .filter_map(|(mark, task)| {
            Some((
                std::ffi::CString::new(mark.as_str()).ok()?,
                std::ffi::CString::new(format!("{}.decoy", task_mark(task))).ok()?,
                task.clone(),
            ))
        })
        .collect()
}

/// The task of `marks` whose mark `pid`'s sandbox lets it look up, if any.
#[cfg(target_os = "macos")]
fn classify(pid: i32, marks: &Marks) -> Option<String> {
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
    let lookup = |name: &std::ffi::CStr| {
        // SAFETY: sandbox_check only reads its arguments.
        unsafe {
            sandbox_check(
                pid,
                c"mach-lookup".as_ptr(),
                FILTER_GLOBAL_NAME | NO_REPORT,
                name.as_ptr(),
            )
        }
    };
    // SAFETY: as above.
    let sandboxed = unsafe { sandbox_check(pid, std::ptr::null(), FILTER_NONE | NO_REPORT) == 1 };
    if !sandboxed {
        return None;
    }
    marks
        .iter()
        .find(|(mark, decoy, _)| lookup(mark) == 0 && lookup(decoy) == 1)
        .map(|(_, _, task)| task.clone())
}

/// [`Sandbox::processes_of_tasks`], asked again and again (by the shield,
/// 50 times a second) for little more than listing the processes: what it
/// found of each process is kept, by its pid and when it started, so a
/// later process given the same pid is looked at anew. That a process is
/// a task's holds for its life, as no process leaves its sandbox; that it
/// is none of theirs is kept only once it is a second old, as a command
/// takes its sandbox in its first moments.
#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct ProcessFinder {
    tasks: Vec<String>,
    seen: std::collections::HashMap<(i32, u64), Option<String>>,
}

#[cfg(target_os = "macos")]
impl ProcessFinder {
    pub fn new() -> ProcessFinder {
        ProcessFinder::default()
    }

    /// The processes of `tasks`' sandboxes now, each with its task.
    pub fn find(&mut self, tasks: &[String]) -> Vec<(i32, String)> {
        use crate::ports::macos;
        if tasks != self.tasks {
            self.tasks = tasks.to_vec();
            self.seen.clear();
        }
        let marks = marks_of(
            &self
                .tasks
                .iter()
                .map(|t| (task_mark(t), t.clone()))
                .collect::<Vec<_>>(),
        );
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_micros() as u64);
        let me = std::process::id() as i32;
        let mut seen = std::collections::HashMap::new();
        let mut found = Vec::new();
        for pid in macos::all_pids() {
            if pid <= 1 || pid == me {
                continue;
            }
            let Some(started) = macos::started(pid) else {
                continue;
            };
            let key = (pid, started);
            let task = match self.seen.remove(&key) {
                Some(known) => known,
                None => classify(pid, &marks),
            };
            if let Some(task) = &task {
                found.push((pid, task.clone()));
            }
            if task.is_some() || now.saturating_sub(started) > 1_000_000 {
                seen.insert(key, task);
            }
        }
        self.seen = seen;
        found
    }
}

impl Sandbox {
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

    /// A command for `program` whose first step between fork and exec gives
    /// it a session of its own: no controlling terminal, so nothing it runs
    /// can push keystrokes into the person's terminal (TIOCSTI), and its
    /// whole process group can be stopped when it is done. The steps a
    /// platform adds come after, inside that session and group.
    #[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
    fn session_command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        use std::os::unix::process::CommandExt;

        let mut command = Command::new(program);
        let file_size = self.file_size;
        // SAFETY: setsid and setrlimit are async-signal-safe and touch
        // nothing but the child between fork and exec.
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
        command
    }

    /// A command that runs `program` inside the sandbox, in the copy, with
    /// `TMPDIR` pointing at the task's temporary directory.
    pub fn command(&self, program: &str, args: &[String]) -> Result<Command> {
        // Less isolated than meant only with the person's say.
        let gaps = crate::isolation::isolation_gaps(self.network, self.servers);
        if !gaps.is_empty() && !self.accept_weaker {
            return Err(Error::Invalid(format!(
                "this computer cannot give the command {}. Kule does not run it less isolated \
                 without your say: accept weaker isolation in Settings → Policy, or with \
                 `kule isolation --accept-weaker on`",
                gaps.join("; nor ")
            )));
        }
        let mut command = self.platform_command(program, args)?;
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
        // zsh's here-documents and `=(…)` go to TMPPREFIX, and Claude
        // Code's own files to CLAUDE_CODE_TMPDIR: /tmp by default, whatever
        // TMPDIR says.
        command
            .current_dir(&self.copy)
            .env("TMPDIR", &self.tmp)
            .env("TMPPREFIX", self.tmp.join("zsh"))
            .env("CLAUDE_CODE_TMPDIR", &self.tmp);
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
            // Go on macOS checks certificates with the system's services,
            // which a command reaches only with the network open; told to,
            // it checks them against the roots `SSL_CERT_FILE` names, as on
            // Linux. Go before 1.27 does that unless its module says a newer
            // Go; older Go ignores the setting. The last setting of a name
            // wins, so this one goes after the person's.
            let godebug = match current("GODEBUG") {
                Some(theirs) if !theirs.is_empty() => {
                    let mut value = theirs;
                    value.push(",x509sslcertoverrideplatform=1");
                    value
                }
                _ => "x509sslcertoverrideplatform=1".into(),
            };
            command.env("GODEBUG", godebug);
        }
        Ok(command)
    }

    /// [`Sandbox::command`] on `terminal`, a pseudo-terminal Kule made for
    /// the person (its follower end): the command reads and writes it, and
    /// it is the controlling terminal of the command's own session, so a
    /// shell there has job control and Ctrl-C reaches what it runs. It is
    /// no terminal of the person's other sessions, and the command still
    /// reaches none of those.
    pub fn terminal_command(
        &self,
        program: &str,
        args: &[String],
        terminal: std::os::fd::BorrowedFd<'_>,
    ) -> Result<Command> {
        use std::os::unix::process::CommandExt;

        let mut on_terminal = self.clone();
        on_terminal.terminal = Some(terminal_path(terminal)?);
        let mut command = on_terminal.command(program, args)?;
        let io = |what: &str| {
            terminal
                .try_clone_to_owned()
                .map(std::process::Stdio::from)
                .map_err(|e| Error::Invalid(format!("the terminal's {what}: {e}")))
        };
        command
            .stdin(io("input")?)
            .stdout(io("output")?)
            .stderr(io("errors")?);
        // Last, in the process that goes on to the command: under a PID
        // namespace of its own that is two forks below the session Kule's
        // child made, so it makes its own.
        // SAFETY: getsid, getpid, setsid and ioctl are async-signal-safe
        // and touch nothing but the child between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::getsid(0) != libc::getpid() && libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(command)
    }
}

/// The device of `terminal`, a pseudo-terminal's follower end: on macOS
/// the one name the profile opens to the command beside its own, checked
/// to be that very device.
#[cfg(target_os = "macos")]
fn terminal_path(terminal: std::os::fd::BorrowedFd<'_>) -> Result<PathBuf> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    let mut name = [0 as libc::c_char; 128];
    // SAFETY: ttyname_r writes at most `name.len()` bytes into `name`.
    let named = unsafe { libc::ttyname_r(terminal.as_raw_fd(), name.as_mut_ptr(), name.len()) };
    if named != 0 {
        return Err(Error::Invalid(
            "a command's terminal must be a terminal".into(),
        ));
    }
    // SAFETY: ttyname_r ended the name with a NUL inside the buffer.
    let name = unsafe { std::ffi::CStr::from_ptr(name.as_ptr()) };
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(name.to_bytes()));
    let pty = path
        .to_str()
        .and_then(|p| p.strip_prefix("/dev/ttys"))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    let held = std::fs::File::from(terminal.try_clone_to_owned().at(&path)?)
        .metadata()
        .at(&path)?;
    let named = std::fs::metadata(&path).at(&path)?;
    if !pty || held.rdev() != named.rdev() {
        return Err(Error::Invalid(format!(
            "{} is not a pseudo-terminal Kule can give a command",
            path.display()
        )));
    }
    Ok(path)
}

/// Landlock judges a descriptor by how it was opened, before the command
/// was restricted: the terminal needs no rule of its own.
#[cfg(not(target_os = "macos"))]
fn terminal_path(_terminal: std::os::fd::BorrowedFd<'_>) -> Result<PathBuf> {
    Ok(PathBuf::new())
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

/// Whether a command's processes are its own: it sees and signals no other,
/// and what it leaves running ends with it. On Linux that takes a PID
/// namespace, refused where user namespaces are; elsewhere what it leaves
/// is found by its sandbox and ended with the task (see
/// [`Sandbox::end_strays`]).
pub fn own_processes() -> bool {
    #[cfg(target_os = "linux")]
    return crate::namespace::pids_available();
    #[cfg(not(target_os = "linux"))]
    return false;
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
    // What the agent should know of them at its next prompt: Kule's own
    // words only, never a name or path a command chose.
    let mut hints: Vec<String> = refusal_hints(task)?;
    for hint in denials.iter().filter_map(|d| d.hint.clone()) {
        if !hints.contains(&hint) {
            hints.push(hint);
        }
    }
    if !hints.is_empty() {
        crate::fsops::write_json(&task.dir().join(HINTS_FILE), &hints)?;
    }
    Ok(())
}

fn refusal_hints(task: &Task) -> Result<Vec<String>> {
    let path = task.dir().join(HINTS_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    crate::fsops::read_json(&path)
}

impl Task {
    /// What the agent should know at the start of its next run of what the
    /// sandbox refused its commands and why, once: `None` when no refusal
    /// had a hint.
    pub fn take_refusal_note(&self) -> Result<Option<String>> {
        let hints = refusal_hints(self)?;
        let path = self.dir().join(HINTS_FILE);
        if path.is_file() {
            std::fs::remove_file(&path).at(&path)?;
        }
        if hints.is_empty() {
            return Ok(None);
        }
        let mut note =
            "Kule's sandbox refused some of what your earlier commands in this task tried:"
                .to_owned();
        for hint in hints {
            note.push_str("\n- ");
            note.push_str(&hint);
        }
        Ok(Some(note))
    }

    /// Everything the agent should know at the start of its next run, once:
    /// what came in from the folder (see [`Task::take_sync_note`]) and what
    /// the sandbox refused.
    pub fn take_agent_notes(&self) -> Result<Option<String>> {
        let notes: Vec<String> = [self.take_sync_note()?, self.take_refusal_note()?]
            .into_iter()
            .flatten()
            .collect();
        Ok((!notes.is_empty()).then(|| notes.join("\n\n")))
    }
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

/// `path` with its parent resolved and its own name kept: where the entry
/// itself is, whatever a link there points to.
fn here(path: &Path) -> Result<PathBuf> {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => Ok(real_or_planned(parent)?.join(name)),
        _ => real_or_planned(path),
    }
}

/// The first hidden paths, which [`Sandbox::reveal`] never takes away:
/// Kule's home and the task's folder (see [`Sandbox::new`]).
const ALWAYS_HIDDEN: usize = 2;

/// Devices under `/dev` never given to a task's commands (see
/// [`Sandbox::allow_device`]): memory and the kernel's log, keyboards and
/// mice (to read or to type), the console, terminals and Kule's own
/// handling of them, shared memory.
const NEVER_DEVICES: &[&str] = &[
    "mem", "kmem", "port", "kmsg", "input", "uinput", "console", "pts", "ptmx", "shm", "mqueue",
    "snapshot", "disk", "mapper", "block", "char", "fd", "stdin", "stdout", "stderr", "core",
    "cpu", "mcelog", "watchdog",
];

/// How many times [`Sandbox::end_strays`] looks before it gives up.
const STRAY_LOOKS: usize = 200;

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
        let mut command = self.session_command("/usr/bin/sandbox-exec");
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
                "(allow mach-lookup (global-name \"{task}\") (global-name \"{task}.{}\"))\n",
                self.own,
                task = task_mark(&self.task)
            )
        } else {
            String::new()
        };
        let keychain = if self.keychain {
            "(allow mach-lookup (global-name \"com.apple.SecurityServer\"))\n"
        } else {
            ""
        };
        let mut text = format!(
            "(version 1)\n\
             (deny default {tag})\n\
             {SEATBELT_BASE}\
             {mark}\
             {keychain}\
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
        if let Some(terminal) = &self.terminal {
            text.push_str("(allow file-read* file-write* file-ioctl (literal (param \"PTY\")))\n");
            params.push(("PTY".to_owned(), terminal.as_path()));
        }
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
            // Names resolve through mDNSResponder's socket, which the rule
            // for the person's daemons above would take away with the rest.
            Network::Allow => text.push_str(
                "(allow network-outbound (remote unix-socket (literal \"/private/var/run/mDNSResponder\")))\n",
            ),
            Network::Deny => {
                text.push_str(&format!(
                    "(deny network-outbound (remote ip) {tag})\n(deny network-bind (local ip) {tag})\n"
                ));
            }
            Network::Proxy(port) => {
                text.push_str(&format!(
                    "(deny network-outbound (remote ip) {tag})\n(allow network-outbound (remote ip \"localhost:{port}\"))\n"
                ));
                // Seatbelt sees the port a program listens on, never the
                // address: `localhost:*` lets every address through. Only
                // where the person allows servers do they listen, kept off
                // the network from outside (see `shield`).
                if !self.servers {
                    text.push_str(&format!("(deny network-bind (local ip) {tag})\n"));
                }
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
            // resolves names itself; so would the certificate services,
            // which fetch the addresses a certificate names (its issuer,
            // OCSP, revocation lists) outside the sandbox.
            for service in [
                "com.apple.dnssd.service",
                "com.apple.trustd.agent",
                "com.apple.ocspd",
            ] {
                text.push_str(&format!(
                    "(deny mach-lookup (global-name \"{service}\") {tag})\n"
                ));
            }
        }
        for (i, path) in self.hidden.iter().enumerate() {
            text.push_str(&format!(
                "(deny file-read* file-write* (subpath (param \"H{i}\")) {tag})\n"
            ));
            params.push((format!("H{i}"), path.as_path()));
        }
        // Tools and the git directory in a hidden folder of the person's,
        // to read; what is hidden inside them, or as them, is taken away
        // again after.
        for (i, path) in self.see_through.iter().enumerate() {
            text.push_str(&format!("(allow file-read* (subpath (param \"S{i}\")))\n"));
            params.push((format!("S{i}"), path.as_path()));
        }
        for (i, path) in self.hidden.iter().enumerate() {
            if self.see_through.iter().any(|s| path.starts_with(s)) {
                text.push_str(&format!(
                    "(deny file-read* file-write* (subpath (param \"H{i}\")) {tag})\n"
                ));
            }
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
        let writable = self.writable();
        let mut between = 0;
        for (i, path) in self.protected.iter().enumerate() {
            text.push_str(&format!(
                "(deny file-write* (subpath (param \"P{i}\")) {tag})\n"
            ));
            params.push((format!("P{i}"), path.as_path()));
            // A rule holds a path by its name: a directory on the way to it
            // inside a writable root, renamed, would take it out from under
            // the rule, to be changed and renamed back.
            for dir in path.ancestors().skip(1) {
                if !writable
                    .iter()
                    .any(|root| dir.starts_with(root) && dir != *root)
                {
                    break;
                }
                text.push_str(&format!(
                    "(deny file-write-unlink file-write-create (literal (param \"Q{between}\")) {tag})\n"
                ));
                params.push((format!("Q{between}"), dir));
                between += 1;
            }
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
            .chain(self.see_through.iter().flat_map(|s| s.ancestors().skip(1)))
            .chain(self.beside_links.iter().map(PathBuf::as_path))
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
        hint: hint(operation, path),
    })
}

#[cfg(target_os = "linux")]
impl Sandbox {
    fn platform_command(&self, program: &str, args: &[String]) -> Result<Command> {
        use std::os::unix::process::CommandExt;

        let mut ruleset = Some(self.landlock_ruleset()?);
        let mut command = self.session_command(program);
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
        // Without Landlock's rule for Unix sockets, those outside the
        // task's folders are covered for the command instead: the person's
        // runtime directory whole, where their daemons make theirs, also
        // after the command started; elsewhere each one listed now.
        let (covered, masked) = if landlock_abi() < 9 && crate::namespace::mounts_available() {
            let covered = self.runtime_dirs(&runtime_dir_candidates());
            let masked = self
                .outside_sockets()?
                .into_iter()
                .filter(|s| !covered.iter().any(|dir| s.starts_with(dir)))
                .collect();
            (covered, masked)
        } else {
            (Vec::new(), Vec::new())
        };
        // Where /tmp is the commands' to write, it is the task's own; not
        // when the task itself lives in /tmp (a Kule home there), which the
        // mount would hide.
        let in_tmp = [&self.copy, &self.tmp]
            .into_iter()
            .chain(self.layer.iter())
            .any(|p| p.starts_with("/tmp"));
        let own_tmp = self.writable.iter().any(|w| w == Path::new("/tmp"))
            && !in_tmp
            && crate::namespace::mounts_available();
        let own_pids = crate::namespace::pids_available();
        if crate::namespace::ipc_available()
            || matches!(net, Net::Own(_))
            || !read_only.is_empty()
            || !masked.is_empty()
            || !covered.is_empty()
            || own_tmp
            || own_pids
            || own_ptys()
        {
            let mut isolation = Isolation::new(net, &read_only)
                .and_then(|i| i.mask_sockets(&masked))
                .and_then(|i| i.cover(&covered))
                .at(Path::new("/proc/self/ns"))?;
            if own_pids {
                isolation = isolation.own_pids();
            }
            if own_ptys() {
                isolation = isolation.own_ptys();
            }
            if own_tmp {
                isolation = isolation.own_tmp(&self.tmp).at(&self.tmp)?;
            }
            // SAFETY: enter only makes system calls on buffers and
            // descriptors prepared before the fork.
            unsafe {
                command.pre_exec(move || isolation.enter());
            }
        }
        // The command's own terminals and its own /proc are mounted after
        // the ruleset was made, and Landlock passes over what a mount
        // covers: their rules go on what the mounts put there, in the child,
        // on a second handle to the same ruleset.
        let mut late: Vec<(&'static std::ffi::CStr, u64)> = Vec::new();
        if own_ptys() {
            // Terminals are driven by their requests (sizes, modes).
            let file =
                landlock::AccessFs::ReadFile | landlock::AccessFs::WriteFile | device_ioctl();
            late.push((c"/dev/pts", (file | landlock::AccessFs::ReadDir).bits()));
            late.push((c"/dev/ptmx", file.bits()));
        }
        if own_pids {
            // Only the command's own processes are there; read as /proc
            // always was.
            let read = landlock::AccessFs::ReadFile | landlock::AccessFs::ReadDir;
            late.push((c"/proc", read.bits()));
        }
        let late_handle = if late.is_empty() {
            None
        } else {
            let handle: Option<std::os::fd::OwnedFd> = ruleset
                .as_ref()
                .map(|r| r.try_clone())
                .transpose()
                .at(Path::new("/proc/self"))?
                .and_then(Into::into);
            Some(handle.ok_or_else(|| {
                Error::Invalid("Landlock: no ruleset to add mounted paths to".into())
            })?)
        };
        // SAFETY: the closure runs between fork and exec. It only makes
        // system calls: the rules for what the namespaces mounted, on paths
        // and masks prepared before the fork, then the two that restrict the
        // process (no_new_privs and landlock_restrict_self).
        unsafe {
            command.pre_exec(move || {
                if let Some(handle) = &late_handle {
                    use std::os::fd::AsRawFd;
                    for (path, access) in &late {
                        add_path_rule(handle.as_raw_fd(), path, *access)?;
                    }
                }
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
        // Unix sockets lead to daemons running as the person (Docker, the
        // session bus, ssh-agent, X): from ABI 9 a command reaches only
        // those in its own folders. Older kernels hide the others with a
        // mount instead (`outside_sockets`). From ABI 6, abstract sockets
        // and signals stay within the sandbox too.
        let abi_now = landlock_abi();
        if abi_now >= 9 {
            ruleset = ruleset
                .handle_access(AccessFs::ResolveUnix)
                .map_err(|e| fail(&e))?;
        }
        if abi_now >= 6 {
            ruleset = ruleset
                .scope(landlock::Scope::AbstractUnixSocket | landlock::Scope::Signal)
                .map_err(|e| fail(&e))?;
        }
        // From ABI 5 a device opened in the sandbox takes only the requests
        // (ioctl) of a device given to it.
        if !device_ioctl().is_empty() {
            ruleset = ruleset
                .handle_access(device_ioctl())
                .map_err(|e| fail(&e))?;
        }
        let carve_all = |roots: Vec<PathBuf>| {
            self.hidden
                .iter()
                .fold(roots, |roots, hidden| carve(roots, hidden))
        };
        let mut readable = carve_all(vec![PathBuf::from("/")]);
        // Tools and the git directory in a hidden folder of the person's
        // come back to read, but for what is hidden inside them, or as them.
        for see in &self.see_through {
            readable.extend(
                self.hidden
                    .iter()
                    .filter(|h| h.starts_with(see))
                    .fold(vec![see.clone()], |roots, h| carve(roots, h)),
            );
        }
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
        // A root that is a link gets no rule: Landlock would open it and
        // allow what it points to (see `allow_write_here`).
        let mut writable = carve_all(
            self.writable
                .iter()
                .filter(|root| !self.protected.iter().any(|p| root.starts_with(p)))
                .filter(|root| {
                    std::fs::symlink_metadata(root).is_ok_and(|m| !m.file_type().is_symlink())
                })
                .cloned()
                .collect(),
        );
        writable.extend([self.copy.clone(), self.tmp.clone()]);
        writable.extend(self.layer.clone());
        // Not `/dev/pts`: the person's other terminals are there. A command
        // keeps the terminal it was given (already open, and `/dev/tty`).
        // Where it has an instance of its own, the rules for it are added
        // once it is mounted (see `pty_rules`); elsewhere it cannot make a
        // new one, whose other end opens by its path there.
        // Devices only by name, never `/dev` whole: a camera, a microphone
        // or a USB device the person's session may open, and other
        // programs' shared memory, are there.
        let mut devices: Vec<PathBuf> = [
            "/dev/null",
            "/dev/zero",
            "/dev/full",
            "/dev/tty",
            "/dev/ptmx",
        ]
        .map(PathBuf::from)
        .to_vec();
        if self.gpu {
            devices.extend(gpu_devices());
        }
        devices.extend(self.devices.iter().cloned());
        let read_only_devices = ["/dev/random", "/dev/urandom"].map(PathBuf::from);
        let readable = carve(readable, Path::new("/dev"));
        let mut created = ruleset
            .create()
            .map_err(|e| fail(&e))?
            .set_compatibility(CompatLevel::BestEffort)
            .add_rules(path_beneath_rules(&readable, AccessFs::from_read(abi)))
            .map_err(|e| fail(&e))?
            .add_rules(path_beneath_rules(
                read_only_devices,
                AccessFs::from_read(abi),
            ))
            .map_err(|e| fail(&e))?
            .add_rules(path_beneath_rules(
                devices,
                AccessFs::from_all(abi) | device_ioctl(),
            ))
            .map_err(|e| fail(&e))?
            .add_rules(path_beneath_rules(&writable, AccessFs::from_all(abi)))
            .map_err(|e| fail(&e))?;
        if let Network::Proxy(port) = self.network {
            created = created
                .add_rule(landlock::NetPort::new(port, AccessNet::ConnectTcp))
                .map_err(|e| fail(&e))?;
        }
        if abi_now >= 9 {
            let own = [self.copy.clone(), self.tmp.clone()]
                .into_iter()
                .chain(self.layer.clone());
            created = created
                .add_rules(path_beneath_rules(own, AccessFs::ResolveUnix))
                .map_err(|e| fail(&e))?;
        }
        Ok(created)
    }
}

#[cfg(target_os = "linux")]
impl Sandbox {
    /// The Unix sockets listening now outside the task's own folders, as
    /// the kernel lists them; an error when the list cannot be read, as
    /// nothing would be kept out then.
    /// Of `candidates`, the person's runtime directories to cover for the
    /// command: each a directory of theirs (by its owner, not the variable
    /// that names it), never `/`, holding none of the task's folders or
    /// any the command may write to, which the cover would hide.
    fn runtime_dirs(&self, candidates: &[PathBuf]) -> Vec<PathBuf> {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: getuid cannot fail.
        let uid = unsafe { libc::getuid() };
        let ours: Vec<&Path> = [&self.copy, &self.tmp]
            .into_iter()
            .chain(self.layer.iter())
            .chain(&self.writable)
            .map(PathBuf::as_path)
            .collect();
        let mut dirs: Vec<PathBuf> = candidates
            .iter()
            .filter_map(|c| std::fs::canonicalize(c).ok())
            .filter(|d| d.parent().is_some())
            .filter(|d| std::fs::metadata(d).is_ok_and(|m| m.is_dir() && m.uid() == uid))
            .filter(|d| !ours.iter().any(|o| o.starts_with(d)))
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    fn outside_sockets(&self) -> Result<Vec<PathBuf>> {
        self.outside_sockets_in(Path::new("/proc/net/unix"))
    }

    /// [`Sandbox::outside_sockets`], from the kernel's list at `list`.
    fn outside_sockets_in(&self, list: &Path) -> Result<Vec<PathBuf>> {
        use std::os::unix::fs::FileTypeExt;
        let own: Vec<&Path> = [&self.copy, &self.tmp]
            .into_iter()
            .chain(self.layer.iter())
            .map(PathBuf::as_path)
            .collect();
        let listed = std::fs::read_to_string(list).at(list)?;
        let mut sockets: Vec<PathBuf> = socket_paths(&listed)
            .into_iter()
            .filter(|path| !own.iter().any(|dir| path.starts_with(dir)))
            .filter(|path| std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_socket()))
            .collect();
        sockets.sort();
        sockets.dedup();
        Ok(sockets)
    }
}

/// Landlock's right to drive a device (ioctl), where this kernel's Landlock
/// knows it (ABI 5): handled by every sandbox there, and given with each
/// device a command gets.
#[cfg(target_os = "linux")]
fn device_ioctl() -> landlock::BitFlags<landlock::AccessFs> {
    if landlock_abi() >= 5 {
        landlock::AccessFs::IoctlDev.into()
    } else {
        landlock::BitFlags::empty()
    }
}

/// Allows `access` beneath `path` in the Landlock ruleset `ruleset`, as
/// the path is now (`landlock_add_rule`).
///
/// # Safety
/// Only system calls, for between fork and exec.
#[cfg(target_os = "linux")]
unsafe fn add_path_rule(
    ruleset: std::os::fd::RawFd,
    path: &std::ffi::CStr,
    access: u64,
) -> std::io::Result<()> {
    #[repr(C, packed)]
    struct PathBeneath {
        allowed_access: u64,
        parent_fd: i32,
    }
    const RULE_PATH_BENEATH: libc::c_int = 1;
    // SAFETY: the path is NUL-terminated; the rule is plain data, passed
    // by pointer for the call's length.
    unsafe {
        let fd = libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC);
        if fd == -1 {
            return Err(std::io::Error::last_os_error());
        }
        let rule = PathBeneath {
            allowed_access: access,
            parent_fd: fd,
        };
        let added = libc::syscall(
            libc::SYS_landlock_add_rule,
            ruleset,
            RULE_PATH_BENEATH,
            &raw const rule,
            0,
        );
        let error = std::io::Error::last_os_error();
        libc::close(fd);
        if added == -1 {
            return Err(error);
        }
    }
    Ok(())
}

/// Whether commands get terminals of their own: a devpts instance in their
/// mount namespace. The Landlock rule that opens `/dev/pts` to a command
/// and the mount that makes it the command's own go together, so both ask
/// this one question.
#[cfg(target_os = "linux")]
fn own_ptys() -> bool {
    crate::namespace::ptys_available()
}

/// Where the person's daemons make their sockets: systemd's runtime
/// directory for this user, and the one `XDG_RUNTIME_DIR` names.
#[cfg(target_os = "linux")]
fn runtime_dir_candidates() -> Vec<PathBuf> {
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    std::iter::once(PathBuf::from(format!("/run/user/{uid}")))
        .chain(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
        .filter(|p| p.is_absolute())
        .collect()
}

/// The paths of the sockets `/proc/net/unix` lists: each the rest of its
/// line from the first ` /`, spaces and all (the fields before it are
/// numbers; abstract names start with `@` and are left out).
#[cfg(target_os = "linux")]
fn socket_paths(listed: &str) -> Vec<PathBuf> {
    listed
        .lines()
        .skip(1)
        .filter_map(|line| line.find(" /").map(|at| PathBuf::from(&line[at + 1..])))
        .collect()
}

/// The Landlock ABI this kernel speaks; 0 without Landlock.
#[cfg(target_os = "linux")]
pub(crate) fn landlock_abi() -> i64 {
    static ABI: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *ABI.get_or_init(|| {
        const VERSION: libc::c_uint = 1;
        // SAFETY: with no attributes and the version flag, the call only
        // reports the ABI.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<libc::c_void>(),
                0usize,
                VERSION,
            )
        };
        abi.max(0)
    })
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
        // Not a link among them: Landlock would allow what it points to,
        // a hidden path perhaps (a dotfiles link to `~/.ssh`). What a link
        // points to is reachable through it where it lies under a path
        // allowed on its own.
        let children = std::fs::read_dir(&root)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| !t.is_symlink()))
                    .map(|e| e.path())
                    .collect()
            })
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

    #[test]
    fn a_refused_certificate_service_says_what_to_do() {
        let trust = parse_denial(
            "Sandbox: Python(4242) deny(1) mach-lookup com.apple.trustd.agent\nkule-1-2",
        )
        .unwrap();
        assert!(trust.hint.unwrap().contains("legacy-certs"));
        let other =
            parse_denial("Sandbox: Python(4242) deny(1) mach-lookup com.apple.lsd.mapdb\nkule-1-2")
                .unwrap();
        assert_eq!(other.hint, None);
    }

    /// A server refused, or a write to the system's /tmp, says why and
    /// what works instead.
    #[test]
    fn a_refused_server_or_tmp_write_says_what_to_do() {
        let server =
            parse_denial("Sandbox: node(63452) deny(1) network-bind local:*:3000\nkule-1-2")
                .unwrap();
        assert!(server.hint.unwrap().contains("servers=on"));
        let tmp = parse_denial(
            "Sandbox: bash(63451) deny(1) file-write-create /private/tmp/kprobe-f\nkule-1-2",
        )
        .unwrap();
        assert!(tmp.hint.unwrap().contains("$TMPDIR"));
        let elsewhere =
            parse_denial("Sandbox: bash(1) deny(1) file-write-create /Users/me/x\nkule-1-2")
                .unwrap();
        assert_eq!(elsewhere.hint, None);
    }

    /// LibreOffice (soffice) aborts when the window server is refused, even
    /// with --headless; the refusal says why, and what works instead.
    #[test]
    fn a_refused_window_server_says_windows_do_not_run_in_a_task() {
        let screen = parse_denial(
            "Sandbox: soffice(77027) deny(1) mach-lookup com.apple.windowserver.active\nkule-1-2",
        )
        .unwrap();
        let hint = screen.hint.unwrap();
        assert!(
            hint.contains("window server") && hint.contains("pandoc"),
            "{hint}"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    /// Every socket path the kernel lists is read whole, spaces and all;
    /// abstract names and unbound sockets are none of them.
    #[test]
    fn socket_paths_are_read_whole() {
        let listed = "Num       RefCount Protocol Flags    Type St Inode Path\n\
                      0000000000000000: 00000002 00000000 00010000 0001 01 21345 /run/user/1000/bus\n\
                      0000000000000000: 00000002 00000000 00010000 0001 01  9876 /tmp/my app/agent.sock\n\
                      0000000000000000: 00000002 00000000 00010000 0001 01 11111 @/tmp/.X11-unix/X0\n\
                      0000000000000000: 00000003 00000000 00000000 0001 03 22222\n";
        assert_eq!(
            socket_paths(listed),
            [
                PathBuf::from("/run/user/1000/bus"),
                PathBuf::from("/tmp/my app/agent.sock")
            ]
        );
    }

    /// A list of sockets that cannot be read keeps nothing out, so the
    /// command does not start, rather than starting with every socket open.
    #[test]
    fn an_unreadable_socket_list_refuses() {
        let (home, ws) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let task = Task::create(home.path(), ws.path()).unwrap();
        let sandbox = Sandbox::new(&task).unwrap();
        assert!(
            sandbox
                .outside_sockets_in(&home.path().join("none"))
                .is_err()
        );
        let list = home.path().join("unix");
        std::fs::write(&list, "Num RefCount\n").unwrap();
        assert!(sandbox.outside_sockets_in(&list).unwrap().is_empty());
    }

    /// Only a directory of the person's own is covered, never `/`, nor one
    /// that holds the task's folders, which the cover would hide.
    #[test]
    fn only_the_persons_own_runtime_directory_is_covered() {
        let (home, ws, runtime) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let task = Task::create(home.path(), ws.path()).unwrap();
        let sandbox = Sandbox::new(&task).unwrap();
        let runtime = std::fs::canonicalize(runtime.path()).unwrap();
        let candidates = [
            runtime.clone(),
            runtime.join("."),
            PathBuf::from("/"),
            // Holds the task's folders.
            std::fs::canonicalize(home.path()).unwrap(),
            // Not the person's.
            PathBuf::from("/proc"),
            PathBuf::from("/nonexistent-runtime"),
        ];
        assert_eq!(sandbox.runtime_dirs(&candidates), [runtime]);
    }
}
