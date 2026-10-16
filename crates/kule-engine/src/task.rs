//! Tasks: a workspace, its copy-on-write copy, and the baseline between them.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::checkpoint::{Checkpoint, State, Store};
use crate::effects::{self, Change, Effect, EffectSet};
use crate::error::{Error, IoContext, Result};
use crate::fsops::{self, read_json, write_json};
use crate::git::{self, GitState, Part};
use crate::manifest::{Baseline, Entry, Kind, Stamp, Tree, relative, scan};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub workspace: PathBuf,
    pub created_unix: u64,
}

/// Layout under `<home>/tasks/<id>/`: `task.json`, `baseline.json`, `copy/`
/// and `checkpoints/`.
#[derive(Debug)]
pub struct Task {
    info: TaskInfo,
    dir: PathBuf,
}

impl Task {
    /// Copies `workspace` into a new task under `home`.
    pub fn create(home: &Path, workspace: &Path) -> Result<Task> {
        let workspace = fs::canonicalize(workspace).at(workspace)?;
        if !workspace.is_dir() {
            return Err(Error::Invalid(format!(
                "{} is not a directory",
                workspace.display()
            )));
        }
        fs::create_dir_all(home).at(home)?;
        let home = fs::canonicalize(home).at(home)?;
        if home.starts_with(&workspace) {
            return Err(Error::Invalid(format!(
                "Kule's home {} is inside the workspace, so the copy would contain itself",
                home.display()
            )));
        }
        let tasks = home.join("tasks");
        fs::create_dir_all(&tasks).at(&tasks)?;
        let (id, dir) = new_task_dir(&tasks)?;
        let created_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let task = Task {
            info: TaskInfo {
                id,
                workspace,
                created_unix,
            },
            dir,
        };
        match task.populate() {
            Ok(()) => Ok(task),
            Err(e) => {
                let _ = fsops::remove_dir_tree(&task.dir);
                Err(e)
            }
        }
    }

    fn populate(&self) -> Result<()> {
        write_json(&self.dir.join("task.json"), &self.info)?;
        let taken = fsops::clone_tree(&self.info.workspace, &self.copy())?;
        // The repository state is read from the copy: it is what the agent
        // starts from, so a human commit made during the copy must show up
        // as a change in the workspace, not be folded into the baseline.
        let baseline = Baseline {
            entries: pair(taken, scan(&self.copy())?)?,
            git: git::read(&self.copy())?,
        };
        self.save_baseline(&baseline)
    }

    pub fn open(home: &Path, id: &str) -> Result<Task> {
        let dir = home.join("tasks").join(id);
        if id.is_empty() || id.contains('/') || !dir.join("task.json").is_file() {
            return Err(Error::Invalid(format!(
                "no task {id} under {}",
                home.display()
            )));
        }
        let info = read_json(&dir.join("task.json"))?;
        Ok(Task { info, dir })
    }

    pub fn list(home: &Path) -> Result<Vec<TaskInfo>> {
        let tasks = home.join("tasks");
        let entries = match fs::read_dir(&tasks) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).at(&tasks),
        };
        let mut infos = Vec::new();
        for entry in entries {
            let file = entry.at(&tasks)?.path().join("task.json");
            if file.is_file() {
                infos.push(read_json::<TaskInfo>(&file)?);
            }
        }
        infos.sort_by_key(|i| (i.created_unix, i.id.clone()));
        Ok(infos)
    }

    pub fn info(&self) -> &TaskInfo {
        &self.info
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn workspace(&self) -> &Path {
        &self.info.workspace
    }

    /// Where commands run: the task's copy of the workspace.
    pub fn copy(&self) -> PathBuf {
        self.dir.join("copy")
    }

    /// The uncommitted changes in the copy.
    pub fn effects(&self) -> Result<EffectSet> {
        effects::compute(&self.copy(), self.workspace(), &self.load_baseline()?)
    }

    /// Applies the copy's changes to the workspace, after saving what they
    /// replace in a checkpoint. Refuses with [`Error::Conflict`] when the
    /// workspace changed under any of the affected paths since the last sync.
    pub fn commit(&self) -> Result<Option<Checkpoint>> {
        let _lock = self.lock()?;
        let mut baseline = self.load_baseline()?;
        let set = effects::compute(&self.copy(), self.workspace(), &baseline)?;
        if set.is_empty() {
            return Ok(None);
        }
        let work_git = git::read(self.workspace())?;
        let mut conflicts = Vec::new();
        for effect in &set.effects {
            let expected = baseline.entries.get(&effect.path).map(|e| &e.work);
            let untouched = self.untouched(
                &effect.path,
                effect.kind,
                expected,
                work_git.as_ref(),
                baseline.git.as_ref(),
            )?;
            let replaces_dir = effect.change == Change::Deleted || effect.kind != Kind::Dir;
            if !untouched || (replaces_dir && self.has_unknown_under(&effect.path, &baseline)?) {
                conflicts.push(effect.path.clone());
            }
        }
        if !conflicts.is_empty() {
            return Err(Error::Conflict(conflicts));
        }

        let store = self.store();
        let mut checkpoint = store.begin(self.workspace(), &set.effects)?;
        let paths = set.paths();
        fsops::mirror(&self.copy(), self.workspace(), &paths, true)?;
        for path in &paths {
            if let Some(stamp) = Stamp::of(&self.workspace().join(path))? {
                checkpoint.after.insert(path.clone(), stamp);
            }
        }
        checkpoint.git_after = git::read(self.workspace())?;
        checkpoint.state = State::Applied;
        store.save(&checkpoint)?;
        self.resync(&mut baseline, &paths)?;
        Ok(Some(checkpoint))
    }

    /// Throws the copy's changes away: each changed path goes back to what
    /// the workspace holds.
    pub fn rollback(&self) -> Result<EffectSet> {
        let _lock = self.lock()?;
        let mut baseline = self.load_baseline()?;
        let set = effects::compute(&self.copy(), self.workspace(), &baseline)?;
        if !set.is_empty() {
            let paths = set.paths();
            fsops::mirror(self.workspace(), &self.copy(), &paths, false)?;
            self.resync(&mut baseline, &paths)?;
        }
        Ok(set)
    }

    /// Reverts the newest applied commit in the workspace and the copy.
    /// Repeating it walks further back. The copy must have no uncommitted
    /// changes, and the workspace must still look as the commit left it.
    pub fn undo(&self) -> Result<Option<Checkpoint>> {
        let _lock = self.lock()?;
        let mut baseline = self.load_baseline()?;
        if !effects::compute(&self.copy(), self.workspace(), &baseline)?.is_empty() {
            return Err(Error::Invalid(
                "the task has uncommitted changes; commit or roll them back before undo".into(),
            ));
        }
        let store = self.store();
        let Some(mut checkpoint) = store.last_applied()? else {
            return Ok(None);
        };
        // Git objects the commit added stay: they are harmless, `git gc`
        // collects them, and a reference made since may already need them.
        let reverted: Vec<&Effect> = checkpoint
            .effects
            .iter()
            .filter(|e| {
                !(e.change == Change::Created && git::classify(&e.path) == Some(Part::Object))
            })
            .collect();
        let mut paths: Vec<String> = reverted.iter().map(|e| e.path.clone()).collect();
        paths.sort();

        let work_git = git::read(self.workspace())?;
        let mut conflicts = Vec::new();
        for effect in &reverted {
            let path = &effect.path;
            let untouched = self.untouched(
                path,
                effect.kind,
                checkpoint.after.get(path),
                work_git.as_ref(),
                checkpoint.git_after.as_ref(),
            )?;
            let now = Stamp::of(&self.workspace().join(path))?;
            let removes_dir = matches!(&now, Some(s) if s.kind == Kind::Dir)
                && Stamp::of(&store.before(checkpoint.id).join(path))?
                    .is_none_or(|s| s.kind != Kind::Dir);
            if !untouched || (removes_dir && self.has_unknown_under(path, &baseline)?) {
                conflicts.push(path.clone());
            }
        }
        conflicts.sort();
        if !conflicts.is_empty() {
            return Err(Error::Conflict(conflicts));
        }

        fsops::mirror(&store.before(checkpoint.id), self.workspace(), &paths, true)?;
        fsops::mirror(self.workspace(), &self.copy(), &paths, false)?;
        checkpoint.state = State::Undone;
        store.save(&checkpoint)?;
        store.refresh_after(checkpoint.id, &paths, self.workspace())?;
        self.resync(&mut baseline, &paths)?;
        Ok(Some(checkpoint))
    }

    /// Deletes the task and its copy. The workspace is not touched.
    pub fn remove(self) -> Result<()> {
        let _lock = self.lock()?;
        fsops::remove_dir_tree(&self.dir)
    }

    /// Whether the workspace has entries under `path` that the baseline does
    /// not know: replacing or deleting the directory would destroy them.
    fn has_unknown_under(&self, path: &str, baseline: &Baseline) -> Result<bool> {
        let root = self.workspace().join(path);
        if Stamp::of(&root)?.is_none_or(|s| s.kind != Kind::Dir) {
            return Ok(false);
        }
        for entry in WalkDir::new(&root).min_depth(1).follow_links(false) {
            let entry = entry.map_err(|e| Error::Io {
                path: e.path().unwrap_or(&root).to_path_buf(),
                source: e.into(),
            })?;
            if !baseline
                .entries
                .contains_key(&relative(self.workspace(), entry.path())?)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether the workspace still holds what Kule expects at `path`: the
    /// stamp `expected` (nothing when `None`), or, for the parts of `.git`
    /// whose files change without meaning anything, the repository state
    /// `expected_git`.
    fn untouched(
        &self,
        path: &str,
        kind: Kind,
        expected: Option<&Stamp>,
        now_git: Option<&GitState>,
        expected_git: Option<&GitState>,
    ) -> Result<bool> {
        match git::classify(path) {
            Some(Part::Object | Part::Derived) => return Ok(true),
            Some(Part::Index) => return Ok(git::index_untouched(now_git, expected_git)),
            Some(Part::Ref) if kind != Kind::Dir => {
                return Ok(git::refs_untouched(path, now_git, expected_git));
            }
            _ => {}
        }
        let now = Stamp::of(&self.workspace().join(path))?;
        Ok(match (expected, &now) {
            (Some(expected), Some(now)) => expected.unchanged(now),
            (None, None) => true,
            _ => false,
        })
    }

    /// Records `paths` as in sync again after the workspace and the copy were
    /// made equal there.
    fn resync(&self, baseline: &mut Baseline, paths: &[String]) -> Result<()> {
        for path in paths {
            let work = Stamp::of(&self.workspace().join(path))?;
            let copy = Stamp::of(&self.copy().join(path))?;
            match (work, copy) {
                (Some(work), Some(copy)) => {
                    baseline.entries.insert(path.clone(), Entry { work, copy });
                }
                (None, None) => {
                    baseline.entries.remove(path);
                }
                _ => {
                    return Err(Error::Invalid(format!(
                        "{path}: the workspace and the copy disagree right after syncing"
                    )));
                }
            }
        }
        baseline.git = git::read(self.workspace())?;
        self.save_baseline(baseline)
    }

    fn store(&self) -> Store {
        Store::new(self.dir.join("checkpoints"))
    }

    fn load_baseline(&self) -> Result<Baseline> {
        read_json(&self.dir.join("baseline.json"))
    }

    fn save_baseline(&self, baseline: &Baseline) -> Result<()> {
        write_json(&self.dir.join("baseline.json"), baseline)
    }

    fn lock(&self) -> Result<Lock> {
        let path = self.dir.join("lock");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let _ = writeln!(file, "{}", std::process::id());
                Ok(Lock(path))
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                Err(Error::Busy(self.info.id.clone()))
            }
            Err(e) => Err(e).at(&path),
        }
    }
}

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn new_task_dir(tasks: &Path) -> Result<(String, PathBuf)> {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    for attempt in 0..64u128 {
        let id = format!("{:08x}", (seed + attempt) as u32);
        let dir = tasks.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((id, dir)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).at(&dir),
        }
    }
    Err(Error::Invalid(format!(
        "could not find a free task id under {}",
        tasks.display()
    )))
}

/// Joins the workspace scan and the copy scan entry by entry; both must hold
/// exactly the same paths.
fn pair(work: Tree, copy: Tree) -> Result<BTreeMap<String, Entry>> {
    if let Some(path) = work
        .keys()
        .find(|p| !copy.contains_key(*p))
        .or_else(|| copy.keys().find(|p| !work.contains_key(*p)))
    {
        return Err(Error::Invalid(format!(
            "{path}: the copy does not match the workspace"
        )));
    }
    Ok(work
        .into_iter()
        .zip(copy.into_values())
        .map(|((path, work), copy)| (path, Entry { work, copy }))
        .collect())
}
