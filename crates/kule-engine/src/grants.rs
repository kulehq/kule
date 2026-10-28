//! What a task's commands may read: nothing by default, and then each root
//! granted on purpose, the same on every platform. The sandbox of each
//! platform (Seatbelt, Landlock, the process container) enforces the list
//! this module computes; none of them decides on its own what is readable.
//!
//! A grant comes from one of a few sources, each shown with it: the task's
//! own folders; the operating system's fixed base (its programs, libraries
//! and settings); where the person's tools are, as the variables passed to
//! commands name them (`PATH`, `JAVA_HOME`, …); the places toolchains keep
//! their own files in the home (`~/.cargo`, `~/.m2`, …); what the person
//! added with `kule tools add`, or an administrator in the tools file; and
//! what `kule reveal` and `kule exec --reveal` open for a folder or a
//! command. Nothing comes from the workspace or the task's copy: an agent
//! cannot widen its own sandbox.
//!
//! Every source but the task's own folders goes through one validator,
//! [`validate`]: a path that would weaken Kule, such as Kule's home, the
//! person's home or a folder above it, another person's home, a credential
//! folder, the root of a filesystem or a first-level system folder, is
//! refused whoever names it, and a path the validator cannot decide on is
//! refused too. Inside a grant, credentials stay denied by name (see
//! [`crate::sandbox::HIDDEN_IN_HOME`] and [`ETC_HIDDEN`]).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How far a command gets at a granted path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Access {
    /// Read and write: the task's copy, its temporary folder, its home
    /// layer and what `--allow-write` names.
    ReadWrite,
    /// Read only.
    Read,
    /// Listed, not read: a folder on the way to a grant, so paths resolve.
    List,
    /// Refused by name inside a grant: a credential file in a toolchain's
    /// folder.
    Denied,
}

/// Who named a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Source {
    /// The task's own folders.
    Task,
    /// The operating system's fixed base, as Kule knows it.
    System,
    /// A variable passed to commands: a `PATH` entry's root, an SDK's home.
    Env,
    /// Where a toolchain keeps its own files in the home, by default.
    ToolDefault,
    /// `kule tools add`.
    Person,
    /// The administrator's tools file.
    Admin,
    /// `kule reveal`: a project beside the workspace, for its folder.
    Beside,
    /// `kule exec --reveal`, for one command.
    Revealed,
    /// Kule's own list of what stays hidden inside a grant.
    Hidden,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Task => "task",
            Source::System => "system",
            Source::Env => "env",
            Source::ToolDefault => "tool",
            Source::Person => "person",
            Source::Admin => "admin",
            Source::Beside => "reveal",
            Source::Revealed => "--reveal",
            Source::Hidden => "hidden",
        })
    }
}

/// One path a task's commands may reach, how far, and who named it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// Resolved: the sandboxes match real paths.
    pub path: PathBuf,
    pub access: Access,
    pub source: Source,
}

/// Why a path is not granted. Each is a rule of its own, with a test that
/// fails without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    NotAbsolute,
    /// Not there, or not resolvable: nothing to grant.
    NotFound,
    /// Kule's home, or a folder holding it: other tasks' copies, the
    /// policy, the keys.
    KuleHome,
    /// The person's home itself, or a folder above it.
    Home,
    /// Another person's home, or inside one.
    OtherHome,
    /// The root of a filesystem or of a volume.
    Root,
    /// A first-level system folder (`/opt`, `/usr`, `/Users`): a folder
    /// inside one can be granted, the folder itself cannot.
    FirstLevel,
    /// A credential path Kule keeps from every command, or inside one.
    Hidden,
    /// One of the person's own folders (Documents, the projects beside
    /// the workspace): a tool's folder inside one can be granted, the
    /// folder itself cannot.
    Personal,
    /// Where apps keep settings and data (`~/.config`, `~/Library/Application
    /// Support`): a tool's folder inside one can be granted, the store
    /// itself cannot.
    Store,
    /// A folder the task writes, or one holding it.
    TaskPath,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refused::NotAbsolute => "not an absolute path",
            Refused::NotFound => "not found",
            Refused::KuleHome => "Kule's home, or a folder holding it",
            Refused::Home => "your home, or a folder above it",
            Refused::OtherHome => "another person's home",
            Refused::Root => "the root of a filesystem or volume",
            Refused::FirstLevel => "a first-level system folder; name a folder inside it",
            Refused::Hidden => "a path Kule keeps from every command",
            Refused::Personal => "one of your own folders; name a tool's folder inside it",
            Refused::Store => "where apps keep settings and data; name a tool's folder inside it",
            Refused::TaskPath => "a folder a task writes",
        })
    }
}

/// What the validator holds a path against: everything resolved.
#[derive(Debug, Clone, Default)]
pub struct Bounds {
    /// Kule's homes: the task's, and the default one.
    pub kule_homes: Vec<PathBuf>,
    /// The person's home.
    pub home: Option<PathBuf>,
    /// Credential paths and everything else Kule hides from every command.
    pub hidden: Vec<PathBuf>,
    /// The person's own folders (see `sandbox::personal_folders`).
    pub personal: Vec<PathBuf>,
    /// Where apps keep settings and data ([`crate::sandbox::SETTINGS_IN_HOME`]).
    pub stores: Vec<PathBuf>,
    /// What the task writes: its copy, tmp, home layer, allowed paths.
    pub writable: Vec<PathBuf>,
}

impl Bounds {
    /// The bounds for a person's grants outside any task (`kule tools
    /// add`): Kule's home at `kule_home`, the person's at `home`, with the
    /// credential folders, personal folders and stores Kule knows there.
    pub fn for_person(kule_home: &Path, home: &Path) -> Bounds {
        use crate::sandbox::{HIDDEN_IN_HOME, PERSONAL_IN_HOME, SETTINGS_IN_HOME};
        let resolve = |p: PathBuf| crate::sandbox::real_or_planned(&p).unwrap_or(p);
        let home = resolve(home.to_owned());
        let mut kule_homes = vec![resolve(kule_home.to_owned())];
        kule_homes.push(home.join(".kule"));
        let mut hidden: Vec<PathBuf> = HIDDEN_IN_HOME
            .iter()
            .map(|h| resolve(home.join(h)))
            .collect();
        hidden.extend(kule_homes.iter().cloned());
        let personal = PERSONAL_IN_HOME
            .iter()
            .map(|p| resolve(home.join(p)))
            .collect();
        let stores = SETTINGS_IN_HOME
            .iter()
            .map(|s| resolve(home.join(s)))
            .collect();
        Bounds {
            kule_homes,
            home: Some(home),
            hidden,
            personal,
            stores,
            writable: Vec::new(),
        }
    }
}

/// Holds `path` against `bounds` for a grant from `source`: the resolved
/// path to grant, or why not. The decision is made on the path as it
/// resolves now, and made again before every run.
///
/// Kule's own fixed base ([`Source::System`]) names first-level folders
/// (`/usr`, `/etc`) and is Kule's code, not a person's input: the rule
/// against first-level folders, and those against the person's own
/// folders and stores, do not apply to it. Every other rule does, to every
/// source.
pub fn validate(path: &Path, source: Source, bounds: &Bounds) -> Result<PathBuf, Refused> {
    if !path.is_absolute() {
        return Err(Refused::NotAbsolute);
    }
    let real = std::fs::canonicalize(path).map_err(|_| Refused::NotFound)?;
    let holds = |outer: &Path, inner: &Path| inner.starts_with(outer);
    // The widest first, so a refusal names the nearest reason: `/` holds
    // Kule's home too, but is refused as the root.
    if is_root(&real) {
        return Err(Refused::Root);
    }
    if let Some(home) = &bounds.home
        && holds(&real, home)
    {
        return Err(Refused::Home);
    }
    for kule in &bounds.kule_homes {
        if holds(&real, kule) || holds(kule, &real) {
            return Err(Refused::KuleHome);
        }
    }
    if let Some(home) = &bounds.home {
        // The homes of the others lie beside the person's.
        if let Some(homes) = home.parent()
            && homes.parent().is_some()
            && holds(homes, &real)
            && !holds(home, &real)
        {
            return Err(Refused::OtherHome);
        }
    }
    if source != Source::System {
        if is_first_level(&real) {
            return Err(Refused::FirstLevel);
        }
        if bounds.hidden.iter().any(|h| holds(h, &real)) {
            return Err(Refused::Hidden);
        }
        if bounds.personal.contains(&real) {
            return Err(Refused::Personal);
        }
        if bounds.stores.contains(&real) {
            return Err(Refused::Store);
        }
    }
    if bounds
        .writable
        .iter()
        .any(|w| holds(w, &real) || holds(&real, w))
    {
        return Err(Refused::TaskPath);
    }
    Ok(real)
}

/// Whether `real` is the root of a filesystem or a volume: `/`, a drive on
/// Windows, a mounted volume (`/Volumes/X`, `/mnt/X`, `/media/X`,
/// `/media/user/X`, `/run/media/user/X`, `/System/Volumes/X`).
fn is_root(real: &Path) -> bool {
    let parts: Vec<&std::ffi::OsStr> = real
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return true;
    }
    let s = |i: usize| parts.get(i).and_then(|p| p.to_str()).unwrap_or("");
    match parts.len() {
        2 => matches!(s(0), "Volumes" | "mnt" | "media"),
        3 => (s(0) == "System" && s(1) == "Volumes") || (s(0) == "media"),
        4 => s(0) == "run" && s(1) == "media",
        _ => false,
    }
}

/// Whether `real` is a first-level folder of the filesystem (`/opt`,
/// `/usr`, `/Users`, `C:\Program Files`).
fn is_first_level(real: &Path) -> bool {
    let normal = real
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    normal == 1
}

/// Credential files in `/etc` that a person, not only root, can read:
/// denied by name while `/etc` is granted whole. Root-only files there are
/// kept by the system itself.
pub const ETC_HIDDEN: &[&str] = &[
    "pip.conf",
    "xdg/pip/pip.conf",
    "docker/config.json",
    "rclone.conf",
    "samba/smb.conf",
    // Read by shells and the session as they start, where people put
    // tokens: the real files stay closed (see `sandbox::use_home`).
    "environment",
    "profile.d",
];

/// The folders a `PATH` entry stands for: its own root where it is a
/// tool's `bin` outside any home (`/opt/gradle/bin` → `/opt/gradle`), and
/// the entry itself. Inside a home (the person's, or any under `/Users`
/// or `/home`, whatever `HOME` says), or where the root would be a dot
/// folder (`~/.local/bin`), only the entry: the toolchains' own folders in
/// the home are granted as such ([`tool_defaults`]), and a parent would
/// reach beside them.
pub fn path_roots(entry: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let in_home = home.is_some_and(|h| entry.starts_with(h)) || under_a_home(entry);
    let is_bin = entry
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n, "bin" | "sbin" | "Scripts"));
    if is_bin
        && !in_home
        && let Some(parent) = entry.parent()
        && !parent
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'))
    {
        roots.push(parent.to_owned());
    }
    roots.push(entry.to_owned());
    roots
}

/// Whether `path` lies under where homes are kept (`/Users/x`, `/home/x`,
/// `/root`, `C:\Users\x`), whoever's it is.
fn under_a_home(path: &Path) -> bool {
    let mut parts = path.components().filter_map(|c| match c {
        Component::Normal(n) => n.to_str(),
        _ => None,
    });
    match parts.next() {
        Some("Users" | "home") => parts.next().is_some(),
        Some("root") => true,
        _ => false,
    }
}

/// Where toolchains keep their own files in the home by default, read
/// without any variable naming them (measured, see the design notes): a
/// command reads these, and nothing else at the top of the home. What
/// lies at the top of the home but is not here is simply not there for a
/// command. Credentials inside stay denied by name.
#[cfg(not(windows))]
pub const TOOL_DEFAULTS_IN_HOME: &[&str] = &[
    // Rust
    ".cargo",
    ".rustup",
    // Node
    ".npm",
    ".nvm",
    ".volta",
    ".bun",
    ".deno",
    ".yarn",
    ".pnpm-store",
    ".local/share/pnpm",
    ".local/share/fnm",
    // Python
    ".pyenv",
    ".local/share/uv",
    ".local/share/pipx",
    ".local/share/mise",
    ".cache",
    ".local/bin",
    ".local/lib",
    ".local/share/man",
    ".local/share/fonts",
    ".local/share/direnv",
    ".local/share/fish",
    // Go
    "go",
    // Java
    ".m2",
    ".gradle",
    ".jdks",
    ".sdkman",
    ".asdf",
    // .NET: NuGet's folder, but for its settings in it (`.nuget/NuGet`),
    // which can hold feed credentials and which NuGet will not run past
    // when refused; it makes its own in the commands' home.
    ".dotnet",
    ".nuget",
    ".templateengine",
    // Ruby
    ".rbenv",
    ".gem",
    // Version managers and shells' own folders
    ".mise",
    ".oh-my-zsh",
    ".zsh",
    ".bash_completion.d",
    // macOS: toolchains' own places under Library
    "Library/Java/JavaVirtualMachines",
    "Library/Caches/Homebrew",
    "Library/Caches/pip",
    "Library/Caches/go-build",
    "Library/Caches/uv",
    "Library/Developer/CommandLineTools",
    "Library/Application Support/go",
    "Library/Application Support/pip",
    "Library/Application Support/pypoetry",
    "Library/Application Support/ruff",
    "Library/Application Support/jj",
    "Library/Application Support/fnm",
    "Library/Application Support/Microsoft/DeveloperTools",
];

/// Settings files at the top of the home that tools read by their own
/// names, beside what shells run as they start (see `home::STARTUP`,
/// cloned into the task's home layer): readable, never a place for
/// credentials. A file that can carry a token (`.netrc`, `.npmrc`,
/// `.curlrc`, `.wgetrc`, `.terraformrc`, …) is not here: it is not there
/// for a command.
#[cfg(not(windows))]
pub const SETTINGS_FILES_IN_HOME: &[&str] = &[
    ".gitconfig",
    ".gitignore",
    ".gitignore_global",
    ".gitattributes",
    ".gitattributes_global",
    ".CFUserTextEncoding",
    ".hushlogin",
    ".editorconfig",
    ".tool-versions",
    ".inputrc",
    ".vimrc",
    ".gvimrc",
    ".tmux.conf",
    ".screenrc",
    ".ripgreprc",
    ".ignore",
    ".rgignore",
    ".fdignore",
    ".clang-format",
    ".clangd",
    ".selected_editor",
    ".lesskey",
    ".dir_colors",
    ".dircolors",
    ".sqliterc",
    ".psqlrc",
    ".mavenrc",
    ".sdkmanrc",
    ".nvmrc",
    ".node-version",
    ".python-version",
    ".ruby-version",
    ".java-version",
    ".terraform-version",
    ".config/starship.toml",
    // Settings with index URLs that may carry a token: cloned into the
    // layer with the token masked (see `home::masks_home`).
    ".pydistutils.cfg",
    ".pip/pip.conf",
];

/// [`SETTINGS_FILES_IN_HOME`] on Windows: the layer is not made there, and
/// the profile is the task's own.
#[cfg(windows)]
pub const SETTINGS_FILES_IN_HOME: &[&str] = &[];

/// [`TOOL_DEFAULTS_IN_HOME`] on Windows: what `sandbox::windows` grants
/// under the profile today.
#[cfg(windows)]
pub const TOOL_DEFAULTS_IN_HOME: &[&str] = &[
    ".cargo", ".rustup", ".dotnet", ".bun", ".deno", ".pyenv", "scoop", "go", ".m2", ".gradle",
    ".jdks", ".nuget", ".npm",
];

/// Where the person's toolchains keep their own files by default, to be
/// held against the validator: [`TOOL_DEFAULTS_IN_HOME`] and
/// [`SETTINGS_FILES_IN_HOME`] in `home`, and on Windows the folders under
/// the application-data folders where package managers install
/// themselves.
pub fn tool_defaults(home: &Path) -> Vec<PathBuf> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut roots: Vec<PathBuf> = TOOL_DEFAULTS_IN_HOME
        .iter()
        .chain(SETTINGS_FILES_IN_HOME)
        .map(|rel| home.join(rel))
        .collect();
    #[cfg(windows)]
    {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
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
    }
    roots
}

/// The operating system's fixed base on this platform: its programs,
/// libraries, settings and services' files, as toolchains read them
/// (measured). Only what exists here is granted.
#[cfg(target_os = "macos")]
pub fn system_roots() -> Vec<PathBuf> {
    [
        "/System/Library",
        "/System/Cryptexes",
        "/System/Volumes/Preboot/Cryptexes",
        "/System/iOSSupport",
        "/System/Applications/Utilities/Terminal.app",
        "/usr",
        "/bin",
        "/sbin",
        "/private/etc",
        "/private/var/db",
        "/private/var/select",
        "/Library/Developer",
        "/Library/Apple",
        "/Library/Frameworks",
        "/Library/Java",
        "/Library/Fonts",
        "/Library/Preferences",
        "/Library/Ruby",
        "/Library/Python",
        "/Library/Perl",
        "/opt/homebrew",
        "/opt/local",
        "/opt/X11",
        "/private/tmp/.dotnet",
        "/nix",
    ]
    .into_iter()
    .map(PathBuf::from)
    .chain(xcode_apps())
    .filter(|p| p.exists())
    .collect()
}

/// Xcode and its tools, where they are installed: every `Xcode*.app` in
/// `/Applications`, and what the system's developer directory link points
/// at.
#[cfg(target_os = "macos")]
fn xcode_apps() -> Vec<PathBuf> {
    let mut apps = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/Applications") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("Xcode") && name.ends_with(".app") {
                apps.push(entry.path());
            }
        }
    }
    if let Ok(target) = std::fs::read_link("/private/var/db/xcode_select_link")
        && let Some(app) = target
            .ancestors()
            .find(|a| a.extension().is_some_and(|e| e == "app"))
    {
        apps.push(app.to_owned());
    }
    apps
}

/// [`system_roots`] on Linux: the distribution's folders, the kernel's
/// views (`/proc`, `/sys`), the resolver's files under `/run`, and the
/// narrow places toolchains read under `/var` and `/tmp`. Never `/opt`,
/// `/srv`, `/mnt`, `/run` or `/tmp` whole.
#[cfg(target_os = "linux")]
pub fn system_roots() -> Vec<PathBuf> {
    [
        "/usr",
        "/bin",
        "/sbin",
        "/lib",
        "/lib32",
        "/lib64",
        "/libx32",
        "/etc",
        "/proc",
        "/sys",
        "/run/systemd/resolve",
        "/run/NetworkManager/resolv.conf",
        "/run/current-system",
        "/nix",
        "/snap",
        "/var/lib/snapd/snap",
        "/var/lib/dhcp",
        "/var/lib/flatpak/exports",
        "/var/lib/ca-certificates",
        "/tmp/.dotnet",
        "/tmp/.X11-unix",
    ]
    .into_iter()
    .map(PathBuf::from)
    .filter(|p| p.exists())
    .collect()
}

/// [`system_roots`] on Windows: Windows itself and the program folders,
/// as `sandbox::windows` granted them.
#[cfg(windows)]
pub fn system_roots() -> Vec<PathBuf> {
    [
        "SystemRoot",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "ProgramData",
    ]
    .into_iter()
    .filter_map(|name| std::env::var_os(name).map(PathBuf::from))
    .filter(|p| p.is_dir())
    .collect()
}

/// `roots`, each resolved and held against `bounds` for `source`: those
/// granted, sorted, one inside another left out, with those refused and
/// why. A path that is not there is left out quietly: a toolchain not
/// installed names nothing.
pub fn granted(
    roots: impl IntoIterator<Item = PathBuf>,
    source: Source,
    bounds: &Bounds,
) -> (Vec<Grant>, Vec<(PathBuf, Refused)>) {
    let mut paths = BTreeSet::new();
    let mut refused = Vec::new();
    for root in roots {
        match validate(&root, source, bounds) {
            Ok(real) => {
                paths.insert(real);
            }
            Err(Refused::NotFound) => {}
            Err(why) => refused.push((root, why)),
        }
    }
    let all: Vec<PathBuf> = paths.into_iter().collect();
    let grants = all
        .iter()
        .filter(|p| !all.iter().any(|o| o != *p && p.starts_with(o)))
        .map(|p| Grant {
            path: p.clone(),
            access: Access::Read,
            source,
        })
        .collect();
    (grants, refused)
}

/// What every task on this computer may read beyond its own folders, as
/// `kule tools list` shows it before any task: the system's base, the
/// person's tools from their environment, the toolchains' default places
/// in their home, what they added and what the administrator's file
/// names, each root with its source; and what was named but refused, with
/// why. A task's sandbox computes the same, with its own folders besides.
pub fn preview(kule_home: &Path, home: &Path) -> (Vec<Grant>, Vec<(PathBuf, Source, Refused)>) {
    let bounds = Bounds::for_person(kule_home, home);
    let real_home = bounds.home.clone();
    let mut all = Vec::new();
    let mut refused = Vec::new();
    let mut take = |roots: Vec<PathBuf>, source: Source| {
        let (grants, no) = granted(roots, source, &bounds);
        all.extend(grants);
        refused.extend(no.into_iter().map(|(p, why)| (p, source, why)));
    };
    take(system_roots(), Source::System);
    let tools = crate::sandbox::tools_from_env();
    take(
        tools
            .iter()
            .flat_map(|t| path_roots(t, real_home.as_deref()))
            .collect(),
        Source::Env,
    );
    if let Some(home) = &real_home {
        take(tool_defaults(home), Source::ToolDefault);
    }
    take(
        crate::tools::tools(kule_home)
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.path)
            .collect(),
        Source::Person,
    );
    take(crate::tools::admin_tools().paths, Source::Admin);
    let granted_paths: Vec<PathBuf> = all.iter().map(|g| g.path.clone()).collect();
    refused.retain(|(path, source, _)| {
        *source != Source::Env
            || !granted_paths
                .iter()
                .any(|g| g.starts_with(path) && g != path)
    });
    (all, refused)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_and_first_level_folders_are_told_apart() {
        assert!(is_root(Path::new("/")));
        assert!(is_root(Path::new("/Volumes/Data")));
        assert!(is_root(Path::new("/System/Volumes/Data")));
        assert!(is_root(Path::new("/mnt/c")));
        assert!(is_root(Path::new("/media/me/usb")));
        assert!(is_root(Path::new("/run/media/me/usb")));
        assert!(!is_root(Path::new("/Volumes/Data/sdk")));
        assert!(!is_root(Path::new("/opt")));
        assert!(is_first_level(Path::new("/opt")));
        assert!(is_first_level(Path::new("/Users")));
        assert!(!is_first_level(Path::new("/opt/x")));
        assert!(!is_first_level(Path::new("/")));
    }

    #[test]
    fn a_path_entry_stands_for_its_tools_root_outside_the_home() {
        let home = Path::new("/home/me");
        assert_eq!(
            path_roots(Path::new("/opt/gradle/bin"), Some(home)),
            vec![
                PathBuf::from("/opt/gradle"),
                PathBuf::from("/opt/gradle/bin")
            ]
        );
        assert_eq!(
            path_roots(Path::new("/home/me/.local/bin"), Some(home)),
            vec![PathBuf::from("/home/me/.local/bin")]
        );
        assert_eq!(
            path_roots(Path::new("/opt/tool"), Some(home)),
            vec![PathBuf::from("/opt/tool")]
        );
        // Another home than HOME says, and a dot folder's bin: the entry
        // alone, never the folder above it.
        assert_eq!(
            path_roots(Path::new("/Users/other/.local/bin"), Some(home)),
            vec![PathBuf::from("/Users/other/.local/bin")]
        );
        assert_eq!(
            path_roots(Path::new("/opt/.hidden/bin"), Some(home)),
            vec![PathBuf::from("/opt/.hidden/bin")]
        );
    }
}
