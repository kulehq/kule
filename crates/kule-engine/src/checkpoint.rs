//! Checkpoints: what a commit replaced, kept so the commit can be undone.
//!
//! What a commit replaced is a snapshot in the task's own store, Lonca's
//! object store at `checkpoints/store.git` (see [`crate::snapshot`]), and
//! every commit and undo is an entry in the task's signed history (see
//! [`crate::history`]). Checkpoints from before M3 kept a plain `before/`
//! directory instead, which undo still reads.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use lonca_core::keys::KeyPair;
use serde::{Deserialize, Serialize};

use crate::effects::{Change, Effect};
use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, remove, write_json};
use crate::git::GitState;
use crate::history::{Entry, Event, History};
use crate::manifest::{Baseline, Stamp};
use crate::snapshot::{KeptFile, Snapshots, lonca};

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
    /// The snapshot of what the commit replaced, a commit in the task's
    /// store; `None` for a checkpoint with a `before/` directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<String>,
    /// Files the folder had changed too, merged before the commit: what it
    /// took from each side.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged: Vec<String>,
}

pub(crate) struct Store {
    root: PathBuf,
    /// The key that signs the task's history: one per Kule home.
    key: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf, key: PathBuf) -> Self {
        Self { root, key }
    }

    /// What the commit of `checkpoint` replaced at `path`, when it replaced
    /// a file there: bytes, mode and modification time.
    pub fn replaced_file(&self, checkpoint: &Checkpoint, path: &str) -> Result<Option<KeptFile>> {
        match &checkpoint.snapshot {
            Some(snapshot) if self.root.join("store.git").is_dir() => {
                self.snapshots()?.file(snapshot, path)
            }
            _ => Ok(None),
        }
    }

    fn snapshots(&self) -> Result<Snapshots> {
        Snapshots::open(&self.root.join("store.git"))
    }

    /// The workspace as it was before the commit, for the paths it replaced
    /// or deleted, laid out as a directory; paths the commit created are
    /// absent, which is what undo wants. [`Store::done_with`] removes it.
    pub fn before(&self, checkpoint: &Checkpoint) -> Result<PathBuf> {
        let before = self.dir(checkpoint.id).join("before");
        if let Some(snapshot) = &checkpoint.snapshot {
            remove(&before)?;
            self.snapshots()?.restore(snapshot, &before)?;
        }
        Ok(before)
    }

    /// Removes the directory [`Store::before`] laid out from a snapshot.
    pub fn done_with(&self, checkpoint: &Checkpoint) -> Result<()> {
        if checkpoint.snapshot.is_some() {
            remove(&self.dir(checkpoint.id).join("before"))?;
        }
        Ok(())
    }

    /// Forgets a checkpoint whose commit was put back, so it does not stand
    /// in undo's way as an interrupted one. Its snapshot stays in the store.
    pub fn discard(&self, checkpoint: &Checkpoint) -> Result<()> {
        remove(&self.dir(checkpoint.id))
    }

    /// Keeps the workspace's version of every path the commit is about to
    /// replace or delete, along with the times of their parent directories,
    /// and records the checkpoint as `Applying`.
    pub fn begin(&self, work: &Path, effects: &[Effect]) -> Result<Checkpoint> {
        fs::create_dir_all(&self.root).at(&self.root)?;
        let id = self.ids()?.last().map_or(1, |last| last + 1);
        let dir = self.dir(id);
        fs::create_dir_all(&dir).at(&dir)?;
        let all: Vec<String> = effects.iter().map(|e| e.path.clone()).collect();
        let replaced: Vec<String> = effects
            .iter()
            .filter(|e| e.change != Change::Created)
            .map(|e| e.path.clone())
            .collect();
        let snapshot = self.snapshots()?.save(id, work, &replaced, &all)?;
        let checkpoint = Checkpoint {
            id,
            state: State::Applying,
            effects: effects.to_vec(),
            after: BTreeMap::new(),
            git_after: None,
            snapshot: Some(snapshot),
            merged: Vec::new(),
        };
        self.save(&checkpoint)?;
        Ok(checkpoint)
    }

    /// Adds `event` to the task's signed history.
    pub fn record(&self, event: Event) -> Result<()> {
        let key = KeyPair::load_or_create(&self.key).map_err(|e| lonca(&self.key, e))?;
        let snapshots = self.snapshots()?;
        History::new(snapshots.store()).append(&key, event)?;
        Ok(())
    }

    /// The task's history, checked against the home's key.
    pub fn history(&self) -> Result<Vec<Entry>> {
        if !self.root.join("store.git").is_dir() {
            return Ok(Vec::new());
        }
        let key = KeyPair::load_or_create(&self.key).map_err(|e| lonca(&self.key, e))?;
        let snapshots = self.snapshots()?;
        let entries = History::new(snapshots.store()).read(&key.public_hex())?;
        Ok(entries.into_iter().map(|(entry, _)| entry).collect())
    }

    pub fn save(&self, checkpoint: &Checkpoint) -> Result<()> {
        write_json(&self.dir(checkpoint.id).join("checkpoint.json"), checkpoint)
    }

    /// Every checkpoint, oldest first.
    pub fn all(&self) -> Result<Vec<Checkpoint>> {
        self.ids()?
            .into_iter()
            .map(|id| read_json(&self.dir(id).join("checkpoint.json")))
            .collect()
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
                         (its previous content is kept in checkpoint {id} under {})",
                        self.dir(id).display()
                    )));
                }
            }
        }
        Ok(None)
    }

    /// After checkpoint `undone` was reverted, `paths` in the workspace hold
    /// what earlier commits left there again, but as new files with new
    /// stamps, which `synced` holds. Updates the newest earlier applied
    /// checkpoint that touched each path, so undoing that one later does not
    /// mistake them for outside edits.
    pub fn refresh_after(&self, undone: u32, paths: &[String], synced: &Baseline) -> Result<()> {
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
                match synced.entries.get(path) {
                    Some(entry) => checkpoint.after.insert(path.clone(), entry.work.clone()),
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
