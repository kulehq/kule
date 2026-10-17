//! The ACP agent registry: the agents that speak ACP and how each one is
//! started, as the protocol's maintainers publish it. Kule reads it to
//! offer a draft profile, never to run anything by itself: a draft still
//! needs the person to save and approve it (see `profile`).
//!
//! What the registry says is not trusted: a field too long is cut, an id
//! that is not a plain name is skipped, a link that is not http(s) is
//! dropped, and the file is read only up to a size.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use kule_engine::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::profile::Profile;

pub const REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
/// Larger answers are refused.
const MAX_REGISTRY: u64 = 4 << 20;
/// The last registry read, in Kule's home, for when the network is not there.
const CACHE: &str = "registry.json";
const MAX_TEXT: usize = 300;

/// One agent of the registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// An http(s) link, or nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub install: Install,
}

/// How the registry says to start the agent on this system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Install {
    /// Fetched and run by npx or uvx, at one version.
    Package {
        command: Vec<String>,
        env: BTreeMap<String, String>,
    },
    /// A release archive to download and unpack first; Kule leaves that to
    /// the person, and runs `cmd` once it is on their PATH.
    Download {
        archive: String,
        sha256: String,
        cmd: String,
        args: Vec<String>,
    },
    /// Nothing for this system.
    Unavailable,
}

impl Entry {
    /// A profile to start from: the agent's command, and nothing opened
    /// yet. What it needs is found by trying it (see `trial`).
    pub fn draft(&self) -> Option<Profile> {
        match &self.install {
            Install::Package { command, env } => {
                Some(Profile::bare(&self.id, command.clone(), env.clone()))
            }
            Install::Download { cmd, args, .. } => {
                let program = Path::new(cmd).file_name()?.to_str()?.to_owned();
                let command = std::iter::once(program)
                    .chain(args.iter().cloned())
                    .collect();
                Some(Profile::bare(&self.id, command, BTreeMap::new()))
            }
            Install::Unavailable => None,
        }
    }
}

/// The registry, fresh from the network or, failing that, as last read;
/// with whether it is fresh.
pub fn load(kule_home: &Path) -> Result<(Vec<Entry>, bool)> {
    let cache = kule_home.join(CACHE);
    match fetch() {
        Ok(bytes) => {
            let entries = parse(&bytes)?;
            let tmp = kule_home.join(format!(".{CACHE}.{}", std::process::id()));
            if std::fs::write(&tmp, &bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &cache);
            }
            Ok((entries, true))
        }
        Err(fetched) => match std::fs::read(&cache) {
            Ok(bytes) => Ok((parse(&bytes)?, false)),
            Err(_) => Err(fetched),
        },
    }
}

fn fetch() -> Result<Vec<u8>> {
    let failed = |e: &dyn std::fmt::Display| Error::Invalid(format!("the ACP registry: {e}"));
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .max_redirects(0)
        .https_only(true)
        .build()
        .new_agent();
    let mut response = agent.get(REGISTRY_URL).call().map_err(|e| failed(&e))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_REGISTRY + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| failed(&e))?;
    if bytes.len() as u64 > MAX_REGISTRY {
        return Err(failed(&"larger than Kule reads"));
    }
    Ok(bytes)
}

/// The entries of a registry file, the ones Kule cannot read left out.
pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>> {
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|e| Error::Invalid(format!("the ACP registry: {e}")))?;
    let agents = root
        .get("agents")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Invalid("the ACP registry has no agents".to_owned()))?;
    let mut entries: Vec<Entry> = agents.iter().filter_map(entry).collect();
    entries.sort_by_key(|e| e.name.to_lowercase());
    Ok(entries)
}

fn entry(agent: &Value) -> Option<Entry> {
    let text = |key: &str| agent.get(key).and_then(Value::as_str).map(cut);
    let id = text("id")?;
    let plain = !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !plain {
        return None;
    }
    let link = |key: &str| text(key).filter(|url| safe_url(url));
    Some(Entry {
        name: text("name").unwrap_or_else(|| id.clone()),
        version: text("version").unwrap_or_default(),
        description: text("description").unwrap_or_default(),
        license: text("license"),
        website: link("website"),
        repository: link("repository"),
        install: install(agent.get("distribution")?),
        id,
    })
}

fn install(distribution: &Value) -> Install {
    let strings = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).map(cut).collect())
            .unwrap_or_default()
    };
    let env = |value: Option<&Value>| -> BTreeMap<String, String> {
        value
            .and_then(Value::as_object)
            .map(|vars| {
                vars.iter()
                    .filter_map(|(k, v)| Some((cut(k), cut(v.as_str()?))))
                    .collect()
            })
            .unwrap_or_default()
    };
    for (launcher, flags) in [("npx", &["-y"][..]), ("uvx", &[][..])] {
        let Some(package) = distribution.get(launcher) else {
            continue;
        };
        let Some(name) = package.get("package").and_then(Value::as_str) else {
            continue;
        };
        let command = std::iter::once(launcher.to_owned())
            .chain(flags.iter().map(|f| (*f).to_owned()))
            .chain(std::iter::once(cut(name)))
            .chain(strings(package.get("args")))
            .collect();
        return Install::Package {
            command,
            env: env(package.get("env")),
        };
    }
    let platform = format!(
        "{}-{}",
        if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        },
        std::env::consts::ARCH
    );
    if let Some(binary) = distribution.get("binary").and_then(|b| b.get(&platform)) {
        let field = |key: &str| binary.get(key).and_then(Value::as_str).map(cut);
        if let (Some(archive), Some(sha256), Some(cmd)) =
            (field("archive"), field("sha256"), field("cmd"))
            && archive.starts_with("https://")
        {
            return Install::Download {
                archive,
                sha256,
                cmd,
                args: strings(binary.get("args")),
            };
        }
    }
    Install::Unavailable
}

/// One line of at most [`MAX_TEXT`] characters, without control characters.
fn cut(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

/// Only http(s) links are shown; `//host` and every other scheme are not.
fn safe_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = r#"{
      "version": "1.0.0",
      "agents": [
        {"id": "qwen-code", "name": "Qwen Code", "version": "0.25.2", "description": "Qwen's agent",
         "website": "https://qwenlm.github.io", "repository": "javascript:alert(1)",
         "distribution": {"npx": {"package": "@qwen-code/qwen-code@0.25.2", "args": ["--acp"]}}},
        {"id": "fast-agent", "name": "fast-agent", "version": "0.10.1", "description": "x",
         "distribution": {"uvx": {"package": "fast-agent-acp==0.10.1", "args": ["-x"], "env": {"FAST_AGENT_MODEL": "m"}}}},
        {"id": "goose", "name": "Goose", "version": "1.55.0", "description": "Block's agent",
         "distribution": {"binary": {
           "darwin-aarch64": {"archive": "https://github.com/block/goose/a.tar.bz2", "cmd": "./goose", "args": ["acp"], "sha256": "00"},
           "darwin-x86_64": {"archive": "https://github.com/block/goose/b.tar.bz2", "cmd": "./goose", "args": ["acp"], "sha256": "01"},
           "linux-aarch64": {"archive": "https://github.com/block/goose/c.tar.bz2", "cmd": "./goose", "args": ["acp"], "sha256": "02"},
           "linux-x86_64": {"archive": "https://github.com/block/goose/d.tar.bz2", "cmd": "./goose", "args": ["acp"], "sha256": "03"}}}},
        {"id": "../evil", "name": "Evil", "distribution": {"npx": {"package": "evil@1.0.0"}}},
        {"id": "nothing", "name": "Nothing", "distribution": {"binary": {"windows-x86_64": {"archive": "https://x/y.zip", "cmd": "y.exe", "sha256": "0"}}}}
      ]
    }"#;

    #[test]
    fn the_registry_is_read_as_far_as_it_can_be_trusted() {
        let entries = parse(REGISTRY.as_bytes()).unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["fast-agent", "goose", "nothing", "qwen-code"]);
        let qwen = &entries[3];
        assert_eq!(qwen.website.as_deref(), Some("https://qwenlm.github.io"));
        assert_eq!(qwen.repository, None);
        assert_eq!(
            qwen.install,
            Install::Package {
                command: vec![
                    "npx".into(),
                    "-y".into(),
                    "@qwen-code/qwen-code@0.25.2".into(),
                    "--acp".into()
                ],
                env: BTreeMap::new()
            }
        );
        assert!(matches!(entries[1].install, Install::Download { .. }));
        assert_eq!(entries[2].install, Install::Unavailable);
        assert_eq!(entries[2].draft(), None);
    }

    #[test]
    fn a_draft_starts_the_agent_and_opens_nothing() {
        let entries = parse(REGISTRY.as_bytes()).unwrap();
        let draft = entries[0].draft().unwrap();
        assert_eq!(draft.name, "fast-agent");
        assert_eq!(draft.command, ["uvx", "fast-agent-acp==0.10.1", "-x"]);
        assert_eq!(draft.env["FAST_AGENT_MODEL"], "m");
        assert!(draft.state.is_empty() && draft.hosts.is_empty());
        let goose = entries[1].draft().unwrap();
        assert_eq!(goose.command, ["goose", "acp"]);
    }

    #[test]
    fn long_text_and_control_characters_are_cut() {
        assert_eq!(cut("a\u{1b}[31mb\nc"), "a[31mbc");
        assert_eq!(cut(&"x".repeat(1000)).len(), MAX_TEXT);
    }
}
