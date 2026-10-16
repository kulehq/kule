//! Checkpoints: what a commit replaced, kept so the commit can be undone.
//!
//! M0 keeps them as plain directories inside the task. This module is the
//! seam where Lonca's store takes over in M3.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::effects::{Change, Effect};
use crate::error::{Error, IoContext, Result};
use crate::fsops::{copy_parent_times, mirror, read_json, write_json};
use crate::git::GitState;
use crate::manifest::{Kind, Stamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Written before the commit touches the workspace; still there means the
    /// commit was interrupted.
    Applying,
    Applied,
    Undone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: u32,
    pub state: State,
    pub effects: Vec<Effect>,
    /// The workspace's stamps right after the commit, for the paths it left in
    /// place. Undo refuses to run when they no longer match.
    pub after: BTreeMap<String, Stamp>,
    /// The workspace repository's state right after the commit; undo checks
    /// `.git` refs and index against it instead of against stamps.
    #[serde(default)]
    pub git_after: Option<GitState>,
}

pub(crate) struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The workspace as it was before the commit, for the paths it replaced or
    /// deleted. Paths the commit created are absent, which is what undo wants.
    pub fn before(&self, id: u32) -> PathBuf {
        self.dir(id).join("before")
    }

    /// Saves the workspace's version of every path the commit is about to
    /// replace or delete, along with the times of their parent directories,
    /// and records the checkpoint as `Applying`.
    pub fn begin(&self, work: &Path, effects: &[Effect]) -> Result<Checkpoint> {
        fs::create_dir_all(&self.root).at(&self.root)?;
        let id = self.ids()?.last().map_or(1, |last| last + 1);
        let before = self.before(id);
        fs::create_dir_all(&before).at(&before)?;
        let all: Vec<String> = effects.iter().map(|e| e.path.clone()).collect();
        let replaced: Vec<String> = effects
            .iter()
            .filter(|e| e.change != Change::Created)
            .map(|e| e.path.clone())
            .collect();
        for path in &all {
            if let Some(parent) = Path::new(path).parent()
                && Stamp::of(&work.join(parent))?.is_some_and(|s| s.kind == Kind::Dir)
            {
                let dir = before.join(parent);
                fs::create_dir_all(&dir).at(&dir)?;
            }
        }
        mirror(work, &before, &replaced, false)?;
        copy_parent_times(work, &before, &all)?;
        let checkpoint = Checkpoint {
            id,
            state: State::Applying,
            effects: effects.to_vec(),
            after: BTreeMap::new(),
            git_after: None,
        };
        self.save(&checkpoint)?;
        Ok(checkpoint)
    }

    pub fn save(&self, checkpoint: &Checkpoint) -> Result<()> {
        write_json(&self.dir(checkpoint.id).join("checkpoint.json"), checkpoint)
    }

    /// The newest checkpoint that is still applied, skipping undone ones, so
    /// repeated undo walks back one commit at a time.
    pub fn last_applied(&self) -> Result<Option<Checkpoint>> {
        for id in self.ids()?.into_iter().rev() {
            let checkpoint: Checkpoint = read_json(&self.dir(id).join("checkpoint.json"))?;
            match checkpoint.state {
                State::Applied => return Ok(Some(checkpoint)),
                State::Undone => continue,
                State::Applying => {
                    return Err(Error::Invalid(format!(
                        "commit {id} was interrupted; the workspace may hold part of it \
                         (its previous content is in {})",
                        self.before(id).display()
                    )));
                }
            }
        }
        Ok(None)
    }

    /// After checkpoint `undone` was reverted, `paths` in the workspace hold
    /// what earlier commits left there again, but as new files with new
    /// stamps. Updates the newest earlier applied checkpoint that touched each
    /// path, so undoing that one later does not mistake them for outside edits.
    pub fn refresh_after(&self, undone: u32, paths: &[String], work: &Path) -> Result<()> {
        let mut pending: Vec<&String> = paths.iter().collect();
        for id in self.ids()?.into_iter().rev().filter(|id| *id < undone) {
            if pending.is_empty() {
                break;
            }
            let mut checkpoint: Checkpoint = read_json(&self.dir(id).join("checkpoint.json"))?;
            if checkpoint.state != State::Applied {
                continue;
            }
            let mut changed = false;
            let mut rest = Vec::new();
            for path in pending {
                if !checkpoint.effects.iter().any(|e| &e.path == path) {
                    rest.push(path);
                    continue;
                }
                match Stamp::of(&work.join(path))? {
                    Some(stamp) => checkpoint.after.insert(path.clone(), stamp),
                    None => checkpoint.after.remove(path),
                };
                changed = true;
            }
            pending = rest;
            if changed {
                self.save(&checkpoint)?;
            }
        }
        Ok(())
    }

    fn dir(&self, id: u32) -> PathBuf {
        self.root.join(format!("{id:06}"))
    }

    fn ids(&self) -> Result<Vec<u32>> {
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).at(&self.root),
        };
        let mut ids = Vec::new();
        for entry in entries {
            let entry = entry.at(&self.root)?;
            if let Some(id) = entry.file_name().to_str().and_then(|n| n.parse().ok()) {
                ids.push(id);
            }
        }
        ids.sort_unstable();
        Ok(ids)
    }
}
