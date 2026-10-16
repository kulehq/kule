//! Steps: what each tool call of an agent changed in the copy.
//!
//! A transaction holds everything the copy changed since the last sync. An
//! agent gets there through many tool calls, and a person reviewing the work
//! wants to see which call did what. After each one, [`Task::step`] compares
//! the effect set with the one recorded at the step before and keeps what
//! moved in between. Only the stamps of changed paths are kept, so a step
//! costs one scan of the copy and no copy of any content.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, ErrorKind, Write};

use serde::{Deserialize, Serialize};

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

/// The copy's stamp for each changed path at the last step.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Mark {
    next_id: u32,
    stamps: BTreeMap<String, (Option<Stamp>, Kind)>,
}

impl Task {
    /// Records a step: what changed in the copy since the previous one.
    pub fn step(&self, label: &str) -> Result<Step> {
        let set = self.effects()?;
        let mut mark = self.load_mark()?;
        let mut now = BTreeMap::new();
        let mut changes = Vec::new();
        for effect in set.files() {
            let stamp = Stamp::of(&self.copy().join(&effect.path))?;
            let moved = mark
                .stamps
                .get(&effect.path)
                .is_none_or(|(before, _)| !same(before, &stamp));
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
        mark.next_id += 1;
        let step = Step {
            id: mark.next_id,
            label: label.to_owned(),
            changes,
        };
        mark.stamps = now;
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

fn same(a: &Option<Stamp>, b: &Option<Stamp>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.unchanged(b),
        (None, None) => true,
        _ => false,
    }
}
