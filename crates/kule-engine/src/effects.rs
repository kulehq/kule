//! Effect sets: what the copy holds that the baseline does not.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fsops::same_bytes;
use crate::git::{self, GitSummary};
use crate::ignore;
use crate::manifest::{Baseline, Entry, Kind, Stamp, scan};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub path: String,
    pub change: Change,
    /// The entry's type now, or before it was deleted.
    pub kind: Kind,
    /// The workspace's ignore rules exclude the path: a commit leaves it in
    /// the copy unless asked to take it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignored: bool,
}

impl Effect {
    /// Whether the path is inside the repository's `.git` directory. Those
    /// effects are read through [`EffectSet::git`] rather than one by one.
    pub fn in_git(&self) -> bool {
        git::classify(&self.path).is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSet {
    /// Every changed path, `.git` included, sorted so that a parent always
    /// comes before its children.
    pub effects: Vec<Effect>,
    /// What the changes under `.git` mean; `None` when they mean nothing,
    /// such as an index refresh by `git status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitSummary>,
}

impl EffectSet {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn count(&self, change: Change) -> usize {
        self.effects.iter().filter(|e| e.change == change).count()
    }

    pub fn paths(&self) -> Vec<String> {
        self.effects.iter().map(|e| e.path.clone()).collect()
    }

    /// The effects outside `.git`.
    pub fn files(&self) -> impl Iterator<Item = &Effect> {
        self.effects.iter().filter(|e| !e.in_git())
    }

    /// The effects on paths the workspace ignores.
    pub fn ignored(&self) -> impl Iterator<Item = &Effect> {
        self.effects.iter().filter(|e| e.ignored)
    }

    /// A hash of the set, to tell whether what a person reviewed is still
    /// what a commit would take.
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        blake3::hash(&bytes).to_hex().to_string()
    }

    /// What a commit takes: everything, or everything but the ignored paths.
    pub fn to_commit(&self, include_ignored: bool) -> EffectSet {
        EffectSet {
            effects: self
                .effects
                .iter()
                .filter(|e| include_ignored || !e.ignored)
                .cloned()
                .collect(),
            git: self.git.clone(),
        }
    }
}

pub(crate) fn compute(copy: &Path, work: &Path, baseline: &Baseline) -> Result<EffectSet> {
    let now = scan(copy)?;
    let mut effects = Vec::new();
    for (path, entry) in &baseline.entries {
        match now.get(path) {
            None => effects.push(Effect {
                path: path.clone(),
                change: Change::Deleted,
                kind: entry.copy.kind,
                ignored: false,
            }),
            Some(stamp)
                if !entry.copy.unchanged(stamp) && differs(copy, work, path, entry, stamp)? =>
            {
                effects.push(Effect {
                    path: path.clone(),
                    change: Change::Modified,
                    kind: stamp.kind,
                    ignored: false,
                })
            }
            Some(_) => {}
        }
    }
    for (path, stamp) in &now {
        if !baseline.entries.contains_key(path) {
            effects.push(Effect {
                path: path.clone(),
                change: Change::Created,
                kind: stamp.kind,
                ignored: false,
            });
        }
    }
    effects.sort_by(|a, b| a.path.cmp(&b.path));
    ignore::mark(work, &mut effects)?;
    let git = git::summarize(copy, baseline.git.as_ref(), &effects)?;
    Ok(EffectSet { effects, git })
}

/// The copy's stamp moved; decides whether its type, mode or content did.
fn differs(copy: &Path, work: &Path, path: &str, entry: &Entry, now: &Stamp) -> Result<bool> {
    let before = &entry.copy;
    if before.kind != now.kind || before.mode != now.mode {
        return Ok(true);
    }
    match now.kind {
        Kind::Dir => Ok(false),
        Kind::Symlink => Ok(before.link != now.link),
        Kind::File if before.size != now.size => Ok(true),
        Kind::File => {
            // Same size: compare bytes with the workspace's file, which holds
            // the baseline content as long as nobody touched it.
            let work_file = work.join(path);
            match Stamp::of(&work_file)? {
                Some(w) if entry.work.unchanged(&w) => {
                    Ok(!same_bytes(&copy.join(path), &work_file)?)
                }
                _ => Ok(true),
            }
        }
    }
}
