//! Effect sets: what the copy holds that the baseline does not.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fsops::same_bytes;
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
}

/// Sorted by path, so a parent always comes before its children.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSet {
    pub effects: Vec<Effect>,
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
}

pub(crate) fn compute(copy: &Path, work: &Path, baseline: &Baseline) -> Result<EffectSet> {
    let now = scan(copy)?;
    let mut effects = Vec::new();
    for (path, entry) in baseline {
        match now.get(path) {
            None => effects.push(Effect {
                path: path.clone(),
                change: Change::Deleted,
                kind: entry.copy.kind,
            }),
            Some(stamp)
                if !entry.copy.unchanged(stamp) && differs(copy, work, path, entry, stamp)? =>
            {
                effects.push(Effect {
                    path: path.clone(),
                    change: Change::Modified,
                    kind: stamp.kind,
                })
            }
            Some(_) => {}
        }
    }
    for (path, stamp) in &now {
        if !baseline.contains_key(path) {
            effects.push(Effect {
                path: path.clone(),
                change: Change::Created,
                kind: stamp.kind,
            });
        }
    }
    effects.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(EffectSet { effects })
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
