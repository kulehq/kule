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
    AgentKey, CacheSources, Error, HIDDEN_IN_HOME, Kept, Policy, Proxy, Result, Sandbox, Task,
};
use serde::{Deserialize, Serialize};

use crate::packages;

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

/// What runs as the person starts a shell, a session or a tool, or is found
/// on the PATH: never an agent's state.
const RUNS_AS_YOU_START: &[&str] = &[
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".zlogin",
    ".zlogout",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".kshrc",
    ".cshrc",
    ".tcshrc",
    ".xprofile",
    ".xinitrc",
    ".xsession",
    ".gitconfig",
    ".vimrc",
    ".config/git",
    ".config/fish",
    ".config/nvim",
    ".config/autostart",
    ".config/systemd",
    ".config/environment.d",
    "Library/LaunchAgents",
    "Library/LaunchDaemons",
    "Library/StartupItems",
    ".local/bin",
    ".cargo/bin",
    "bin",
    ".bin",
    "go/bin",
    ".npm-global",
];

/// The only hidden path a profile may reveal: the macOS keychain, whose
/// items each keep their own access list.
const KEYCHAIN: &str = "Library/Keychains";

/// Kule's home when `KULE_HOME` does not say otherwise, relative to the home
/// directory.
const DEFAULT_HOME: &str = ".kule";

/// Where agents with a home of their own in Kule keep it, relative to the
/// home directory: outside Kule's home, which the sandbox hides whole.
#[cfg(target_os = "macos")]
const OWN_HOMES: &str = "Library/Application Support/Kule/agents";
#[cfg(not(target_os = "macos"))]
const OWN_HOMES: &str = ".local/share/kule/agents";

/// Names in an agent's state that make it run something, or follow
/// instructions, the next time it starts: settings with hooks and MCP
/// servers, extensions, plugins, skills, rules for what runs unasked,
/// trusted folders. Written in Kule, they would reach the agent when the
/// person starts it outside Kule. Upper-case Markdown files (`CLAUDE.md`)
/// are its instructions, and count too.
const RUNS_LATER: &[&str] = &[
    "settings.json",
    "settings.local.json",
    "config.toml",
    "config.json",
    "config.yaml",
    "config.yml",
    "hooks",
    "extensions",
    "extension_integrity.json",
    "plugins",
    "skills",
    "commands",
    "agents",
    "rules",
    "prompts",
    "mcp.json",
    "mcp_servers.json",
    "trustedFolders.json",
];

/// Variables an agent's own home may not be put in: they are the
/// sandbox's.
const NOT_A_HOME: &[&str] = &["PATH", "TMPDIR", "TMP", "TEMP", "PWD", "SHELL"];

/// Credentials Kule knows, by variable or keychain service, with the hosts
/// they are for: a profile sends them nowhere else.
const KNOWN_CREDENTIALS: &[(&str, &[&str])] = &[
    ("ANTHROPIC_API_KEY", &["api.anthropic.com"]),
    ("ANTHROPIC_AUTH_TOKEN", &["api.anthropic.com"]),
    ("CLAUDE_CODE_OAUTH_TOKEN", &["api.anthropic.com"]),
    ("Claude Code-credentials", &["api.anthropic.com"]),
    ("OPENAI_API_KEY", &["api.openai.com"]),
    ("GEMINI_API_KEY", &["generativelanguage.googleapis.com"]),
    ("GOOGLE_API_KEY", &["generativelanguage.googleapis.com"]),
    (
        "GITHUB_TOKEN",
        &["api.github.com", "github.com", "uploads.github.com"],
    ),
    (
        "GH_TOKEN",
        &["api.github.com", "github.com", "uploads.github.com"],
    ),
    ("NPM_TOKEN", &["registry.npmjs.org"]),
    ("CARGO_REGISTRY_TOKEN", &["crates.io"]),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    /// Starts the agent speaking ACP on stdio.
    pub command: Vec<String>,
    /// Paths under the home directory the agent writes its own state to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub state: Vec<String>,
    /// Paths in that state that configure what runs outside the sandbox
    /// later (hooks, MCP servers, sandbox settings): readable, never
    /// writable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected: Vec<String>,
    /// Its login inside that state, hidden from every other agent and from
    /// commands Kule runs itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub login: Vec<String>,
    /// Default-hidden paths under the home directory the agent needs to read
    /// (macOS only: the keychain holding its login).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reveal_on_macos: Vec<String>,
    /// The session mode to start in, when the agent offers it: one that
    /// leaves sandboxing to Kule, for an agent whose own sandbox cannot
    /// start inside Kule's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Environment variables the agent is started with.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// The person's environment variables the agent may see: its own
    /// credentials, when it signs in by key. Others stay out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pass_env: Vec<String>,
    /// The person's variables holding the agent's API keys, each with the
    /// hosts it is for. The agent never sees the key: its variable holds a
    /// placeholder, and the task's proxy puts the key into requests to
    /// those hosts alone, over HTTPS.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keys: BTreeMap<String, Vec<String>>,
    /// Keys the agent keeps in the macOS keychain, by the variable it reads
    /// them from: Kule reads each there itself, and the agent gets a
    /// placeholder as for [`Profile::keys`], so neither it nor what it runs
    /// needs the keychain. A key of the same name set in the person's
    /// environment wins.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub keychain: BTreeMap<String, KeychainKey>,
    /// HTTPS hosts the agent needs for its login and model, beyond what the
    /// policy already lets through. Open only while this agent runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
    /// A home of its own in Kule, for an agent that reads where its state
    /// lives from a variable (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, or `HOME`
    /// itself): Kule points the variable at a folder of the agent's own, so
    /// what the agent writes in Kule never reaches it when the person runs
    /// it outside. It signs in there once, apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_home: Option<String>,
    /// The other agents' state and logins, hidden from this one; filled in
    /// when the profiles are loaded.
    #[serde(skip)]
    others: Vec<String>,
}

/// A key in the macOS keychain (see [`Profile::keychain`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeychainKey {
    /// The service of the generic password that holds it.
    pub service: String,
    /// Where in the item's JSON the key is, as `outer.inner`; the item
    /// itself without.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<String>,
    /// The hosts it is for.
    pub hosts: Vec<String>,
}

impl KeychainKey {
    /// The key out of `item`, the keychain item's text.
    pub fn pick(&self, item: &str) -> Option<String> {
        let item = item.trim();
        let Some(path) = &self.json else {
            return Some(item.to_owned()).filter(|v| !v.is_empty());
        };
        let mut at: serde_json::Value = serde_json::from_str(item).ok()?;
        for step in path.split('.') {
            at = at.get_mut(step)?.take();
        }
        at.as_str().filter(|v| !v.is_empty()).map(str::to_owned)
    }

    /// The key, read from the person's login keychain; macOS only.
    fn read(&self) -> Option<String> {
        if !cfg!(target_os = "macos") {
            return None;
        }
        let out = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", &self.service, "-w"])
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        self.pick(&String::from_utf8_lossy(&out.stdout))
    }
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
    /// An extension that may run brings it (see [`Listed::extension`]).
    Extension,
}

/// A profile as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listed {
    pub profile: Profile,
    pub origin: Origin,
    /// Why it cannot run now: it fails a check, or it was never approved as
    /// it stands. `None` when it can.
    pub held: Option<String>,
    /// What approval pins; an approval names it, so a profile changed
    /// since a person looked at it is not approved unseen.
    pub digest: String,
    /// What the person should know before approving it: what it can change
    /// that runs outside the sandbox later.
    #[serde(default)]
    pub warnings: Vec<String>,
    /// The program it starts, when it is not on `PATH` (or not at the path
    /// named): the agent cannot start until it is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
    /// Where its home of its own is, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_home_dir: Option<String>,
    /// The extension that brings it, for [`Origin::Extension`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

impl Listed {
    /// What its approval is kept under: its name, or for an extension's
    /// profile `ext/<extension>/<name>`, which no name can be.
    fn approval_key(&self) -> String {
        match &self.extension {
            Some(ext) => format!("ext/{ext}/{}", self.profile.name),
            None => self.profile.name.clone(),
        }
    }
}

/// The profiles of an `agents.toml`, read but not checked.
pub fn parse_profiles(text: &str) -> std::result::Result<Vec<Profile>, String> {
    toml::from_str::<File>(text)
        .map(|f| f.agent)
        .map_err(|e| e.to_string())
}

/// The profiles extensions that may run bring: (extension id, profile).
/// One whose file cannot be read now is left out (it was read when the
/// extension was installed).
fn extension_profiles(kule_home: &Path) -> Vec<(String, Profile)> {
    let store = kule_ext::Store::new(kule_home);
    let Ok(all) = store.list() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for listed in &all {
        // Only an extension that may run (on, approved as it is) brings any.
        let Ok(source) = store.runnable(&listed.id) else {
            continue;
        };
        let Some(path) = source.manifest().contributes.agents.clone() else {
            continue;
        };
        let Ok(profiles) = source
            .read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
            .and_then(|t| parse_profiles(&t))
        else {
            continue;
        };
        out.extend(profiles.into_iter().map(|p| (listed.id.clone(), p)));
    }
    out
}

/// The program `command` starts, when it cannot be: a bare name on no
/// directory of `path`, or a path to no executable file.
pub fn missing_program(command: &[String], path: Option<&std::ffi::OsStr>) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    let program = command.first()?;
    let runs = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let found = if program.contains('/') {
        runs(Path::new(program))
    } else {
        path.is_some_and(|path| std::env::split_paths(path).any(|dir| runs(&dir.join(program))))
    };
    (!found).then(|| program.clone())
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
                digest: String::new(),
                warnings: Vec::new(),
                missing: None,
                own_home_dir: None,
                extension: None,
            })
            .collect();
        Agents::complete(listed)
    }

    /// Kule's own profiles, the person's from `kule_home`, and those
    /// extensions that may run bring. The person's and the extensions'
    /// run only once approved as they stand.
    pub fn load(kule_home: &Path) -> Result<Agents> {
        let mut agents = Agents::built_in();
        let path = kule_home.join(AGENTS_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(io(&path, e)),
        };
        let person = match text {
            Some(text) => {
                toml::from_str::<File>(&text)
                    .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?
                    .agent
            }
            None => Vec::new(),
        };
        let theirs = extension_profiles(kule_home);
        if person.is_empty() && theirs.is_empty() {
            return Ok(agents);
        }
        let approved = approved(kule_home)?;
        let mut listed = std::mem::take(&mut agents.listed);
        let all = person
            .into_iter()
            .map(|p| (None, p))
            .chain(theirs.into_iter().map(|(ext, p)| (Some(ext), p)));
        for (extension, profile) in all {
            let mut entry = Listed {
                origin: if extension.is_some() {
                    Origin::Extension
                } else {
                    Origin::Person
                },
                profile,
                held: None,
                digest: String::new(),
                warnings: Vec::new(),
                missing: None,
                own_home_dir: None,
                extension,
            };
            let name = &entry.profile.name;
            entry.held = if listed.iter().any(|l| l.profile.name == *name) {
                Some(format!("another profile is named {name}"))
            } else if let Err(why) = entry
                .profile
                .check(&listed, kule_home_in_home(kule_home).as_deref())
            {
                Some(why)
            } else if approved.get(&entry.approval_key()) != Some(&entry.profile.digest()) {
                Some(format!(
                    "not approved as it stands; review it with `kule agents show {name}`, then `kule agents approve {name}`"
                ))
            } else {
                None
            };
            listed.push(entry);
        }
        Ok(Agents::complete(listed))
    }

    /// Takes back the approvals of every profile extension `id` brought.
    pub fn forget_extension(kule_home: &Path, id: &str) -> Result<()> {
        let mut approved = approved(kule_home)?;
        let prefix = format!("ext/{id}/");
        let before = approved.len();
        approved.retain(|key, _| !key.starts_with(&prefix));
        if approved.len() != before {
            write_approved(kule_home, &approved)?;
        }
        Ok(())
    }

    /// Notes in each profile what the others hold, to hide it, and what
    /// the person should know about it.
    fn complete(mut listed: Vec<Listed>) -> Agents {
        let all: Vec<(String, Vec<String>)> = listed
            .iter()
            .map(|l| (l.profile.name.clone(), l.profile.holds()))
            .collect();
        let home = person_home();
        let path = std::env::var_os("PATH");
        for l in &mut listed {
            l.digest = l.profile.digest();
            l.missing = missing_program(&l.profile.command, path.as_deref());
            l.profile.others = all
                .iter()
                .filter(|(name, _)| *name != l.profile.name)
                .flat_map(|(_, paths)| paths.iter().cloned())
                .collect();
            if let Some(home) = &home {
                l.warnings = l.profile.warnings(home);
                l.own_home_dir = l
                    .profile
                    .own_home
                    .as_ref()
                    .map(|_| home.join(l.profile.own_home_path()).display().to_string());
            }
        }
        Agents { listed }
    }

    /// `draft` as it would run next to these profiles: what they hold is
    /// hidden from it. For a trial before it is saved or approved.
    pub fn draft(&self, draft: &Profile) -> Profile {
        let mut draft = draft.clone();
        draft.others = self
            .listed
            .iter()
            .filter(|l| l.profile.name != draft.name)
            .flat_map(|l| l.profile.holds())
            .collect();
        draft
    }

    /// Whether `draft` may be saved next to these profiles; why not, when
    /// it may not.
    pub fn check_draft(
        &self,
        kule_home: &Path,
        draft: &Profile,
    ) -> std::result::Result<(), String> {
        if self
            .listed
            .iter()
            .any(|l| l.origin == Origin::BuiltIn && l.profile.name == draft.name)
        {
            return Err(format!("{} is one of Kule's own agents", draft.name));
        }
        let others: Vec<Listed> = self
            .listed
            .iter()
            .filter(|l| l.profile.name != draft.name)
            .cloned()
            .collect();
        draft.check(&others, kule_home_in_home(kule_home).as_deref())
    }

    /// Writes `profile` into the person's `agents.toml`, in place of the one
    /// with its name or after the others; the rest of the file, comments
    /// included, stays as it was. A saved profile is not approved: a
    /// changed one waits for approval again.
    pub fn save(kule_home: &Path, profile: &Profile) -> Result<()> {
        Agents::load(kule_home)?
            .check_draft(kule_home, profile)
            .map_err(Error::Invalid)?;
        let mut doc = person_file(kule_home)?;
        let table = toml_edit::ser::to_document(profile)
            .map_err(|e| Error::Invalid(format!("profile {}: {e}", profile.name)))?
            .as_table()
            .clone();
        let agents = agents_array(&mut doc)?;
        let at = agents
            .iter()
            .position(|t| t.get("name").and_then(|n| n.as_str()) == Some(profile.name.as_str()));
        match at {
            Some(at) => {
                if let Some(old) = agents.get_mut(at) {
                    *old = table;
                }
            }
            None => agents.push(table),
        }
        write_person_file(kule_home, &doc)
    }

    /// Takes `name` out of the person's `agents.toml`, and its approval.
    pub fn remove(kule_home: &Path, name: &str) -> Result<bool> {
        let mut doc = person_file(kule_home)?;
        let agents = agents_array(&mut doc)?;
        let before = agents.len();
        agents.retain(|t| t.get("name").and_then(|n| n.as_str()) != Some(name));
        let removed = agents.len() != before;
        if removed {
            write_person_file(kule_home, &doc)?;
        }
        Agents::revoke(kule_home, name)?;
        Ok(removed)
    }

    /// [`Agents::approve`], but only if the profile still has `digest`: what
    /// the person looked at is what gets approved.
    pub fn approve_seen(kule_home: &Path, name: &str, digest: &str) -> Result<Profile> {
        Agents::approve_as(kule_home, name, Some(digest))
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
        sandbox.hide(&home.join(OWN_HOMES))?;
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

    /// Records the profile `name` (the person's, or one an extension
    /// brings), as it stands now, as approved. Refused for one that fails
    /// its checks.
    pub fn approve(kule_home: &Path, name: &str) -> Result<Profile> {
        Agents::approve_as(kule_home, name, None)
    }

    /// [`Agents::approve`] of the profile as read once: when `seen` names
    /// a digest, the profile read is the one with it, and that profile is
    /// what is checked and approved, whatever the file says meanwhile.
    fn approve_as(kule_home: &Path, name: &str, seen: Option<&str>) -> Result<Profile> {
        let agents = Agents::load(kule_home)?;
        let listed = agents
            .listed
            .iter()
            .find(|l| l.profile.name == name && l.origin != Origin::BuiltIn)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "no agent {name} in {}",
                    kule_home.join(AGENTS_FILE).display()
                ))
            })?;
        if seen.is_some_and(|d| d != listed.digest) {
            return Err(Error::Invalid(format!(
                "agent {name} changed since you looked at it; look again"
            )));
        }
        let others: Vec<Listed> = agents
            .listed
            .iter()
            .filter(|l| l.profile.name != name)
            .cloned()
            .collect();
        listed
            .profile
            .check(&others, kule_home_in_home(kule_home).as_deref())
            .map_err(Error::Invalid)?;
        let mut approved = approved(kule_home)?;
        approved.insert(listed.approval_key(), listed.profile.digest());
        write_approved(kule_home, &approved)?;
        Ok(listed.profile.clone())
    }

    /// Takes the approval of profile `name` back.
    pub fn revoke(kule_home: &Path, name: &str) -> Result<bool> {
        let key = Agents::load(kule_home)?
            .listed
            .iter()
            .find(|l| l.profile.name == name && l.origin != Origin::BuiltIn)
            .map_or_else(|| name.to_owned(), Listed::approval_key);
        let mut approved = approved(kule_home)?;
        let had = approved.remove(&key).is_some();
        write_approved(kule_home, &approved)?;
        Ok(had)
    }
}

/// One of Kule's own profiles.
pub fn profile(name: &str) -> Option<Profile> {
    Agents::built_in().get(name).cloned()
}

/// An upper-case Markdown file: instructions an agent reads first
/// (`CLAUDE.md`, `AGENTS.md`).
fn instructions(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
    })
}

/// The person's home directory.
fn person_home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Kule's home relative to the person's home directory, when it is in there.
fn kule_home_in_home(kule_home: &Path) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    kule_home.strip_prefix(home).ok().map(Path::to_path_buf)
}

/// The person's `agents.toml` as written, or an empty one.
fn person_file(kule_home: &Path) -> Result<toml_edit::DocumentMut> {
    let path = kule_home.join(AGENTS_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(io(&path, e)),
    };
    text.parse()
        .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
}

/// The `[[agent]]` tables of `doc`, made when there are none.
fn agents_array(doc: &mut toml_edit::DocumentMut) -> Result<&mut toml_edit::ArrayOfTables> {
    doc.entry("agent")
        .or_insert_with(|| toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or_else(|| {
            Error::Invalid(format!(
                "{AGENTS_FILE}: `agent` is not a list of [[agent]] tables"
            ))
        })
}

fn write_person_file(kule_home: &Path, doc: &toml_edit::DocumentMut) -> Result<()> {
    let path = kule_home.join(AGENTS_FILE);
    let tmp = kule_home.join(format!(".{AGENTS_FILE}.{}", std::process::id()));
    std::fs::write(&tmp, doc.to_string()).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io(&path, e))
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
    /// A profile that starts `command` and opens nothing.
    pub fn bare(name: &str, command: Vec<String>, env: BTreeMap<String, String>) -> Profile {
        Profile {
            name: name.to_owned(),
            command,
            state: Vec::new(),
            protected: Vec::new(),
            login: Vec::new(),
            reveal_on_macos: Vec::new(),
            mode: None,
            env,
            pass_env: Vec::new(),
            keys: BTreeMap::new(),
            keychain: BTreeMap::new(),
            hosts: Vec::new(),
            own_home: None,
            others: Vec::new(),
        }
    }

    /// What this agent keeps, hidden from every other: its state, its
    /// login and its home of its own, relative to the home directory.
    fn holds(&self) -> Vec<String> {
        self.state
            .iter()
            .chain(&self.login)
            .cloned()
            .chain(std::iter::once(self.own_home_path()))
            .collect()
    }

    /// Its home of its own in Kule, relative to the home directory (whether
    /// it uses one or not).
    pub fn own_home_path(&self) -> String {
        format!("{OWN_HOMES}/{}", self.name)
    }

    /// What in its state under `home`, as it is now, can make the agent run
    /// something or follow instructions the next time it starts, and is not
    /// read-only: what a run in Kule could leave for a run outside it.
    pub fn unprotected(&self, home: &Path) -> Vec<String> {
        let protected = |rel: &str| self.protected.iter().any(|p| within(rel, p));
        let mut found = Vec::new();
        for state in &self.state {
            let Ok(entries) = std::fs::read_dir(home.join(state)) else {
                continue;
            };
            let mut names: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| RUNS_LATER.contains(&name.as_str()) || instructions(name))
                .collect();
            names.sort();
            for name in names {
                let rel = format!("{state}/{name}");
                if !protected(&rel) {
                    found.push(rel);
                }
            }
        }
        found
    }

    /// What the person should know before approving it.
    fn warnings(&self, home: &Path) -> Vec<String> {
        let mut warnings: Vec<String> = self
            .unprotected(home)
            .into_iter()
            .map(|path| {
                format!(
                    "~/{path} is writable: what the agent writes there in Kule takes effect when you start {} outside it. Make it read-only, or give the agent a home of its own.",
                    self.name
                )
            })
            .collect();
        if warnings.is_empty() && !self.state.is_empty() && self.protected.is_empty() {
            warnings.push(format!(
                "Nothing in its state is read-only: what the agent writes there in Kule reaches {} when you start it outside, its settings included. Make those read-only, or give the agent a home of its own.",
                self.name
            ));
        }
        // Which of the person's credentials it gets, and where they go.
        for (name, hosts) in &self.keys {
            warnings.push(format!(
                "Kule puts your {name} into what {} sends to {}.",
                self.name,
                hosts.join(", ")
            ));
        }
        for key in self.keychain.values() {
            warnings.push(format!(
                "Kule reads the keychain item \"{}\" and puts it into what {} sends to {}.",
                key.service,
                self.name,
                key.hosts.join(", ")
            ));
        }
        for name in &self.pass_env {
            warnings.push(format!("{} sees your {name} as it is.", self.name));
        }
        warnings
    }

    /// Makes its home of its own, when it has one; where it is.
    fn make_own_home(&self, home: &Path) -> Result<Option<std::path::PathBuf>> {
        use std::os::unix::fs::DirBuilderExt;
        if self.own_home.is_none() {
            return Ok(None);
        }
        let dir = home.join(self.own_home_path());
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| io(&dir, e))?;
        Ok(Some(dir))
    }

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
            // State is the agent's to write, past the home layer: never
            // what starts, or is found on the PATH, outside Kule.
            let overlaps = |starts: &str| within(path, starts) || within(starts, path);
            if let Some(starts) = RUNS_AS_YOU_START.iter().find(|s| overlaps(s)) {
                return Err(format!(
                    "state {path:?}: {starts} runs as you start your shell, your session or a tool; an agent's state is a directory of its own"
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
        let homes = [
            Some(DEFAULT_HOME),
            kule_home.and_then(Path::to_str),
            Some(OWN_HOMES),
            Some(packages::PACKAGES),
        ];
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
        if let Some(var) = &self.own_home {
            let ok = !var.is_empty()
                && var
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && !NOT_A_HOME.contains(&var.as_str())
                && !var.starts_with("LD_")
                && !var.starts_with("DYLD_")
                && !var.ends_with("PROXY");
            if !ok {
                return Err(format!(
                    "own_home {var:?}: the variable the agent reads its home or config directory from, like CODEX_HOME or HOME"
                ));
            }
        }
        for name in self.keys.keys().chain(self.keychain.keys()) {
            if self.env.contains_key(name) || self.pass_env.contains(name) {
                return Err(format!(
                    "{name}: a key the agent never sees cannot also be in env or pass_env"
                ));
            }
        }
        for (name, key) in &self.keychain {
            if key.hosts.is_empty() || key.service.trim().is_empty() {
                return Err(format!(
                    "{name}: name the keychain service and the hosts the key is for"
                ));
            }
        }
        for name in self
            .env
            .keys()
            .chain(&self.pass_env)
            .chain(self.keys.keys())
            .chain(self.keychain.keys())
        {
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
        for (name, hosts) in &self.keys {
            if hosts.is_empty() {
                return Err(format!("{name}: name the hosts the key is for"));
            }
        }
        // A credential Kule knows goes to its own service's hosts alone:
        // no profile sends the person's Anthropic key, or Claude Code's
        // sign-in, anywhere else.
        let bound = self
            .keys
            .iter()
            .map(|(name, hosts)| (name.as_str(), hosts))
            .chain(
                self.keychain
                    .values()
                    .map(|k| (k.service.as_str(), &k.hosts)),
            );
        for (name, hosts) in bound {
            if let Some((_, own)) = KNOWN_CREDENTIALS.iter().find(|(n, _)| *n == name)
                && let Some(other) = hosts.iter().find(|h| !own.contains(&h.as_str()))
            {
                return Err(format!(
                    "{name} goes to {} alone, not {other}",
                    own.join(", ")
                ));
            }
        }
        for name in &self.pass_env {
            if KNOWN_CREDENTIALS.iter().any(|(n, _)| n == name) {
                return Err(format!(
                    "{name}: a key, which the agent gets through `keys`, as a placeholder"
                ));
            }
        }
        for host in self
            .hosts
            .iter()
            .chain(self.keys.values().flatten())
            .chain(self.keychain.values().flat_map(|k| &k.hosts))
        {
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

    /// Makes the agent's state directories that are not there yet, which
    /// the sandbox could not open otherwise: an agent that only ever runs
    /// in Kule never made them itself. A state path named like a file
    /// (`.claude.json`) is left to the agent.
    pub fn make_state(&self, home: &Path) -> Result<()> {
        use std::os::unix::fs::DirBuilderExt;
        for state in &self.state {
            let path = home.join(state);
            let name = Path::new(state)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let file_like = name.trim_start_matches('.').contains('.');
            if file_like || std::fs::symlink_metadata(&path).is_ok() {
                continue;
            }
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&path)
                .map_err(|e| io(&path, e))?;
        }
        Ok(())
    }

    /// Opens the agent's own state and login in `sandbox`. Paths that do not
    /// exist are skipped: the agent has never run here.
    /// Other agents' state, their logins and what the person said to them,
    /// is hidden. `home` is the person's home directory.
    pub fn apply(&self, sandbox: &mut Sandbox, home: &Path) -> Result<()> {
        // State and logins as they are written: a link the agent put in
        // place of one (it can write there) opens nothing more.
        for state in &self.state {
            let path = home.join(state);
            if path.exists() {
                sandbox.allow_write_here(&path)?;
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
        // After the reveals, so none takes another agent's away.
        for other in &self.others {
            sandbox.hide(&home.join(other))?;
        }
        for name in &self.pass_env {
            sandbox.pass_env(name);
        }
        Ok(())
    }

    /// The agent's keys the person has set, with their values, for the
    /// task's proxy to put in for it (see [`Profile::keys`]).
    pub fn agent_keys(&self) -> Vec<AgentKey> {
        let mut keys: Vec<AgentKey> = self
            .keys
            .iter()
            .filter_map(|(name, hosts)| {
                let value = std::env::var(name).ok().filter(|v| !v.is_empty())?;
                Some(AgentKey {
                    name: name.clone(),
                    value,
                    hosts: hosts.clone(),
                })
            })
            .collect();
        for (name, key) in &self.keychain {
            if keys.iter().any(|k| &k.name == name) {
                continue;
            }
            if let Some(value) = key.read() {
                keys.push(AgentKey {
                    name: name.clone(),
                    value,
                    hosts: key.hosts.clone(),
                });
            }
        }
        keys
    }

    /// `policy` with this agent's hosts let through, under `profile-<name>`:
    /// the ones it names, and the ones its keys are for (its model's API).
    pub fn open_hosts(&self, policy: Policy) -> Result<Policy> {
        let mut hosts: Vec<&str> = self
            .hosts
            .iter()
            .chain(self.keys.values().flatten())
            .chain(self.keychain.values().flat_map(|k| &k.hosts))
            .map(String::as_str)
            .collect();
        hosts.sort_unstable();
        hosts.dedup();
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
        self.launch_with(task, proxy, CacheSources::from_env(), true)
    }

    /// [`Profile::launch`] for the person whose home and caches `person`
    /// names, without the home layer: what the agent writes in the home is
    /// refused, as a trial needs to learn its state directories.
    pub fn launch_for(
        &self,
        task: &Task,
        proxy: &Proxy,
        person: Option<CacheSources>,
    ) -> Result<Launch> {
        self.launch_with(task, proxy, person, false)
    }

    /// [`Profile::launch_for`], with the home layer when `layer` is set:
    /// what the agent writes in the home is caught for review (see
    /// [`Task::prepare_home`]).
    pub fn launch_with(
        &self,
        task: &Task,
        proxy: &Proxy,
        person: Option<CacheSources>,
        layer: bool,
    ) -> Result<Launch> {
        let home = person.as_ref().map(|p| p.home.clone());
        let (program, args) = self.started_as(home.as_deref())?;
        let mut sandbox = Sandbox::for_task_with(task, person)?;
        let mut own_home = None;
        if let Some(home) = home {
            self.make_state(&home)?;
            own_home = self.make_own_home(&home)?;
            self.apply(&mut sandbox, &home)?;
            if let Some(dir) = &own_home {
                sandbox.allow_write(dir)?;
            }
            // An agent whose own home is HOME has its whole home in Kule's
            // keeping already.
            if layer && self.own_home.as_deref() != Some("HOME") {
                let mut linked: Vec<std::path::PathBuf> =
                    self.state.iter().map(|s| home.join(s)).collect();
                linked.extend(own_home.clone());
                let ignored: Vec<String> = self
                    .state
                    .iter()
                    .filter_map(|s| s.split('/').next())
                    .map(str::to_owned)
                    .collect();
                let dir = task.prepare_home(&home, &linked, sandbox.hidden(), &ignored)?;
                sandbox.use_home(&dir, &home)?;
            }
        }
        sandbox.use_proxy(proxy);
        let kept = sandbox.keep_protected()?;
        let mut command = sandbox.command(&program, &args)?;
        command.envs(&self.env);
        if let (Some(var), Some(dir)) = (&self.own_home, &own_home) {
            command.env(var, dir);
        }
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

impl Profile {
    /// The package [`Profile::launch`] will install first, taking a while,
    /// when the agent is one `npx` fetches and it is not installed yet.
    pub fn to_install(&self) -> Option<String> {
        let home = CacheSources::from_env()?.home;
        let (spec, _, _) = packages::npx_package(&self.command)?;
        (!packages::is_installed(&home.join(packages::PACKAGES), &spec)).then_some(spec)
    }

    /// What starts the agent: for one `npx` fetches, its package as Kule
    /// installed it once under the person's `home` (see [`crate::packages`]),
    /// installed now when it is not there yet; `npx` itself when that
    /// fails, or with no home to install under.
    fn started_as(&self, home: Option<&Path>) -> Result<(String, Vec<String>)> {
        let as_written = (self.program().to_string_lossy().into_owned(), self.args());
        let (Some(home), Some((spec, name, rest))) = (home, packages::npx_package(&self.command))
        else {
            return Ok(as_written);
        };
        let announce = || {
            eprintln!(
                "kule: installing {spec} for {}, once; later runs start from it",
                self.name
            )
        };
        match packages::installed(&home.join(packages::PACKAGES), &spec, &name, announce) {
            Ok(script) => {
                let mut args = vec![script.to_string_lossy().into_owned()];
                args.extend(rest);
                Ok(("node".to_owned(), args))
            }
            // A package Kule ships a lockfile for starts as locked or not at
            // all: through npx its dependencies would float.
            Err(e) if packages::locked(&spec) => Err(Error::Invalid(format!(
                "{spec} could not be installed as locked ({e}); {} does not start without it",
                self.name
            ))),
            Err(e) => {
                eprintln!("kule: {e}; {} starts through npx instead", self.name);
                Ok(as_written)
            }
        }
    }
}

/// A command that fetches its package on the fly must name one exact
/// version: an unpinned one runs whatever the registry serves that day.
pub(crate) fn check_pinned(program: &str, args: &[String]) -> std::result::Result<(), String> {
    let launcher = Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);
    // Launchers that fetch a package by other names: one way, checked.
    let sub = args.first().map(String::as_str);
    if matches!(
        (launcher, sub),
        ("npm", Some("exec" | "x")) | ("pnpm" | "yarn", Some("dlx")) | ("bun", Some("x"))
    ) {
        return Err(format!(
            "{launcher} {}: start a package with npx and one exact version, like npx -y name@1.2.3",
            sub.unwrap_or_default()
        ));
    }
    // The package comes first (after npx's -y, pipx's run): an option
    // before it could say where it comes from, or name another.
    let mut words = args.iter().skip_while(|a| match launcher {
        "npx" | "bunx" | "pnpx" => *a == "-y" || *a == "--yes",
        "pipx" => *a == "run",
        _ => false,
    });
    if matches!(launcher, "npx" | "bunx" | "pnpx" | "uvx" | "pipx")
        && let Some(option) = args.iter().find(|a| a.starts_with('-'))
        && words
            .clone()
            .next()
            .is_none_or(|first| first.starts_with('-'))
    {
        return Err(format!(
            "{launcher} {option}: name the package first, with one exact version"
        ));
    }
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
                // `name==1.2.3`, or `name@1.2.3` as uvx also takes it.
                let version = p
                    .split_once("==")
                    .or_else(|| p.split_once('@'))
                    .map(|(_, v)| v);
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
pub(crate) fn exact_version(version: &str) -> bool {
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
    fn a_program_not_on_path_is_missing() {
        use std::os::unix::fs::PermissionsExt;
        let (one, two) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let tool = two.path().join("agent-acp");
        std::fs::write(&tool, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = one.path().join("plain");
        std::fs::write(&plain, "").unwrap();
        let path = std::env::join_paths([one.path(), two.path()]).unwrap();
        let cmd = |p: &str| vec![p.to_owned(), "--acp".to_owned()];

        assert_eq!(missing_program(&cmd("agent-acp"), Some(&path)), None);
        assert_eq!(missing_program(&cmd(tool.to_str().unwrap()), None), None);
        assert_eq!(
            missing_program(&cmd("npx"), Some(&path)).as_deref(),
            Some("npx")
        );
        assert_eq!(
            missing_program(&cmd("agent-acp"), None).as_deref(),
            Some("agent-acp")
        );
        // A file that cannot be run is not the program.
        assert!(missing_program(&cmd("plain"), Some(&path)).is_some());
        assert!(missing_program(&cmd(plain.to_str().unwrap()), None).is_some());
        assert_eq!(missing_program(&[], Some(&path)), None);
    }

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
            (
                "another package before",
                r#"command = ["npx", "--package=opencode-ai@latest", "other@1.2.3", "acp"]"#,
                "name the package first",
            ),
            (
                "another index",
                r#"command = ["uvx", "--index-url=https://evil.example", "some-agent==1.2.3"]"#,
                "name the package first",
            ),
            (
                "npm exec",
                r#"command = ["npm", "exec", "opencode-ai@latest"]"#,
                "start a package with npx",
            ),
            (
                "pnpm dlx",
                r#"command = ["pnpm", "dlx", "opencode-ai"]"#,
                "start a package with npx",
            ),
            ("shell start", r#"state = [".zshrc"]"#, "runs as you start"),
            (
                "launch agents",
                r#"state = ["Library/LaunchAgents"]"#,
                "runs as you start",
            ),
            (
                "inside a start folder",
                r#"state = [".config/autostart/opencode"]"#,
                "runs as you start",
            ),
            (
                "on the PATH",
                r#"state = [".local/bin"]"#,
                "runs as you start",
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
                "key also passed",
                "pass_env = [\"A_KEY\"]\nkeys = { A_KEY = [\"api.example.com\"] }",
                "never sees",
            ),
            (
                "key without hosts",
                r#"keys = { A_KEY = [] }"#,
                "name the hosts",
            ),
            (
                "key host",
                r#"keys = { A_KEY = ["https://api.example.com"] }"#,
                "domain name",
            ),
            (
                "key name",
                r#"keys = { "A-KEY" = ["api.example.com"] }"#,
                "not an environment variable",
            ),
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
    fn an_agents_keys_come_from_the_persons_environment_when_set() {
        let mut profile = Profile::bare("x", vec!["x".into()], BTreeMap::new());
        for name in ["KULE_TEST_AGENT_KEY_SET", "KULE_TEST_AGENT_KEY_UNSET"] {
            profile
                .keys
                .insert(name.into(), vec!["api.example.com".into()]);
        }
        // SAFETY: no other test reads or writes these names.
        unsafe { std::env::set_var("KULE_TEST_AGENT_KEY_SET", "sk-1") };
        assert_eq!(
            profile.agent_keys(),
            [AgentKey {
                name: "KULE_TEST_AGENT_KEY_SET".into(),
                value: "sk-1".into(),
                hosts: vec!["api.example.com".into()],
            }]
        );
        // The built-in agents never see their API keys.
        let home = tempfile::TempDir::new().unwrap();
        let agents = Agents::load(home.path()).unwrap();
        for (agent, key) in [
            ("claude-code", "ANTHROPIC_API_KEY"),
            ("claude-code", "ANTHROPIC_AUTH_TOKEN"),
            ("claude-code", "CLAUDE_CODE_OAUTH_TOKEN"),
            ("codex", "OPENAI_API_KEY"),
            ("gemini", "GEMINI_API_KEY"),
        ] {
            let p = agents.get(agent).unwrap();
            assert!(
                p.keys.contains_key(key) && !p.pass_env.iter().any(|n| n == key),
                "{agent}"
            );
        }
    }

    /// Claude Code keeps its sign-in in the keychain: Kule reads it there
    /// for the proxy, and the agent, with all it runs, sees no keychain.
    #[test]
    fn claude_codes_keychain_sign_in_is_kules_to_read_not_the_agents() {
        let home = tempfile::TempDir::new().unwrap();
        let agents = Agents::load(home.path()).unwrap();
        let claude = agents.get("claude-code").unwrap();
        assert!(
            claude.reveal_on_macos.is_empty(),
            "{:?}",
            claude.reveal_on_macos
        );
        let key = &claude.keychain["CLAUDE_CODE_OAUTH_TOKEN"];
        assert_eq!(key.service, "Claude Code-credentials");
        assert_eq!(key.hosts, ["api.anthropic.com"]);
        // The token inside the item as Claude Code writes it.
        let item = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-x","refreshToken":"r"}}"#;
        assert_eq!(key.pick(item).as_deref(), Some("sk-ant-oat01-x"));
        assert_eq!(key.pick("not json"), None);
    }

    /// Each built-in agent reaches its own model's API while it runs, and
    /// no other's: an agent's command cannot carry data to someone else's
    /// account at another provider.
    #[test]
    fn an_agent_opens_its_own_models_api_alone() {
        let home = tempfile::TempDir::new().unwrap();
        let agents = Agents::load(home.path()).unwrap();
        let apis = [
            "api.anthropic.com",
            "api.openai.com",
            "generativelanguage.googleapis.com",
        ];
        for (agent, own) in [
            ("claude-code", "api.anthropic.com"),
            ("codex", "api.openai.com"),
            ("gemini", "generativelanguage.googleapis.com"),
        ] {
            let policy = agents
                .get(agent)
                .unwrap()
                .open_hosts(Policy::builtin())
                .unwrap();
            for api in apis {
                let verdict = policy.judge_connection(api, 443).verdict;
                let want = if api == own {
                    kule_engine::Verdict::Allow
                } else {
                    kule_engine::Verdict::Ask
                };
                assert_eq!(verdict, want, "{agent} → {api}");
            }
        }
    }

    #[test]
    fn a_persons_profile_cannot_take_a_built_in_name() {
        let home = kule_home(&OPENCODE.replace("name = \"opencode\"", "name = \"codex\""));
        let agents = Agents::load(home.path()).unwrap();
        let codex = agents.get("codex").unwrap();
        assert_eq!(codex.hosts, ["auth.openai.com", "chatgpt.com"]);
    }

    #[test]
    fn saving_keeps_the_rest_of_the_file_and_needs_approval_again() {
        let file = format!("# My agents, by hand.\n{OPENCODE}");
        let home = kule_home(&file);
        Agents::approve(home.path(), "opencode").unwrap();

        // A new one goes after the others.
        let mut qwen = Profile::bare(
            "qwen-code",
            vec![
                "npx".into(),
                "-y".into(),
                "@qwen-code/qwen-code@0.25.2".into(),
                "--acp".into(),
            ],
            BTreeMap::new(),
        );
        qwen.state = vec![".qwen".into()];
        Agents::save(home.path(), &qwen).unwrap();
        let text = std::fs::read_to_string(home.path().join(AGENTS_FILE)).unwrap();
        assert!(text.starts_with("# My agents, by hand.\n"), "{text}");
        let agents = Agents::load(home.path()).unwrap();
        assert_eq!(
            agents
                .listed()
                .iter()
                .map(|l| l.profile.name.as_str())
                .collect::<Vec<_>>(),
            ["claude-code", "gemini", "codex", "opencode", "qwen-code"]
        );
        assert!(agents.get("opencode").is_some(), "the others stay approved");
        assert!(agents.get("qwen-code").is_none(), "a saved one is not");

        // One with the same name is replaced, and waits again.
        let mut opencode = agents.listed()[3].profile.clone();
        opencode.hosts.push("api.openai.com".into());
        Agents::save(home.path(), &opencode).unwrap();
        let agents = Agents::load(home.path()).unwrap();
        assert_eq!(agents.listed().len(), 5);
        assert_eq!(
            agents.listed()[3].profile.hosts,
            ["openrouter.ai", "api.openai.com"]
        );
        assert!(agents.get("opencode").is_none());

        // What fails its checks is not written.
        let mut wide = qwen.clone();
        wide.state = vec![".ssh".into()];
        assert!(Agents::save(home.path(), &wide).is_err());
        assert!(
            Agents::save(
                home.path(),
                &Profile::bare("codex", vec!["x".into()], BTreeMap::new())
            )
            .is_err()
        );

        assert!(Agents::remove(home.path(), "qwen-code").unwrap());
        assert_eq!(Agents::load(home.path()).unwrap().listed().len(), 4);
        assert!(!Agents::remove(home.path(), "qwen-code").unwrap());
    }

    #[test]
    fn only_what_the_person_looked_at_is_approved() {
        let home = kule_home(OPENCODE);
        let seen = Agents::load(home.path()).unwrap().listed()[3]
            .digest
            .clone();
        // Changed after the person looked.
        let changed =
            OPENCODE.replace(r#"hosts = ["openrouter.ai"]"#, r#"hosts = ["example.com"]"#);
        std::fs::write(home.path().join(AGENTS_FILE), changed).unwrap();
        assert!(Agents::approve_seen(home.path(), "opencode", &seen).is_err());
        assert!(Agents::load(home.path()).unwrap().get("opencode").is_none());

        let seen = Agents::load(home.path()).unwrap().listed()[3]
            .digest
            .clone();
        Agents::approve_seen(home.path(), "opencode", &seen).unwrap();
        assert!(Agents::load(home.path()).unwrap().get("opencode").is_some());
    }

    /// The profile approved is the one whose digest the person saw, even
    /// while the file changes between reads.
    #[test]
    fn an_approval_never_takes_a_profile_the_person_did_not_see() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let home = kule_home(OPENCODE);
        let seen = Agents::load(home.path()).unwrap().listed()[3]
            .digest
            .clone();
        let other = OPENCODE.replace(r#"hosts = ["openrouter.ai"]"#, r#"hosts = ["example.com"]"#);
        let file = home.path().join(AGENTS_FILE);
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let tmp = home.path().join("agents.toml.next");
                let mut n = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let text = if n.is_multiple_of(2) {
                        &other
                    } else {
                        OPENCODE
                    };
                    std::fs::write(&tmp, text).unwrap();
                    std::fs::rename(&tmp, &file).unwrap();
                    n += 1;
                }
            });
            // The writer stops however this ends, a failed assertion too.
            struct Stop<'a>(&'a AtomicBool);
            impl Drop for Stop<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
            let _stop = Stop(&stop);
            for _ in 0..400 {
                if Agents::approve_seen(home.path(), "opencode", &seen).is_ok() {
                    let approved = approved(home.path()).unwrap();
                    assert!(
                        approved.values().all(|d| *d == seen),
                        "approved a profile nobody saw: {approved:?}"
                    );
                }
            }
        });
    }

    /// What each of Kule's own agents reads when it starts that can make it
    /// run something or follow instructions: none of it may be writable.
    #[test]
    fn kules_own_agents_leave_nothing_writable_that_runs_later() {
        let reads: &[(&str, &[&str])] = &[
            (
                "claude-code",
                &[
                    ".claude/settings.json",
                    ".claude/settings.local.json",
                    ".claude/hooks",
                    ".claude/plugins",
                    ".claude/skills",
                    ".claude/commands",
                    ".claude/agents",
                    ".claude/CLAUDE.md",
                ],
            ),
            (
                "gemini",
                &[
                    ".gemini/settings.json",
                    ".gemini/extensions",
                    ".gemini/extension_integrity.json",
                    ".gemini/commands",
                    ".gemini/trustedFolders.json",
                    ".gemini/GEMINI.md",
                ],
            ),
            (
                "codex",
                &[
                    ".codex/config.toml",
                    ".codex/rules",
                    ".codex/plugins",
                    ".codex/skills",
                    ".codex/prompts",
                    ".codex/AGENTS.md",
                ],
            ),
        ];
        let home = TempDir::new().unwrap();
        for (_, paths) in reads {
            for path in *paths {
                let path = home.path().join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, "x").unwrap();
            }
        }
        let agents = Agents::built_in();
        for (name, _) in reads {
            let profile = &agents
                .listed()
                .iter()
                .find(|l| l.profile.name == *name)
                .unwrap()
                .profile;
            assert_eq!(
                profile.unprotected(home.path()),
                Vec::<String>::new(),
                "{name}"
            );
        }
    }

    /// A profile sends a credential Kule knows to its own service alone,
    /// and the person sees every credential a profile takes, and where it
    /// goes, before approving it.
    #[test]
    fn a_profile_s_credentials_go_where_they_belong_and_are_told() {
        let base = || Profile::bare("tool", vec!["tool".into()], BTreeMap::new());
        let mut wrong = base();
        wrong
            .keys
            .insert("ANTHROPIC_API_KEY".into(), vec!["evil.example".into()]);
        assert!(
            wrong
                .check(&[], None)
                .unwrap_err()
                .contains("api.anthropic.com alone")
        );
        let mut wrong = base();
        wrong.keychain.insert(
            "TOKEN".into(),
            KeychainKey {
                service: "Claude Code-credentials".into(),
                json: Some("claudeAiOauth.accessToken".into()),
                hosts: vec!["evil.example".into()],
            },
        );
        assert!(wrong.check(&[], None).is_err());
        let mut wrong = base();
        wrong.pass_env.push("GITHUB_TOKEN".into());
        assert!(wrong.check(&[], None).is_err());

        let home = TempDir::new().unwrap();
        let mut told = base();
        told.keys
            .insert("OPENROUTER_API_KEY".into(), vec!["openrouter.ai".into()]);
        told.pass_env.push("TOOL_CONFIG".into());
        assert!(told.check(&[], None).is_ok());
        let warnings = told.warnings(home.path());
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("OPENROUTER_API_KEY") && w.contains("openrouter.ai")),
            "{warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("TOOL_CONFIG")),
            "{warnings:?}"
        );
    }

    /// A package Kule ships a lockfile for starts as locked or not at all;
    /// any other falls back to npx when it cannot be installed.
    #[test]
    fn a_locked_package_that_cannot_install_does_not_start() {
        let home = TempDir::new().unwrap();
        // Nothing can be installed under this home.
        let first = Path::new(packages::PACKAGES).components().next().unwrap();
        std::fs::write(home.path().join(first), "").unwrap();
        let npx = |spec: &str| {
            Profile::bare(
                "tool",
                vec!["npx".into(), "-y".into(), spec.into(), "acp".into()],
                BTreeMap::new(),
            )
        };
        let locked = npx("@agentclientprotocol/claude-agent-acp@0.92.0");
        let refused = locked.started_as(Some(home.path())).unwrap_err();
        assert!(refused.to_string().contains("as locked"), "{refused}");
        let floating = npx("some-agent@1.2.3");
        let (program, _) = floating.started_as(Some(home.path())).unwrap();
        assert_eq!(program, "npx");
    }

    #[test]
    fn what_runs_later_and_is_writable_is_found_and_warned_about() {
        let home = TempDir::new().unwrap();
        let mut profile = Profile::bare("tool", vec!["tool".into()], BTreeMap::new());
        profile.state = vec![".tool".into()];
        let dir = home.path().join(".tool");
        std::fs::create_dir_all(dir.join("hooks")).unwrap();
        for name in [
            "settings.json",
            "TOOL.md",
            "notes.txt",
            "cache.json",
            "readme.md",
        ] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        assert_eq!(
            profile.unprotected(home.path()),
            [".tool/TOOL.md", ".tool/hooks", ".tool/settings.json"]
        );
        assert_eq!(profile.warnings(home.path()).len(), 3);
        profile.protected = vec![
            ".tool/hooks".into(),
            ".tool/settings.json".into(),
            ".tool/TOOL.md".into(),
        ];
        assert!(profile.unprotected(home.path()).is_empty());
        assert!(profile.warnings(home.path()).is_empty());

        // Nothing found, nothing read-only: still told.
        let mut bare = Profile::bare("other", vec!["other".into()], BTreeMap::new());
        bare.state = vec![".other".into()];
        assert_eq!(bare.warnings(home.path()).len(), 1);
        // A home of its own and no state in the person's: nothing to tell.
        bare.state.clear();
        bare.own_home = Some("OTHER_HOME".into());
        assert!(bare.warnings(home.path()).is_empty());
    }

    #[test]
    fn an_agent_with_a_home_of_its_own_writes_there_and_nobody_else_reads_it() {
        let (tasks, ws, person) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let task = Task::create(tasks.path(), ws.path()).unwrap();
        let person = std::fs::canonicalize(person.path()).unwrap();
        let sources = || {
            Some(CacheSources {
                home: person.clone(),
                cargo_home: person.join(".cargo"),
            })
        };
        let agents_file = r#"
[[agent]]
name = "tool"
command = ["sh", "-c", "echo secret > \"$TOOL_HOME/login\""]
own_home = "TOOL_HOME"
"#;
        let kule = kule_home(agents_file);
        Agents::approve(kule.path(), "tool").unwrap();
        let agents = Agents::load(kule.path()).unwrap();
        let tool = agents.get("tool").unwrap();
        let proxy = Proxy::start(&task, Policy::builtin()).unwrap();
        let mut launch = tool.launch_for(&task, &proxy, sources()).unwrap();
        assert!(launch.command.status().unwrap().success());
        let own = person.join(tool.own_home_path());
        assert_eq!(
            std::fs::read_to_string(own.join("login")).unwrap(),
            "secret\n"
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&own).unwrap().permissions().mode() & 0o777,
            0o700
        );

        // Another agent cannot read it, nor can a command that is none.
        let claude = agents.get("claude-code").unwrap();
        let path = format!("{}/login", tool.own_home_path());
        assert!(!reads(&task, &person, claude, &path));
        let mut sandbox = Sandbox::for_task_with(&task, sources()).unwrap();
        agents.hide_logins(&mut sandbox, &person).unwrap();
        let cat = sandbox
            .command("cat", &[own.join("login").display().to_string()])
            .unwrap()
            .output()
            .unwrap();
        assert!(!cat.status.success());
    }

    #[test]
    fn a_home_of_its_own_is_named_by_a_plain_variable() {
        for var in [
            "PATH",
            "TMPDIR",
            "tool_home",
            "HTTPS_PROXY",
            "LD_PRELOAD",
            "",
        ] {
            let mut profile = Profile::bare("tool", vec!["tool".into()], BTreeMap::new());
            profile.own_home = Some(var.into());
            assert!(profile.check(&[], None).is_err(), "{var}");
        }
        let mut profile = Profile::bare("tool", vec!["tool".into()], BTreeMap::new());
        profile.own_home = Some("HOME".into());
        assert_eq!(profile.check(&[], None), Ok(()));
        // And no profile reaches into the agents' own homes, nor into the
        // packages Kule starts them from.
        for state in [
            format!("{OWN_HOMES}/codex"),
            format!("{}/npm", packages::PACKAGES),
        ] {
            profile.state = vec![state.clone()];
            assert!(
                profile
                    .check(&[], None)
                    .unwrap_err()
                    .contains("Kule's own home"),
                "{state}"
            );
        }
    }
}
