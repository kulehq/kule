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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    #[serde(flatten)]
    info: SecretInfo,
    value: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    secrets: Vec<Stored>,
}

fn read_store(home: &Path) -> Result<Store> {
    let path = home.join(SECRETS_FILE);
    if !path.is_file() {
        return Ok(Store::default());
    }
    read_json(&path)
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
    check_value(value)?;
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
    write_private(&home.join(SECRETS_FILE), &store)?;
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
        write_private(&home.join(SECRETS_FILE), &store)?;
    }
    Ok(gone)
}

/// Every kept secret, without its value.
pub fn secrets(home: &Path) -> Result<Vec<SecretInfo>> {
    Ok(read_store(home)?
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

/// What can go into a request header as it is.
fn check_value(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 16 * 1024 || value.chars().any(|c| c.is_control()) {
        return Err(Error::Invalid(
            "a secret's value is one line of text, up to 16 KiB".into(),
        ));
    }
    Ok(())
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
/// hidden from the sandbox) so they stay the same from run to run.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(crate) struct TaskSecrets {
    /// Kept secret name → its placeholder in this task.
    #[serde(default)]
    pub placeholders: BTreeMap<String, String>,
    /// The values masked in the copy, by placeholder.
    #[serde(default)]
    pub masked: BTreeMap<String, Masked>,
}

/// A value Kule took out of a file in the copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Masked {
    pub path: String,
    pub key: String,
    pub value: String,
}

impl TaskSecrets {
    pub fn load(dir: &Path) -> Result<TaskSecrets> {
        let path = dir.join(SECRETS_FILE);
        if !path.is_file() {
            return Ok(TaskSecrets::default());
        }
        read_json(&path)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        write_private(&dir.join(SECRETS_FILE), self)
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
}

impl Vault {
    /// Holds an agent's `keys` too, each under a placeholder of its own for
    /// this run. A kept secret of the same name wins: the person said where
    /// that one goes.
    pub(crate) fn with_keys(&mut self, keys: &[AgentKey]) -> Result<()> {
        for key in keys {
            check_name(&key.name)?;
            check_value(&key.value)?;
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
            });
        }
        Ok(())
    }

    /// The secrets kept in `home` for `workspace`, and the values masked in
    /// the task's copy, with the placeholders of the task in `dir`.
    pub(crate) fn load(home: &Path, workspace: &Path, dir: &Path) -> Result<Vault> {
        let store = read_store(home)?;
        let mut task = TaskSecrets::load(dir)?;
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
            });
        }
        if task.placeholders.len() != before {
            task.save(dir)?;
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
            });
        }
        Ok(Vault { held })
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
    /// values, also inside `Authorization: Basic` credentials; and the names
    /// of the secrets that went in.
    pub fn fill(&self, host: &str, head: &str) -> (String, Vec<String>) {
        let going: Vec<&Held> = self
            .held
            .iter()
            .filter(|h| h.hosts.iter().any(|h| h == host))
            .collect();
        let mut used = BTreeSet::new();
        let mut out = String::with_capacity(head.len());
        for (i, line) in head.split_inclusive('\n').enumerate() {
            if i == 0 {
                // The request line: a value in its URL is percent-encoded.
                let (filled, names) = replace_bytes(&going, line.as_bytes(), Encoding::Url);
                used.extend(names);
                out.push_str(&String::from_utf8_lossy(&filled));
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

    /// A request body of `content_type` bound for `host`, with the
    /// placeholders of the secrets that may go there put back to their
    /// values, written as that kind of body needs (escaped in JSON,
    /// percent-encoded in a form); and the names of the secrets that went in.
    pub(crate) fn fill_body(
        &self,
        host: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> (Vec<u8>, Vec<String>) {
        let going: Vec<&Held> = self
            .held
            .iter()
            .filter(|h| h.hosts.iter().any(|h| h == host))
            .collect();
        replace_bytes(&going, body, Encoding::of_body(content_type))
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

    /// `text` with every placeholder of the task put back to its value,
    /// wherever it is bound: for a file going to the folder.
    pub(crate) fn unmask(&self, text: &str) -> String {
        let all: Vec<&Held> = self.held.iter().collect();
        replace_all(&all, text).0
    }
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
    /// For a request body of `content_type`.
    fn of_body(content_type: Option<&str>) -> Encoding {
        let kind = content_type.unwrap_or("").to_ascii_lowercase();
        let kind = kind.split(';').next().unwrap_or("").trim();
        if kind == "application/json" || kind.ends_with("+json") {
            Encoding::Json
        } else if kind == "application/x-www-form-urlencoded" {
            Encoding::Url
        } else {
            Encoding::Raw
        }
    }

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
/// `task` (new ones made as needed) and of `vault`'s kept secrets.
pub(crate) fn mask_with(
    path: &str,
    text: &str,
    task: &mut TaskSecrets,
    vault: &Vault,
) -> Result<String> {
    mask_env(text, |key, value| {
        let kept = vault.placeholder_for_value(value);
        if kept.is_none() && !looks_secret(key, value) {
            return Ok(None);
        }
        if value.starts_with(PLACEHOLDER_PREFIX) {
            return Ok(None);
        }
        if let Some(p) = kept {
            // Kept so commit can put it back like any other.
            task.masked.entry(p.to_owned()).or_insert_with(|| Masked {
                path: path.to_owned(),
                key: key.to_owned(),
                value: value.to_owned(),
            });
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
                    },
                );
                p
            }
        };
        Ok(Some(p))
    })
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
    fn a_secret_in_a_url_or_a_body_is_written_as_it_needs_there() {
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

        let (filled, used) = vault.fill(
            host,
            &format!("GET /v1?key={p} HTTP/1.1\r\nX-Key: {p}\r\n\r\n"),
        );
        assert_eq!(
            filled,
            "GET /v1?key=a%2Bb%2Fc%22d%5Ce HTTP/1.1\r\nX-Key: a+b/c\"d\\e\r\n\r\n"
        );
        assert_eq!(used, ["KEY"]);

        let body = |kind: Option<&str>, text: &str| {
            let (out, used) = vault.fill_body(host, kind, text.as_bytes());
            (String::from_utf8(out).unwrap(), used.len())
        };
        let json = format!("{{\"key\":\"{p}\",\"n\":1}}");
        assert_eq!(
            body(Some("application/json; charset=utf-8"), &json),
            ("{\"key\":\"a+b/c\\\"d\\\\e\",\"n\":1}".to_owned(), 1)
        );
        assert_eq!(
            body(
                Some("application/x-www-form-urlencoded"),
                &format!("key={p}&x=1")
            ),
            ("key=a%2Bb%2Fc%22d%5Ce&x=1".to_owned(), 1)
        );
        assert_eq!(body(Some("text/plain"), &p), ("a+b/c\"d\\e".to_owned(), 1));
        assert_eq!(body(None, "no secret"), ("no secret".to_owned(), 0));
        // Never for another host.
        let (out, used) = vault.fill_body("evil.example.com", None, p.as_bytes());
        assert_eq!((out, used.len()), (p.into_bytes(), 0));

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
        let mut task = TaskSecrets::load(dir.path()).unwrap();
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
        task.save(dir.path()).unwrap();

        let vault = Vault::load(home.path(), ws.path(), dir.path()).unwrap();
        assert_eq!(vault.unmask(&out), text);
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
}
