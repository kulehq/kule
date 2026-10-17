//! What a person told Kule not to ask again when they approved it: a
//! connection to one host and port, or the changes to one file that a rule
//! puts to a person (a manifest, a CI file), on one folder or everywhere.
//!
//! A connection becomes a Cedar `permit` like any other; a file becomes a
//! narrow exception to the rules that ask about it, since a Cedar `forbid`
//! always wins over a `permit`. What is denied is never remembered, and
//! neither is anything under `.kule/`, where a repository keeps the rules
//! that tighten Kule's. Before an allowance is kept, [`impact`] says what it
//! would have let through so far: the past connections and commits it
//! covers.
//!
//! The list lives in Kule's home, which no sandbox can see.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::network::connections;
use crate::policy::Verdict;
use crate::task::Task;

pub const REMEMBERED_FILE: &str = "remembered.json";

/// The rules that ask about one file by its name or place, whose question a
/// person may answer once for good. Others (a mass delete, a history
/// rewrite) ask about what a change does, not where.
pub const REMEMBERABLE: &[&str] = &["manifests-and-lockfiles", "project-files", "automation"];

/// What is not asked about again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Allowance {
    /// Connections to `host` on `port`.
    Connect { host: String, port: u16 },
    /// Changes to the file at `path` (relative to the folder) that `rule`
    /// would put to a person.
    File { path: String, rule: String },
}

/// One allowance, where it holds and since when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remembered {
    pub id: u64,
    #[serde(flatten)]
    pub allowance: Allowance,
    /// The folder it holds for; `None` for every folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    pub added_unix: u64,
}

impl Remembered {
    /// Whether it holds for `workspace`.
    pub fn holds_for(&self, workspace: &Path) -> bool {
        self.workspace.as_deref().is_none_or(|w| w == workspace)
    }
}

/// What an allowance would have let through so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impact {
    /// Connections that waited for a person or were refused.
    pub connections: usize,
    /// Commits that changed the file, and the uncommitted changes to it now.
    pub changes: usize,
    /// The tasks they were in.
    pub tasks: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    next: u64,
    #[serde(default)]
    remembered: Vec<Remembered>,
}

fn read(home: &Path) -> Result<File> {
    let path = home.join(REMEMBERED_FILE);
    if !path.is_file() {
        return Ok(File::default());
    }
    read_json(&path)
}

/// Every allowance kept in `home`.
pub fn load(home: &Path) -> Result<Vec<Remembered>> {
    Ok(read(home)?.remembered)
}

/// Keeps `allowance` for `workspace` (or everywhere); the one already kept
/// when it is there.
pub fn remember(home: &Path, allowance: Allowance, workspace: Option<&Path>) -> Result<Remembered> {
    check(&allowance)?;
    let workspace = match workspace {
        Some(w) => Some(std::fs::canonicalize(w).at(w)?),
        None => None,
    };
    let mut file = read(home)?;
    if let Some(kept) = file
        .remembered
        .iter()
        .find(|r| r.allowance == allowance && r.workspace == workspace)
    {
        return Ok(kept.clone());
    }
    file.next += 1;
    let kept = Remembered {
        id: file.next,
        allowance,
        workspace,
        added_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    file.remembered.push(kept.clone());
    write_json(&home.join(REMEMBERED_FILE), &file)?;
    Ok(kept)
}

/// Lets allowance `id` go; whether there was one.
pub fn forget(home: &Path, id: u64) -> Result<bool> {
    let mut file = read(home)?;
    let before = file.remembered.len();
    file.remembered.retain(|r| r.id != id);
    let gone = file.remembered.len() != before;
    if gone {
        write_json(&home.join(REMEMBERED_FILE), &file)?;
    }
    Ok(gone)
}

/// What an allowance may be: a plain host name and a port, or a plain
/// relative path under one of the [`REMEMBERABLE`] rules, outside `.kule/`.
fn check(allowance: &Allowance) -> Result<()> {
    let refuse = |why: String| Err(Error::Invalid(why));
    match allowance {
        Allowance::Connect { host, port } => {
            let labels: Vec<&str> = host.split('.').collect();
            let plain = labels.len() >= 2
                && labels.iter().all(|l| {
                    !l.is_empty()
                        && l.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                });
            if !plain || *port == 0 {
                return refuse(format!(
                    "{host}:{port}: a lowercase host name and a port, without a wildcard"
                ));
            }
        }
        Allowance::File { path, rule } => {
            let relative = !path.is_empty()
                && Path::new(path)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)));
            if !relative {
                return refuse(format!("{path}: a path relative to the folder, without .."));
            }
            if Path::new(path).starts_with(".kule") {
                return refuse(format!(
                    "{path}: the repository's own rules are always put to a person"
                ));
            }
            if !REMEMBERABLE.contains(&rule.as_str()) {
                return refuse(format!(
                    "{rule}: only a rule that asks about a file by its name can be answered for good"
                ));
            }
        }
    }
    Ok(())
}

/// What `allowance` would have let through so far, in the tasks on
/// `workspace` (or on every folder) that `home` holds.
pub fn impact(home: &Path, allowance: &Allowance, workspace: Option<&Path>) -> Result<Impact> {
    let workspace = match workspace {
        Some(w) => Some(std::fs::canonicalize(w).at(w)?),
        None => None,
    };
    let mut impact = Impact::default();
    for info in Task::list(home)? {
        if workspace.as_ref().is_some_and(|w| *w != info.workspace) {
            continue;
        }
        let Ok(task) = Task::open(home, &info.id) else {
            continue;
        };
        let found = match allowance {
            Allowance::Connect { host, port } => connections(&task)?
                .iter()
                .filter(|c| c.host == *host && c.port == *port && c.verdict != Verdict::Allow)
                .count(),
            Allowance::File { path, .. } => {
                let committed = task
                    .checkpoints()?
                    .iter()
                    .filter(|c| c.effects.iter().any(|e| e.path == *path))
                    .count();
                let pending = task
                    .effects()
                    .map(|set| set.effects.iter().any(|e| e.path == *path))
                    .unwrap_or(false);
                committed + usize::from(pending)
            }
        };
        if found > 0 {
            match allowance {
                Allowance::Connect { .. } => impact.connections += found,
                Allowance::File { .. } => impact.changes += found,
            }
            impact.tasks += 1;
        }
    }
    Ok(impact)
}
