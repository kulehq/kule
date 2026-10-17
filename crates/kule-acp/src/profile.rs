//! Agent profiles: how to start an agent over ACP, and the narrow extra room
//! each one needs in the sandbox.
//!
//! Kule's own profiles are in `agents.toml` next to this file. A person adds
//! more in `<kule home>/agents.toml`, in the same format. Kule's home is
//! hidden from every sandbox, so no agent can write a profile for itself,
//! and profiles are never read from a workspace, which an agent could. A
//! profile opens holes in the sandbox, so one of the person's runs only
//! once they approved it as it stands (`kule agents approve`): changed
//! afterwards, by anyone, it waits for approval again. What a profile may
//! open at all is checked first (see [`Profile::check`]): never the
//! credentials Kule hides, never another agent's state, never a whole
//! directory like `~/Documents`, and only exact package versions.

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path};
use std::process::Command;

use kule_engine::{
    CacheSources, Error, HIDDEN_IN_HOME, Kept, Network, Policy, Proxy, Result, Sandbox, Task,
};
use serde::{Deserialize, Serialize};

/// Kule's own profiles.
const BUILT_IN: &str = include_str!("agents.toml");

/// The person's profiles, in Kule's home.
pub const AGENTS_FILE: &str = "agents.toml";
/// The person's profiles as they approved them, by name: a hash of each.
const APPROVED_FILE: &str = "agents-approved.json";

/// Paths no profile may claim whole: they hold far more than one agent's
/// state.
const TOO_WIDE: &[&str] = &[
    ".config",
    ".local",
    ".local/share",
    ".local/state",
    ".cache",
    ".npm",
    ".cargo",
    ".rustup",
    "Library",
    "Library/Application Support",
    "Library/Caches",
    "Library/Preferences",
    "Library/Containers",
    "Library/Group Containers",
    "Documents",
    "Desktop",
    "Downloads",
    "Pictures",
    "Movies",
    "Music",
    "Public",
];

/// The only hidden path a profile may reveal: the macOS keychain, whose
/// items each keep their own access list.
const KEYCHAIN: &str = "Library/Keychains";

/// Kule's home when `KULE_HOME` does not say otherwise, relative to the home
/// directory.
const DEFAULT_HOME: &str = ".kule";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    /// Starts the agent speaking ACP on stdio.
    pub command: Vec<String>,
    /// Paths under the home directory the agent writes its own state to.
    #[serde(default)]
    pub state: Vec<String>,
    /// Paths in that state that configure what runs outside the sandbox
    /// later (hooks, MCP servers, sandbox settings): readable, never
    /// writable.
    #[serde(default)]
    pub protected: Vec<String>,
    /// Its login inside that state, hidden from every other agent and from
    /// commands Kule runs itself.
    #[serde(default)]
    pub login: Vec<String>,
    /// Default-hidden paths under the home directory the agent needs to read
    /// (macOS only: the keychain holding its login).
    #[serde(default)]
    pub reveal_on_macos: Vec<String>,
    /// The session mode to start in, when the agent offers it: one that
    /// leaves sandboxing to Kule, for an agent whose own sandbox cannot
    /// start inside Kule's.
    #[serde(default)]
    pub mode: Option<String>,
    /// Environment variables the agent is started with.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The person's environment variables the agent may see: its own
    /// credentials, when it signs in by key. Others stay out.
    #[serde(default)]
    pub pass_env: Vec<String>,
    /// HTTPS hosts the agent needs for its login and model, beyond what the
    /// policy already lets through. Open only while this agent runs.
    #[serde(default)]
    pub hosts: Vec<String>,
    /// The other agents' state and logins, hidden from this one; filled in
    /// when the profiles are loaded.
    #[serde(skip)]
    others: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    agent: Vec<Profile>,
}

/// Where a profile comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    BuiltIn,
    /// `<kule home>/agents.toml`.
    Person,
}

/// A profile as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub profile: Profile,
    pub origin: Origin,
    /// Why it cannot run now: it fails a check, or it was never approved as
    /// it stands. `None` when it can.
    pub held: Option<String>,
}

/// Every profile Kule knows.
#[derive(Debug, Clone)]
pub struct Agents {
    listed: Vec<Listed>,
}

impl Agents {
    /// Kule's own profiles alone.
    pub fn built_in() -> Agents {
        let file: File = toml::from_str(BUILT_IN).expect("the built-in profiles parse");
        let listed = file
            .agent
            .into_iter()
            .map(|profile| Listed {
                profile,
                origin: Origin::BuiltIn,
                held: None,
            })
            .collect();
        Agents::complete(listed)
    }

    /// Kule's own profiles and the person's, from `kule_home`.
    pub fn load(kule_home: &Path) -> Result<Agents> {
        let mut agents = Agents::built_in();
        let path = kule_home.join(AGENTS_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(agents),
            Err(e) => return Err(io(&path, e)),
        };
        let file: File = toml::from_str(&text)
            .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
        let approved = approved(kule_home)?;
        let kule_home_in_home =
            home_dir().and_then(|home| kule_home.strip_prefix(home).ok().map(Path::to_path_buf));
        let mut listed = std::mem::take(&mut agents.listed);
        for profile in file.agent {
            let held = if listed.iter().any(|l| l.profile.name == profile.name) {
                Some(format!("another profile is named {}", profile.name))
            } else if let Err(why) = profile.check(&listed, kule_home_in_home.as_deref()) {
                Some(why)
            } else if approved.get(&profile.name) != Some(&profile.digest()) {
                Some(format!(
                    "not approved as it stands; review it with `kule agents show {0}`, then `kule agents approve {0}`",
                    profile.name
                ))
            } else {
                None
            };
            listed.push(Listed {
                profile,
                origin: Origin::Person,
                held,
            });
        }
        Ok(Agents::complete(listed))
    }

    /// Notes in each profile what the others hold, to hide it.
    fn complete(mut listed: Vec<Listed>) -> Agents {
        let all: Vec<(String, Vec<String>)> = listed
            .iter()
            .map(|l| {
                let p = &l.profile;
                (
                    p.name.clone(),
                    p.state.iter().chain(&p.login).cloned().collect(),
                )
            })
            .collect();
        for l in &mut listed {
            l.profile.others = all
                .iter()
                .filter(|(name, _)| *name != l.profile.name)
                .flat_map(|(_, paths)| paths.iter().cloned())
                .collect();
        }
        Agents { listed }
    }

    /// Every profile, the ones that cannot run included.
    pub fn listed(&self) -> &[Listed] {
        &self.listed
    }

    /// The profile `name`, when it can run.
    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.listed
            .iter()
            .find(|l| l.profile.name == name && l.held.is_none())
            .map(|l| &l.profile)
    }

    /// The names of the profiles that can run.
    pub fn names(&self) -> Vec<String> {
        self.listed
            .iter()
            .filter(|l| l.held.is_none())
            .map(|l| l.profile.name.clone())
            .collect()
    }

    /// Hides every agent's login in `sandbox`, for a command that is none of
    /// them; `home` is the person's home directory.
    pub fn hide_logins(&self, sandbox: &mut Sandbox, home: &Path) -> Result<()> {
        for listed in &self.listed {
            for login in &listed.profile.login {
                sandbox.hide(&home.join(login))?;
            }
        }
        Ok(())
    }

    /// Why `name` cannot run, for a person who asked for it.
    pub fn refusal(&self, name: &str) -> String {
        match self.listed.iter().find(|l| l.profile.name == name) {
            Some(Listed {
                held: Some(why), ..
            }) => format!("agent {name} cannot run: {why}"),
            _ => format!("no agent {name}; known: {}", self.names().join(", ")),
        }
    }

    /// Records the person's profile `name`, as it stands now, as approved.
    /// Refused for one that fails its checks.
    pub fn approve(kule_home: &Path, name: &str) -> Result<Profile> {
        let agents = Agents::load(kule_home)?;
        let listed = agents
            .listed
            .iter()
            .find(|l| l.profile.name == name && l.origin == Origin::Person)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "no agent {name} in {}",
                    kule_home.join(AGENTS_FILE).display()
                ))
            })?;
        let kule_home_in_home =
            home_dir().and_then(|home| kule_home.strip_prefix(home).ok().map(Path::to_path_buf));
        let others: Vec<Listed> = agents
            .listed
            .iter()
            .filter(|l| l.profile.name != name)
            .cloned()
            .collect();
        listed
            .profile
            .check(&others, kule_home_in_home.as_deref())
            .map_err(Error::Invalid)?;
        let mut approved = approved(kule_home)?;
        approved.insert(name.to_owned(), listed.profile.digest());
        write_approved(kule_home, &approved)?;
        Ok(listed.profile.clone())
    }

    /// Takes the approval of the person's profile `name` back.
    pub fn revoke(kule_home: &Path, name: &str) -> Result<bool> {
        let mut approved = approved(kule_home)?;
        let had = approved.remove(name).is_some();
        write_approved(kule_home, &approved)?;
        Ok(had)
    }
}

/// One of Kule's own profiles.
pub fn profile(name: &str) -> Option<Profile> {
    Agents::built_in().get(name).cloned()
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn approved(kule_home: &Path) -> Result<HashMap<String, String>> {
    let path = kule_home.join(APPROVED_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| Error::State { path, source }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(io(&path, e)),
    }
}

fn write_approved(kule_home: &Path, approved: &HashMap<String, String>) -> Result<()> {
    let path = kule_home.join(APPROVED_FILE);
    let tmp = kule_home.join(format!(".{APPROVED_FILE}.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(approved).map_err(|source| Error::State {
        path: path.clone(),
        source,
    })?;
    std::fs::write(&tmp, bytes).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))
}

/// Whether `a` is `b` or inside it, as relative paths.
fn within(a: &str, b: &str) -> bool {
    Path::new(a).starts_with(b)
}

impl Profile {
    /// What approval pins: the profile exactly as written.
    fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        blake3::hash(&bytes).to_hex().to_string()
    }

    /// Whether the profile may run at all, next to `others`; why not, when
    /// it may not. `kule_home` is Kule's home relative to the home
    /// directory, when it is in there.
    pub fn check(
        &self,
        others: &[Listed],
        kule_home: Option<&Path>,
    ) -> std::result::Result<(), String> {
        let name_ok = !self.name.is_empty()
            && self.name.len() <= 40
            && self
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !name_ok {
            return Err(format!(
                "name {:?}: use lowercase letters, digits and dashes",
                self.name
            ));
        }
        let Some(program) = self.command.first().filter(|p| !p.is_empty()) else {
            return Err("command is empty".to_owned());
        };
        check_pinned(program, &self.command[1..])?;

        let paths = self
            .state
            .iter()
            .chain(&self.login)
            .chain(&self.protected)
            .chain(&self.reveal_on_macos);
        for path in paths {
            let relative = !path.is_empty()
                && Path::new(path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)));
            if !relative {
                return Err(format!(
                    "{path:?}: paths are relative to the home directory, without .. or a leading /"
                ));
            }
        }
        for path in &self.state {
            if TOO_WIDE.iter().any(|wide| path == wide) {
                return Err(format!(
                    "state {path:?} holds far more than one agent's own; name the agent's directory in it"
                ));
            }
        }
        // Kule hides the built-in agents' logins too; those are kept from
        // the others by their state, below.
        let built_in = Agents::built_in();
        let logins: Vec<&str> = built_in
            .listed
            .iter()
            .flat_map(|l| l.profile.login.iter().map(String::as_str))
            .collect();
        let hidden = HIDDEN_IN_HOME.iter().filter(|h| !logins.contains(h));
        let homes = [Some(DEFAULT_HOME), kule_home.and_then(Path::to_str)];
        for path in self.state.iter().chain(&self.login).chain(&self.protected) {
            if let Some(h) = hidden.clone().find(|h| within(path, h) || within(h, path)) {
                return Err(format!("{path:?} reaches {h}, which Kule keeps hidden"));
            }
            if homes
                .iter()
                .flatten()
                .any(|home| within(path, home) || within(home, path))
            {
                return Err(format!("{path:?} reaches Kule's own home"));
            }
            for other in others {
                let theirs = other.profile.state.iter().chain(&other.profile.login);
                if let Some(t) = theirs
                    .into_iter()
                    .find(|t| within(path, t) || within(t, path))
                {
                    return Err(format!(
                        "{path:?} reaches {t}, which agent {} keeps",
                        other.profile.name
                    ));
                }
            }
        }
        for path in &self.reveal_on_macos {
            if path != KEYCHAIN {
                return Err(format!(
                    "reveal_on_macos {path:?}: only {KEYCHAIN} may be revealed"
                ));
            }
        }
        for login in &self.login {
            if !self.state.iter().any(|s| within(login, s)) {
                return Err(format!("login {login:?} is not in the agent's state"));
            }
        }
        for name in self.env.keys().chain(&self.pass_env) {
            let ok = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit());
            if !ok {
                return Err(format!("{name:?} is not an environment variable name"));
            }
            if name.to_ascii_uppercase().ends_with("PROXY") {
                return Err(format!(
                    "{name}: the network goes through Kule's proxy, which sets these"
                ));
            }
        }
        for host in &self.hosts {
            let labels: Vec<&str> = host.split('.').collect();
            let ok = labels.len() >= 2
                && labels.iter().all(|l| {
                    !l.is_empty()
                        && !l.starts_with('-')
                        && !l.ends_with('-')
                        && l.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                })
                && !labels
                    .last()
                    .is_some_and(|tld| tld.chars().all(|c| c.is_ascii_digit()));
            if !ok {
                return Err(format!(
                    "host {host:?}: a lowercase domain name, without a port, a scheme or a wildcard"
                ));
            }
        }
        Ok(())
    }

    /// Opens the agent's own state and login in `sandbox`. Paths that do not
    /// exist are skipped: the agent has never run here.
    /// Other agents' state, their logins and what the person said to them,
    /// is hidden. `home` is the person's home directory.
    pub fn apply(&self, sandbox: &mut Sandbox, home: &Path) -> Result<()> {
        for other in &self.others {
            sandbox.hide(&home.join(other))?;
        }
        for state in &self.state {
            let path = home.join(state);
            if path.exists() {
                sandbox.allow_write(&path)?;
            }
        }
        for path in &self.login {
            sandbox.reveal(&home.join(path))?;
        }
        for path in &self.protected {
            sandbox.protect(&home.join(path))?;
        }
        if cfg!(target_os = "macos") {
            for path in &self.reveal_on_macos {
                sandbox.reveal(&home.join(path))?;
            }
        }
        for name in &self.pass_env {
            sandbox.pass_env(name);
        }
        Ok(())
    }

    /// `policy` with this agent's hosts let through, under `profile-<name>`.
    pub fn open_hosts(&self, policy: Policy) -> Result<Policy> {
        let hosts: Vec<&str> = self.hosts.iter().map(String::as_str).collect();
        policy.permit_hosts(&format!("profile-{}", self.name), &hosts)
    }

    pub fn program(&self) -> &Path {
        Path::new(&self.command[0])
    }

    pub fn args(&self) -> Vec<String> {
        self.command[1..].to_vec()
    }

    /// Prepares the agent for `task`: the task's sandbox with this agent's
    /// state opened, its network through `proxy` (started with
    /// [`Profile::open_hosts`]).
    pub fn launch(&self, task: &Task, proxy: &Proxy) -> Result<Launch> {
        self.launch_for(task, proxy, CacheSources::from_env())
    }

    /// [`Profile::launch`] for the person whose home and caches `person`
    /// names.
    pub fn launch_for(
        &self,
        task: &Task,
        proxy: &Proxy,
        person: Option<CacheSources>,
    ) -> Result<Launch> {
        let home = person.as_ref().map(|p| p.home.clone());
        let mut sandbox = Sandbox::for_task_with(task, person)?;
        if let Some(home) = home {
            self.apply(&mut sandbox, &home)?;
        }
        sandbox.set_network(Network::Proxy(proxy.port()));
        let kept = sandbox.keep_protected()?;
        let mut command = sandbox.command(&self.program().to_string_lossy(), &self.args())?;
        command.envs(&self.env);
        // Started from the task's tmp, not the copy: `npx` would otherwise
        // take the repository's node_modules or .npmrc for the agent's own.
        // The agent still works in the copy, which its session names.
        command.current_dir(task.tmp()?);
        Ok(Launch {
            command,
            sandbox,
            kept,
            mode: self.mode.clone(),
        })
    }
}

/// A command that fetches its package on the fly must name one exact
/// version: an unpinned one runs whatever the registry serves that day.
fn check_pinned(program: &str, args: &[String]) -> std::result::Result<(), String> {
    let launcher = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    // The package is the first argument that is not a flag (after `run`,
    // for pipx).
    let mut words = args.iter().filter(|a| !a.starts_with('-'));
    let pinned = match launcher {
        "npx" | "bunx" | "pnpx" => words.next().map(|p| {
            // `@scope/name@1.2.3` or `name@1.2.3`.
            let version = p
                .rsplit_once('@')
                .filter(|(name, _)| !name.is_empty())
                .map(|(_, v)| v);
            (p.clone(), version.is_some_and(exact_version))
        }),
        "uvx" | "pipx" => {
            let mut word = words.next();
            if launcher == "pipx" && word.is_some_and(|w| w == "run") {
                word = words.next();
            }
            word.map(|p| {
                let version = p.split_once("==").map(|(_, v)| v);
                (p.clone(), version.is_some_and(exact_version))
            })
        }
        _ => None,
    };
    match pinned {
        Some((package, false)) => Err(format!(
            "{launcher} {package}: name one exact version (like name@1.2.3), not a range or latest"
        )),
        _ => Ok(()),
    }
}

/// `1.2.3`, with an optional pre-release or build: nothing that matches
/// more than one release.
fn exact_version(version: &str) -> bool {
    let core = version.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// An agent ready to start, and the sandbox it runs in, which reports what
/// it refused once the agent is done.
pub struct Launch {
    pub command: Command,
    pub sandbox: Sandbox,
    /// The agent's settings, to put back once it is gone where the sandbox
    /// could not keep them read-only.
    pub kept: Kept,
    /// The session mode to ask for (see [`Profile::mode`]).
    pub mode: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn every_built_in_profile_passes_its_own_checks() {
        let agents = Agents::built_in();
        for listed in agents.listed() {
            let others: Vec<Listed> = agents
                .listed()
                .iter()
                .filter(|l| l.profile.name != listed.profile.name)
                .cloned()
                .collect();
            assert_eq!(
                listed.profile.check(&others, Some(Path::new(".kule"))),
                Ok(()),
                "{}",
                listed.profile.name
            );
            assert!(listed.profile.open_hosts(Policy::builtin()).is_ok());
        }
        assert_eq!(agents.names(), ["claude-code", "gemini", "codex"]);
    }

    #[test]
    fn an_agent_starts_with_its_profiles_environment() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        let proxy = Proxy::start(&task, Policy::builtin()).unwrap();
        let launch = profile("gemini")
            .unwrap()
            .launch_for(&task, &proxy, None)
            .unwrap();
        let env: Vec<_> = launch.command.get_envs().collect();
        assert!(env.contains(&(
            OsStr::new("GEMINI_FORCE_FILE_STORAGE"),
            Some(OsStr::new("true"))
        )));
        // Not from the copy, where the repository could stand in for npx's
        // package.
        assert_eq!(
            launch.command.get_current_dir(),
            Some(task.tmp().unwrap().as_path())
        );
    }

    /// Runs `cat` on `path` in the sandbox `profile` gets, for `person`.
    fn reads(task: &Task, person: &Path, profile: &Profile, path: &str) -> bool {
        let mut sandbox = Sandbox::for_task_with(
            task,
            Some(CacheSources {
                home: person.to_owned(),
                cargo_home: person.join(".cargo"),
            }),
        )
        .unwrap();
        profile.apply(&mut sandbox, person).unwrap();
        let target = person.join(path);
        sandbox
            .command("cat", &[target.display().to_string()])
            .unwrap()
            .output()
            .unwrap()
            .status
            .success()
    }

    /// One agent's login and transcripts are none of another's business.
    #[test]
    fn an_agent_sees_its_own_login_and_no_other_agents_state() {
        let (home, ws, person) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let task = Task::create(home.path(), ws.path()).unwrap();
        let person = std::fs::canonicalize(person.path()).unwrap();
        for (dir, file) in [
            (".claude", ".credentials.json"),
            (".claude/projects", "session.jsonl"),
            (".codex", "auth.json"),
            (".gemini", "gemini-credentials.json"),
        ] {
            std::fs::create_dir_all(person.join(dir)).unwrap();
            std::fs::write(person.join(dir).join(file), "secret\n").unwrap();
        }
        let (claude, codex) = (profile("claude-code").unwrap(), profile("codex").unwrap());
        assert!(reads(&task, &person, &claude, ".claude/.credentials.json"));
        assert!(!reads(&task, &person, &claude, ".codex/auth.json"));
        assert!(!reads(
            &task,
            &person,
            &claude,
            ".gemini/gemini-credentials.json"
        ));
        assert!(reads(&task, &person, &codex, ".codex/auth.json"));
        assert!(!reads(&task, &person, &codex, ".claude/.credentials.json"));
        assert!(!reads(
            &task,
            &person,
            &codex,
            ".claude/projects/session.jsonl"
        ));
    }

    const OPENCODE: &str = r#"
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode", ".config/opencode"]
login = [".local/share/opencode/auth.json"]
protected = [".config/opencode/opencode.json"]
pass_env = ["OPENROUTER_API_KEY"]
hosts = ["openrouter.ai"]
"#;

    fn kule_home(agents: &str) -> TempDir {
        let home = TempDir::new().unwrap();
        std::fs::write(home.path().join(AGENTS_FILE), agents).unwrap();
        home
    }

    #[test]
    fn a_persons_profile_runs_only_once_approved_as_it_stands() {
        let home = kule_home(OPENCODE);
        let agents = Agents::load(home.path()).unwrap();
        assert!(agents.get("opencode").is_none());
        assert!(agents.refusal("opencode").contains("not approved"));

        let approved = Agents::approve(home.path(), "opencode").unwrap();
        assert_eq!(approved.hosts, ["openrouter.ai"]);
        let agents = Agents::load(home.path()).unwrap();
        assert!(agents.get("opencode").is_some());
        assert_eq!(
            agents.names(),
            ["claude-code", "gemini", "codex", "opencode"]
        );

        // Changed afterwards: held again until approved again.
        let widened = OPENCODE.replace(
            r#"hosts = ["openrouter.ai"]"#,
            r#"hosts = ["openrouter.ai", "example.com"]"#,
        );
        std::fs::write(home.path().join(AGENTS_FILE), widened).unwrap();
        assert!(Agents::load(home.path()).unwrap().get("opencode").is_none());

        Agents::approve(home.path(), "opencode").unwrap();
        assert!(Agents::load(home.path()).unwrap().get("opencode").is_some());
        assert!(Agents::revoke(home.path(), "opencode").unwrap());
        assert!(Agents::load(home.path()).unwrap().get("opencode").is_none());
    }

    #[test]
    fn a_persons_profile_hides_from_the_others_and_they_from_it() {
        let home = kule_home(OPENCODE);
        Agents::approve(home.path(), "opencode").unwrap();
        let agents = Agents::load(home.path()).unwrap();
        let claude = agents.get("claude-code").unwrap();
        assert!(
            claude
                .others
                .iter()
                .any(|p| p == ".local/share/opencode/auth.json")
        );
        let opencode = agents.get("opencode").unwrap();
        assert!(opencode.others.iter().any(|p| p == ".claude"));

        let (tasks, ws, person) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let task = Task::create(tasks.path(), ws.path()).unwrap();
        let person = std::fs::canonicalize(person.path()).unwrap();
        for (dir, file) in [
            (".claude", ".credentials.json"),
            (".local/share/opencode", "auth.json"),
        ] {
            std::fs::create_dir_all(person.join(dir)).unwrap();
            std::fs::write(person.join(dir).join(file), "secret\n").unwrap();
        }
        assert!(reads(
            &task,
            &person,
            opencode,
            ".local/share/opencode/auth.json"
        ));
        assert!(!reads(
            &task,
            &person,
            opencode,
            ".claude/.credentials.json"
        ));
        assert!(!reads(
            &task,
            &person,
            claude,
            ".local/share/opencode/auth.json"
        ));
        // Nor does a command that is no agent at all.
        let mut sandbox = Sandbox::for_task_with(
            &task,
            Some(CacheSources {
                home: person.clone(),
                cargo_home: person.join(".cargo"),
            }),
        )
        .unwrap();
        agents.hide_logins(&mut sandbox, &person).unwrap();
        let login = person.join(".local/share/opencode/auth.json");
        let cat = sandbox
            .command("cat", &[login.display().to_string()])
            .unwrap()
            .output()
            .unwrap();
        assert!(!cat.status.success());
    }

    /// The checks a person's profile must pass, each with the profile line
    /// that fails it.
    #[test]
    fn a_profile_cannot_open_what_kule_keeps_closed() {
        let failing = [
            ("name", r#"name = "Open Code""#, "use lowercase"),
            (
                "unpinned npx",
                r#"command = ["npx", "-y", "opencode-ai@latest", "acp"]"#,
                "exact version",
            ),
            (
                "ranged npx",
                r#"command = ["npx", "-y", "opencode-ai@^1.2.3", "acp"]"#,
                "exact version",
            ),
            (
                "no version",
                r#"command = ["npx", "-y", "opencode-ai", "acp"]"#,
                "exact version",
            ),
            (
                "unpinned uvx",
                r#"command = ["uvx", "some-agent", "acp"]"#,
                "exact version",
            ),
            ("absolute path", r#"state = ["/etc"]"#, "relative"),
            ("parent", r#"state = [".config/../.ssh"]"#, "relative"),
            ("too wide", r#"state = ["Documents"]"#, "far more"),
            ("hidden", r#"state = [".ssh"]"#, "Kule keeps hidden"),
            (
                "inside hidden",
                r#"state = [".aws/opencode"]"#,
                "Kule keeps hidden",
            ),
            (
                "around hidden",
                r#"state = [".config/gh/../gh"]"#,
                "relative",
            ),
            (
                "another agent",
                r#"state = [".claude"]"#,
                "agent claude-code keeps",
            ),
            (
                "another login",
                r#"login = [".codex/auth.json"]"#,
                "agent codex keeps",
            ),
            ("kule home", r#"state = [".kule"]"#, "Kule's own home"),
            (
                "keychain only",
                r#"reveal_on_macos = [".ssh"]"#,
                "only Library/Keychains",
            ),
            (
                "proxy env",
                r#"env = { HTTPS_PROXY = "http://evil" }"#,
                "proxy",
            ),
            (
                "env name",
                r#"pass_env = ["A-B"]"#,
                "not an environment variable",
            ),
            (
                "wildcard host",
                r#"hosts = ["*.example.com"]"#,
                "domain name",
            ),
            (
                "host with port",
                r#"hosts = ["example.com:8080"]"#,
                "domain name",
            ),
            ("ip host", r#"hosts = ["10.0.0.1"]"#, "domain name"),
            (
                "login outside state",
                r#"login = [".netrc"]"#,
                "Kule keeps hidden",
            ),
        ];
        for (what, line, reason) in failing {
            let key = line.split(" = ").next().unwrap();
            let profile: String = OPENCODE
                .lines()
                .filter(|l| !l.starts_with(&format!("{key} = ")))
                .chain([line])
                .collect::<Vec<_>>()
                .join("\n");
            let home = kule_home(&profile);
            assert!(
                Agents::approve(
                    home.path(),
                    if key == "name" {
                        "Open Code"
                    } else {
                        "opencode"
                    }
                )
                .is_err(),
                "{what}: {line}"
            );
            let agents = Agents::load(home.path()).unwrap();
            let held = agents
                .listed()
                .iter()
                .find(|l| l.origin == Origin::Person)
                .and_then(|l| l.held.clone())
                .unwrap_or_default();
            assert!(held.contains(reason), "{what}: {line}: held for {held:?}");
        }
    }

    #[test]
    fn a_persons_profile_cannot_take_a_built_in_name() {
        let home = kule_home(&OPENCODE.replace("name = \"opencode\"", "name = \"codex\""));
        let agents = Agents::load(home.path()).unwrap();
        let codex = agents.get("codex").unwrap();
        assert_eq!(codex.hosts, ["auth.openai.com", "chatgpt.com"]);
    }
}
