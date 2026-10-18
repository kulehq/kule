//! Steps: what each tool call of an agent changed in the copy.
//!
//! A transaction holds everything the copy changed since the last sync. An
//! agent gets there through many tool calls, and a person reviewing the work
//! wants to see which call did what. After each one, [`Task::step`] compares
//! the effect set with the one recorded at the step before and keeps what
//! moved in between. Only the stamps of changed paths are kept, so a step
//! costs one scan of the copy and no copy of any content.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::effects::Change;
use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::manifest::{Kind, Stamp};
use crate::task::Task;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepChange {
    pub path: String,
    /// The change relative to the workspace, as the effect set has it now.
    /// `None` when the step undid an earlier change: the path is back to
    /// what the workspace holds.
    pub change: Option<Change>,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Counts up from 1 within the uncommitted transaction.
    pub id: u32,
    /// What the step was, such as a tool call's title.
    pub label: String,
    /// Files outside `.git` that changed during the step.
    pub changes: Vec<StepChange>,
}

/// The copy's stamp for each changed path at the last step, and the content
/// digest of the files whose stamp was too fresh to trust (see `clock`).
#[derive(Debug, Default, Serialize, Deserialize)]
struct Mark {
    next_id: u32,
    stamps: BTreeMap<String, (Option<Stamp>, Kind)>,
    #[serde(default)]
    digests: BTreeMap<String, String>,
    /// The bytes of files kept for going back, this transaction.
    #[serde(default)]
    kept_bytes: u64,
}

impl Task {
    /// Records a step: what changed in the copy since the previous one.
    pub fn step(&self, label: &str) -> Result<Step> {
        let mut mark = self.load_mark()?;
        let (changes, now, digests) = self.since_mark(&mark)?;
        mark.next_id += 1;
        let step = Step {
            id: mark.next_id,
            label: label.to_owned(),
            changes,
        };
        // Kept before the step is logged: a step in the log always has its
        // paths kept, or noted as not kept.
        crate::step_store::keep(self, &step, &mut mark.kept_bytes)?;
        mark.stamps = now;
        mark.digests = digests;
        write_json(&self.dir().join("steps-mark.json"), &mark)?;
        let path = self.dir().join("steps.jsonl");
        let mut log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .at(&path)?;
        let line = serde_json::to_string(&step).map_err(|source| Error::State {
            path: path.clone(),
            source,
        })?;
        writeln!(log, "{line}").at(&path)?;
        Ok(step)
    }

    /// Whether the copy changed since the last step.
    pub(crate) fn unstepped(&self) -> Result<bool> {
        Ok(!self.since_mark(&self.load_mark()?)?.0.is_empty())
    }

    /// What moved in the copy since `mark`, and the stamps and digests to
    /// mark now.
    #[allow(clippy::type_complexity)]
    fn since_mark(
        &self,
        mark: &Mark,
    ) -> Result<(
        Vec<StepChange>,
        BTreeMap<String, (Option<Stamp>, Kind)>,
        BTreeMap<String, String>,
    )> {
        let start = Clock::new(self.dir(), &self.copy())?.now()?;
        let set = self.effects()?;
        let mut now = BTreeMap::new();
        let mut digests = BTreeMap::new();
        let mut changes = Vec::new();
        for effect in set.files() {
            let file = self.copy().join(&effect.path);
            let stamp = Stamp::of(&file)?;
            let racy = matches!(&stamp, Some(s) if s.kind == Kind::File && s.ctime >= start);
            let recorded = mark.digests.get(&effect.path);
            let digest = if racy || recorded.is_some() {
                digest(&file)?
            } else {
                None
            };
            let moved = mark.stamps.get(&effect.path).is_none_or(|(before, _)| {
                !same(before, &stamp) || recorded.is_some_and(|d| digest.as_ref() != Some(d))
            });
            if racy && let Some(digest) = digest {
                digests.insert(effect.path.clone(), digest);
            }
            if moved {
                changes.push(StepChange {
                    path: effect.path.clone(),
                    change: Some(effect.change),
                    kind: effect.kind,
                });
            }
            now.insert(effect.path.clone(), (stamp, effect.kind));
        }
        for (path, (_, kind)) in &mark.stamps {
            if !now.contains_key(path) {
                changes.push(StepChange {
                    path: path.clone(),
                    change: None,
                    kind: *kind,
                });
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok((changes, now, digests))
    }

    /// The steps of the uncommitted transaction, oldest first.
    pub fn steps(&self) -> Result<Vec<Step>> {
        let path = self.dir().join("steps.jsonl");
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).at(&path),
        };
        let mut steps = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line.at(&path)?;
            steps.push(serde_json::from_str(&line).map_err(|source| Error::State {
                path: path.clone(),
                source,
            })?);
        }
        Ok(steps)
    }

    /// Starts the step history over: the copy and the workspace agree again.
    pub(crate) fn clear_steps(&self) -> Result<()> {
        for name in ["steps-mark.json", "steps.jsonl"] {
            let path = self.dir().join(name);
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e).at(&path),
                _ => {}
            }
        }
        let store = self.dir().join(crate::step_store::STORE);
        if store.exists() {
            crate::fsops::remove_dir_tree(&store)?;
        }
        Ok(())
    }

    fn load_mark(&self) -> Result<Mark> {
        let path = self.dir().join("steps-mark.json");
        if path.is_file() {
            read_json(&path)
        } else {
            Ok(Mark::default())
        }
    }
}

/// The BLAKE3 digest of the file at `path`, or `None` when it is gone.
fn digest(path: &Path) -> Result<Option<String>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(None);
        }
        Err(e) => return Err(e).at(path),
    };
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(file).at(path)?;
    Ok(Some(hasher.finalize().to_hex().to_string()))
}

fn same(a: &Option<Stamp>, b: &Option<Stamp>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.unchanged(b),
        (None, None) => true,
        _ => false,
    }
}
