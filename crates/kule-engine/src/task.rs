//! Tasks: a workspace, its copy-on-write copy, and the baseline between them.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::approvals;
use crate::checkpoint::{Checkpoint, State, Store};
use crate::clock::Clock;
use crate::effects::{self, Change, Effect, EffectSet};
use crate::error::{Error, IoContext, Result};
use crate::fsops::{self, read_json, write_json};
use crate::git::{self, GitState, Part};
use crate::history::{Entry as HistoryEntry, Event};
use crate::manifest::{Baseline, Entry, Kind, Stamp, Tree, relative, scan};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub workspace: PathBuf,
    pub created_unix: u64,
}

/// What a person set for the task.
#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    approval_timeout_secs: u64,
}

/// Layout under `<home>/tasks/<id>/`: `task.json`, `baseline.json`, `copy/`
/// and `checkpoints/`.
#[derive(Debug, Clone)]
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
        let clock = self.clock(&self.info.workspace)?;
        let taken = fsops::clone_tree(&self.info.workspace, &self.copy(), &clock)?;
        // The copy's stamps need no settling: its files carry the workspace's
        // modification times, which `clone_tree` has waited past when they
        // were recent, so the agent's first write moves them.
        //
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

    /// The task's own directory: state, checkpoints, logs and the copy.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
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

    /// The task's own temporary directory, created on first use. Commands
    /// get it as `TMPDIR`; nothing in it is part of the workspace.
    pub fn tmp(&self) -> Result<PathBuf> {
        let tmp = self.dir.join("tmp");
        fs::create_dir_all(&tmp).at(&tmp)?;
        Ok(tmp)
    }

    /// The uncommitted changes in the copy.
    pub fn effects(&self) -> Result<EffectSet> {
        effects::compute(&self.copy(), self.workspace(), &self.load_baseline()?)
    }

    /// Applies the copy's changes to the workspace, after saving what they
    /// replace in a checkpoint. Refuses with [`Error::Conflict`] when the
    /// workspace changed under any of the affected paths since the last sync.
    /// No policy is consulted; see [`Task::commit_if`]. Paths the workspace
    /// ignores stay in the copy; see [`Task::commit_with`].
    pub fn commit(&self) -> Result<Option<Checkpoint>> {
        self.commit_if(|_| Ok(()))
    }

    /// [`Task::commit`] behind a gate: `check` sees the exact effect set
    /// about to be applied, under the task's lock, and can refuse it. Nothing
    /// can slip in between the check and the commit.
    pub fn commit_if(
        &self,
        check: impl FnOnce(&EffectSet) -> Result<()>,
    ) -> Result<Option<Checkpoint>> {
        self.commit_with(false, check)
    }

    /// [`Task::commit_if`], taking the paths the workspace ignores too when
    /// `include_ignored` is set.
    pub fn commit_with(
        &self,
        include_ignored: bool,
        check: impl FnOnce(&EffectSet) -> Result<()>,
    ) -> Result<Option<Checkpoint>> {
        let _lock = self.lock()?;
        let mut baseline = self.load_baseline()?;
        let set =
            effects::compute(&self.copy(), self.workspace(), &baseline)?.to_commit(include_ignored);
        if set.is_empty() {
            return Ok(None);
        }
        check(&set)?;
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
        self.resync(&mut baseline, &paths)?;
        for path in &paths {
            if let Some(entry) = baseline.entries.get(path) {
                checkpoint.after.insert(path.clone(), entry.work.clone());
            }
        }
        checkpoint.git_after = baseline.git.clone();
        checkpoint.state = State::Applied;
        store.save(&checkpoint)?;
        store.record(Event::Commit {
            checkpoint: checkpoint.id,
            changes: checkpoint.effects.len(),
            snapshot: checkpoint.snapshot.clone().unwrap_or_default(),
            effects: lonca_core::canonical::digest_of(&checkpoint.effects),
        })?;
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
    /// changes, ignored paths aside, and the workspace must still look as
    /// the commit left it.
    pub fn undo(&self) -> Result<Option<Checkpoint>> {
        let _lock = self.lock()?;
        let mut baseline = self.load_baseline()?;
        let pending = effects::compute(&self.copy(), self.workspace(), &baseline)?;
        if !pending.to_commit(false).is_empty() {
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
        // Undo brings these paths back into the copy too, which would throw
        // away the copy's own uncommitted change to an ignored one.
        let under = |p: &str, q: &str| p == q || p.starts_with(&format!("{q}/"));
        if let Some(clash) = pending
            .ignored()
            .find(|e| paths.iter().any(|p| under(&e.path, p) || under(p, &e.path)))
        {
            return Err(Error::Invalid(format!(
                "the task has an uncommitted change to {}, which undo would overwrite; \
                 commit it or roll it back first",
                clash.path
            )));
        }

        let before = store.before(&checkpoint)?;
        let undone = self.revert(&checkpoint, &reverted, &paths, &before, &mut baseline);
        store.done_with(&checkpoint)?;
        undone?;
        checkpoint.state = State::Undone;
        store.save(&checkpoint)?;
        store.refresh_after(checkpoint.id, &paths, &baseline)?;
        store.record(Event::Undo {
            checkpoint: checkpoint.id,
        })?;
        Ok(Some(checkpoint))
    }

    /// Puts `before` back at `paths`, in the workspace and the copy, unless
    /// the workspace no longer looks as `checkpoint` left it.
    fn revert(
        &self,
        checkpoint: &Checkpoint,
        reverted: &[&Effect],
        paths: &[String],
        before: &Path,
        baseline: &mut Baseline,
    ) -> Result<()> {
        let work_git = git::read(self.workspace())?;
        let mut conflicts = Vec::new();
        for effect in reverted {
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
                && Stamp::of(&before.join(path))?.is_none_or(|s| s.kind != Kind::Dir);
            if !untouched || (removes_dir && self.has_unknown_under(path, baseline)?) {
                conflicts.push(path.clone());
            }
        }
        conflicts.sort();
        if !conflicts.is_empty() {
            return Err(Error::Conflict(conflicts));
        }
        fsops::mirror(before, self.workspace(), paths, true)?;
        fsops::mirror(self.workspace(), &self.copy(), paths, false)?;
        self.resync(baseline, paths)
    }

    /// How long the task's connections wait for a person before they are
    /// refused: [`approvals::DEFAULT_TIMEOUT`] unless a person set it. The
    /// setting lives outside the copy, where nothing the task runs can
    /// write.
    pub fn approval_timeout(&self) -> Result<Duration> {
        let path = self.dir.join("settings.json");
        if !path.is_file() {
            return Ok(approvals::DEFAULT_TIMEOUT);
        }
        let settings: Settings = read_json(&path)?;
        Ok(Duration::from_secs(settings.approval_timeout_secs))
    }

    /// Sets [`Task::approval_timeout`]; it can be anywhere from
    /// [`approvals::MIN_TIMEOUT`] to [`approvals::MAX_TIMEOUT`] but never off.
    pub fn set_approval_timeout(&self, timeout: Duration) -> Result<()> {
        if !(approvals::MIN_TIMEOUT..=approvals::MAX_TIMEOUT).contains(&timeout) {
            return Err(Error::Invalid(format!(
                "an approval can wait from {} minute to {} hours, not {} seconds",
                approvals::MIN_TIMEOUT.as_secs() / 60,
                approvals::MAX_TIMEOUT.as_secs() / 3600,
                timeout.as_secs()
            )));
        }
        let settings = Settings {
            approval_timeout_secs: timeout.as_secs(),
        };
        write_json(&self.dir.join("settings.json"), &settings)
    }

    /// The task's signed history, oldest first: every commit and undo.
    /// Fails when the chain was changed after it was signed.
    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        self.store().history()
    }

    /// The key that signs the history of every task in this home.
    pub(crate) fn key_path(&self) -> PathBuf {
        self.home().join("kule.key")
    }

    /// Kule's home: the task lives at `<home>/tasks/<id>`.
    pub(crate) fn home(&self) -> PathBuf {
        self.dir
            .parent()
            .and_then(Path::parent)
            .map_or_else(|| self.dir.clone(), Path::to_path_buf)
    }

    /// The task's commits, oldest first, undone ones included.
    pub fn checkpoints(&self) -> Result<Vec<Checkpoint>> {
        self.store().all()
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
    ///
    /// A file written within the tick its stamp is read in can be written
    /// again unseen (see `clock`), so such files are compared once the clock
    /// has moved on. A pair that changed meanwhile keeps its old stamps,
    /// which no longer match; one that differs without a stamp moving gets
    /// stamps that match nothing.
    fn resync(&self, baseline: &mut Baseline, paths: &[String]) -> Result<()> {
        let work_clock = self.clock(self.workspace())?;
        let copy_clock = self.clock(&self.copy())?;
        let (work_start, copy_start) = (work_clock.now()?, copy_clock.now()?);
        let mut racy = Vec::new();
        for path in paths {
            let work = Stamp::of(&self.workspace().join(path))?;
            let copy = Stamp::of(&self.copy().join(path))?;
            match (work, copy) {
                (Some(work), Some(copy)) => {
                    if work.kind == Kind::File
                        && (work.ctime >= work_start || copy.ctime >= copy_start)
                    {
                        racy.push((
                            path.clone(),
                            Entry {
                                work: work.clone(),
                                copy: copy.clone(),
                            },
                        ));
                    }
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
        if !racy.is_empty() {
            let latest = |side: fn(&Entry) -> &Stamp| {
                racy.iter()
                    .map(|(_, e)| side(e).ctime)
                    .max()
                    .unwrap_or_default()
            };
            let past = work_clock.wait_past(latest(|e| &e.work))?
                && copy_clock.wait_past(latest(|e| &e.copy))?;
            for (path, entry) in racy {
                let (work, copy) = (self.workspace().join(&path), self.copy().join(&path));
                let held = || -> Result<bool> {
                    Ok(Stamp::of(&work)?.as_ref() == Some(&entry.work)
                        && Stamp::of(&copy)?.as_ref() == Some(&entry.copy))
                };
                if past && held()? && fsops::same_bytes(&work, &copy)? && held()? {
                    continue;
                }
                if held()? {
                    baseline.entries.insert(
                        path,
                        Entry {
                            work: entry.work.unknown(),
                            copy: entry.copy.unknown(),
                        },
                    );
                }
            }
        }
        baseline.git = git::read(self.workspace())?;
        self.save_baseline(baseline)?;
        self.clear_steps()
    }

    fn clock(&self, watched: &Path) -> Result<Clock> {
        Clock::new(&self.dir, watched)
    }

    fn store(&self) -> Store {
        Store::new(self.dir.join("checkpoints"), self.key_path())
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
