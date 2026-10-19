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
use crate::labels::{self, TaskColor};
use crate::limits::TaskLimits;
use crate::manifest::{Baseline, Entry, Kind, Stamp, Tree, relative, scan};
use crate::meet::{self, Touched};
use crate::outline::{self, FileOutline};
use crate::preview::{self, Preview};
use crate::secrets::{self, TaskSecrets, Vault};
use crate::usage::Money;

/// The largest settings file Kule masks secrets in.
const MAX_SETTINGS: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: String,
    pub workspace: PathBuf,
    pub created_unix: u64,
}

/// A file merged before a commit: what goes into the copy, and the folder's
/// stamp it was merged with.
struct Merge {
    path: String,
    bytes: Vec<u8>,
    theirs: Stamp,
}

/// What a person set for the task.
#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    #[serde(default = "default_timeout")]
    approval_timeout_secs: u64,
    /// What the task's runs may cost, all of them together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    budget: Option<Money>,
    /// The name a person gave the task, shown with its folder's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<TaskColor>,
    /// The group the person put the task in; its policy judges the task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<u64>,
    /// What the task's runs may take, where the task sets its own.
    #[serde(default, skip_serializing_if = "is_default")]
    limits: TaskLimits,
    /// What the copy, the temporary directory and the home changes took
    /// when the task was made: disk limits count from there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disk_base: Option<u64>,
}

fn is_default(limits: &TaskLimits) -> bool {
    *limits == TaskLimits::default()
}

fn default_timeout() -> u64 {
    approvals::DEFAULT_TIMEOUT.as_secs()
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

    /// A new task on the same folder whose copy is this one's: as it is
    /// now, or as it was after step `at` (`0`: before the first). It has
    /// this task's settings (label, colour, group, budget), the same
    /// stand-ins for the folder's secrets, the same last sync with the
    /// folder, and no steps or commits of its own yet. Changes since the
    /// last step are kept as a step here first when going back to a step.
    pub fn fork(&self, at: Option<u32>) -> Result<Task> {
        let staging = self.dir.join(crate::step_store::STORE).join("forking");
        let staged = at
            .map(|n| self.stage_step(n, &staging, "Changes before forking"))
            .transpose();
        let made = staged.and_then(|staged| {
            let (id, dir) = new_task_dir(&self.home().join("tasks"))?;
            let created_unix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let fork = Task {
                info: TaskInfo {
                    id,
                    workspace: self.info.workspace.clone(),
                    created_unix,
                },
                dir,
            };
            match fork.populate_from(self, staged.as_deref(), &staging) {
                Ok(()) => Ok(fork),
                Err(e) => {
                    let _ = fsops::remove_dir_tree(&fork.dir);
                    Err(e)
                }
            }
        });
        if staging.exists() {
            let _ = fsops::remove_dir_tree(&staging);
        }
        made
    }

    /// Makes this task a fork of `source` (see [`Task::fork`]); `staged`
    /// are the paths `staging` holds as they were at the step forked from.
    fn populate_from(
        &self,
        source: &Task,
        staged: Option<&[(String, bool)]>,
        staging: &Path,
    ) -> Result<()> {
        let _lock = source.lock()?;
        fsops::clone_tree(&source.copy(), &self.copy(), &self.clock(&source.copy())?)?;
        if source.base_dir().is_dir() {
            let clock = self.clock(&source.base_dir())?;
            fsops::clone_tree(&source.base_dir(), &self.base_dir(), &clock)?;
        }
        for name in [secrets::SECRETS_FILE, "settings.json"] {
            let from = source.dir.join(name);
            if from.is_file() {
                fs::copy(&from, self.dir.join(name)).at(&from)?;
            }
        }
        // The last sync is the source's; the copy's side of it is this
        // copy's own files wherever the source's copy still matched it.
        let mut baseline = source.load_baseline()?;
        let source_copy = source.copy();
        for (path, entry) in baseline.entries.iter_mut() {
            let theirs = Stamp::of(&source_copy.join(path))?;
            if theirs.is_some_and(|s| entry.copy.unchanged(&s))
                && let Some(mine) = Stamp::of(&self.copy().join(path))?
            {
                entry.copy = mine;
            }
        }
        if let Some(staged) = staged {
            let paths: Vec<String> = staged.iter().map(|(p, _)| p.clone()).collect();
            fsops::mirror(staging, &self.copy(), &paths, false)?;
            for (path, synced) in staged {
                if *synced
                    && let Some(entry) = baseline.entries.get_mut(path)
                    && let Some(mine) = Stamp::of(&self.copy().join(path))?
                {
                    entry.copy = mine;
                }
            }
        }
        self.save_baseline(&baseline)?;
        if self.mask_copy(&mut baseline)? {
            self.save_baseline(&baseline)?;
        }
        // Its own: the source's copy may have grown since it was made.
        self.mark_disk_base()?;
        write_json(&self.dir.join("task.json"), &self.info)
    }

    /// Writes `task.json` last: until then the task is not listed, so a
    /// half-made one never shows.
    fn populate(&self) -> Result<()> {
        let clock = self.clock(&self.info.workspace)?;
        let taken = fsops::clone_tree(&self.info.workspace, &self.copy(), &clock)?;
        // The copy's stamps need no settling: its files carry the workspace's
        // modification times, which `clone_tree` has waited past when they
        // were recent, so the agent's first write moves them.
        //
        // The repository state is read from the copy: it is what the agent
        // starts from, so a human commit made during the copy must show up
        // as a change in the workspace, not be folded into the baseline.
        let mut baseline = Baseline {
            entries: pair(taken, scan(&self.copy())?)?,
            git: git::read(&self.copy())?,
        };
        self.save_baseline(&baseline)?;
        // The base holds what the folder holds, secrets too: it is for
        // merging, outside the copy.
        fsops::keep_base(&self.copy(), &self.base_dir())?;
        if self.mask_copy(&mut baseline)? {
            self.save_baseline(&baseline)?;
        }
        self.mark_disk_base()?;
        write_json(&self.dir.join("task.json"), &self.info)
    }

    /// What the copy and the folder held at the last sync, for merging:
    /// hidden from the sandbox with the rest of the task.
    fn base_dir(&self) -> PathBuf {
        self.dir.join("base")
    }

    /// What the task's proxy holds: the person's secrets for this folder
    /// and the values masked in the copy, each with the task's placeholder.
    pub fn vault(&self) -> Result<Vault> {
        Vault::load(&self.home(), self.workspace(), &self.dir)
    }

    /// Masks the values that look like secrets in the copy's settings files
    /// (`.env` and the like), before an agent runs: see [`crate::secrets`].
    fn protect_secrets(&self) -> Result<()> {
        let _lock = self.lock()?;
        let mut baseline = self.load_baseline()?;
        if self.mask_copy(&mut baseline)? {
            self.save_baseline(&baseline)?;
        }
        Ok(())
    }

    /// Masks the copy's settings files the baseline knows; where the copy
    /// still held what it held at the last sync, the baseline takes the
    /// masked file as that, so masking shows as no change. Whether the
    /// baseline changed.
    fn mask_copy(&self, baseline: &mut Baseline) -> Result<bool> {
        let copy = self.copy();
        let candidates: Vec<String> = baseline
            .entries
            .iter()
            .filter(|(path, e)| e.copy.kind == Kind::File && secrets::is_env_file(path))
            .map(|(path, _)| path.clone())
            .collect();
        if candidates.is_empty() {
            return Ok(false);
        }
        let vault = self.vault()?;
        let mut kept = TaskSecrets::load(&self.dir)?;
        let mut writes = Vec::new();
        for path in candidates {
            let full = copy.join(&path);
            let Some(stamp) = Stamp::of(&full)?.filter(|s| s.kind == Kind::File) else {
                continue;
            };
            if stamp.size > MAX_SETTINGS {
                continue;
            }
            let Ok(text) = fs::read_to_string(&full) else {
                continue;
            };
            let masked = secrets::mask_with(&path, &text, &mut kept, &vault)?;
            if masked != text {
                let synced = baseline.entries.get(&path).is_some_and(|e| e.copy == stamp);
                writes.push((path, masked, synced));
            }
        }
        if writes.is_empty() {
            return Ok(false);
        }
        // The values first: a masked file whose values were lost could only
        // ever be committed as placeholders.
        kept.save(&self.dir)?;
        let mut latest = (i64::MIN, 0);
        let mut moved = Vec::new();
        for (path, masked, synced) in writes {
            let full = copy.join(&path);
            fsops::replace_file(&full, masked.as_bytes())?;
            if let (true, Some(now)) = (synced, Stamp::of(&full)?) {
                latest = latest.max(now.ctime);
                moved.push((path, now));
            }
        }
        // As in settle: a write within the same tick of the clock could
        // otherwise go unnoticed.
        let past = moved.is_empty() || self.clock(&copy)?.wait_past(latest)?;
        let changed = !moved.is_empty();
        for (path, now) in moved {
            if let Some(entry) = baseline.entries.get_mut(&path) {
                entry.copy = if past { now } else { now.unknown() };
            }
        }
        Ok(changed)
    }

    /// Puts the masked values back into the settings files among `paths`
    /// in the copy, for a commit to take them to the folder.
    fn unmask_copy(&self, paths: &[String]) -> Result<()> {
        let mut vault = None;
        for path in paths.iter().filter(|p| secrets::is_env_file(p)) {
            let full = self.copy().join(path);
            if Stamp::of(&full)?.is_none_or(|s| s.kind != Kind::File || s.size > MAX_SETTINGS) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&full) else {
                continue;
            };
            if vault.is_none() {
                vault = Some(self.vault()?);
            }
            let real = vault.as_ref().map(|v| v.unmask(&text)).unwrap_or_default();
            if real != text {
                fsops::replace_file(&full, real.as_bytes())?;
            }
        }
        Ok(())
    }

    pub fn open(home: &Path, id: &str) -> Result<Task> {
        let dir = home.join("tasks").join(id);
        if id.is_empty() || id.contains('/') || !dir.join("task.json").is_file() {
            // Without the home's path: the message reaches extensions too.
            return Err(Error::Invalid(format!("no task {id}")));
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

    /// What changed inside the files of `set`, as functions, sections, keys
    /// and the like, for the files Kule outlines (see [`crate::entities`]).
    pub fn outline(&self, set: &EffectSet) -> Result<Vec<FileOutline>> {
        outline::outline(&self.copy(), self.workspace(), &self.load_baseline()?, set)
    }

    /// What the picture at `path` (relative to the workspace) was at the
    /// last sync and is in the copy now; see [`crate::preview`].
    pub fn preview(&self, path: &str) -> Result<Preview> {
        if !preview::previewable(path) {
            return Err(Error::Invalid(format!(
                "{path} is not a picture Kule shows"
            )));
        }
        let baseline = self.load_baseline()?;
        let (before, after) = outline::sides(
            &self.copy(),
            self.workspace(),
            &baseline,
            path,
            preview::MAX_IMAGE,
        )?;
        Ok(Preview {
            path: path.to_owned(),
            before: preview::picture(before),
            after: preview::picture(after),
        })
    }

    /// What the work a commit would take changes, file by file and inside
    /// the files, to tell where it meets another task's (see [`crate::meet`]).
    pub fn touched(&self) -> Result<Touched> {
        let set = self.pending(false)?;
        let outlines = self.outline(&set)?;
        Ok(meet::touched(&set, &outlines))
    }

    /// What a commit would take now: [`Task::effects`] but for the paths the
    /// workspace ignores, unless `include_ignored`. A person reviews this
    /// set; its [`EffectSet::digest`] tells whether it is still the same.
    pub fn pending(&self, include_ignored: bool) -> Result<EffectSet> {
        self.commit_set(&self.load_baseline()?, include_ignored)
    }

    fn commit_set(&self, baseline: &Baseline, include_ignored: bool) -> Result<EffectSet> {
        let all = effects::compute(&self.copy(), self.workspace(), baseline)?;
        let mut set = all.to_commit(include_ignored);
        // A directory the commit removes (deletes, or replaces with a file)
        // takes the ignored paths under it with it: they go through the
        // conflict check and into the checkpoint like the rest, or undo
        // could not bring them back.
        let removed: Vec<String> = set
            .effects
            .iter()
            .filter(|e| {
                let was_dir = baseline
                    .entries
                    .get(&e.path)
                    .is_some_and(|entry| entry.work.kind == Kind::Dir);
                was_dir && (e.change == Change::Deleted || e.kind != Kind::Dir)
            })
            .map(|e| format!("{}/", e.path))
            .collect();
        if !include_ignored && !removed.is_empty() {
            set.effects.extend(
                all.ignored()
                    .filter(|e| removed.iter().any(|dir| e.path.starts_with(dir.as_str())))
                    .cloned(),
            );
            set.effects.sort_by(|a, b| a.path.cmp(&b.path));
        }
        set.read_contents(&self.copy())?;
        Ok(set)
    }

    /// After a commit's copy: the folder holds what the set's contents say
    /// for every path, but for files whose secrets the commit put back in
    /// and files merged with the folder's own changes, which hold `merged`.
    /// Anything else came in from somewhere other than what was reviewed.
    fn verify_committed(&self, set: &EffectSet, merged: &[(String, String)]) -> Result<()> {
        for (path, reviewed) in &set.contents {
            if secrets::is_env_file(path) {
                continue;
            }
            let wanted = merged
                .iter()
                .find(|(p, _)| p == path)
                .map_or(reviewed, |(_, hash)| hash);
            if fsops::content_hash(self.workspace(), path)? != *wanted {
                return Err(Error::Invalid(format!(
                    "{path} changed while it was being committed; nothing was committed"
                )));
            }
        }
        Ok(())
    }

    /// [`Task::commit_if`], taking the paths the workspace ignores too when
    /// `include_ignored` is set.
    pub fn commit_with(
        &self,
        include_ignored: bool,
        check: impl FnOnce(&EffectSet) -> Result<()>,
    ) -> Result<Option<Checkpoint>> {
        let _lock = self.lock()?;
        self.workspace_in_place()?;
        // Nothing left running in the task's sandbox changes the copy
        // while it is read and committed.
        if let Ok(sandbox) = crate::Sandbox::new(self) {
            sandbox.end_strays();
        }
        let mut baseline = self.load_baseline()?;
        let set = self.commit_set(&baseline, include_ignored)?;
        if set.is_empty() {
            return Ok(None);
        }
        check(&set)?;
        let work_git = git::read(self.workspace())?;
        let mut conflicts = Vec::new();
        let mut merges = Vec::new();
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
            if untouched && !(replaces_dir && self.has_unknown_under(&effect.path, &baseline)?) {
                continue;
            }
            // Changed in the folder too: merged, when Kule knows what both
            // started from and the two changes keep apart.
            match self.merge(effect, &baseline)? {
                Some(Ok(merge)) => merges.push(merge),
                Some(Err(both)) => conflicts.push(format!(
                    "{} (both changed {})",
                    effect.path,
                    both.join(", ")
                )),
                None => conflicts.push(effect.path.clone()),
            }
        }
        // A reference deleted in the copy is a loose file gone; in the
        // workspace it may live in packed-refs, which the commit would leave
        // standing, and the deletion would silently not happen.
        for change in set.git.iter().flat_map(|g| &g.refs) {
            if change.new.is_none() && git::is_packed(self.workspace(), &change.name)? {
                conflicts.push(format!(".git/{}", change.name));
            }
        }
        if !conflicts.is_empty() {
            return Err(Error::Conflict(conflicts));
        }

        // The merges go into the copy, which then starts from what the
        // folder holds now: the commit takes the merged files.
        for merge in &merges {
            fsops::replace_file(&self.copy().join(&merge.path), &merge.bytes)?;
            if let Some(entry) = baseline.entries.get_mut(&merge.path) {
                entry.work = merge.theirs.clone();
            }
        }

        let store = self.store();
        let mut checkpoint = store.begin(self.workspace(), &set.effects)?;
        checkpoint.merged = merges.iter().map(|m| m.path.clone()).collect();
        let paths = set.paths();
        // The folder gets the real values; the copy is masked again below.
        self.unmask_copy(&paths)?;
        let merged: Vec<(String, String)> = merges
            .iter()
            .map(|m| {
                (
                    m.path.clone(),
                    format!("file:{}", blake3::hash(&m.bytes).to_hex()),
                )
            })
            .collect();
        let placed = fsops::mirror(&self.copy(), self.workspace(), &paths, true)
            .and_then(|()| self.verify_committed(&set, &merged));
        if let Err(failed) = placed {
            let _ = self.mask_copy(&mut baseline);
            // Part of it may be in place: put back what the commit replaced
            // and forget the checkpoint, so nothing is left half done. Only
            // the paths it got to: rewriting the others would change their
            // stamps and make them look edited from outside.
            let touched: Vec<String> = paths
                .iter()
                .filter(|p| {
                    let now = Stamp::of(&self.workspace().join(p)).ok().flatten();
                    let was = baseline.entries.get(*p).map(|e| &e.work);
                    match (was, &now) {
                        (Some(was), Some(now)) => !was.unchanged(now),
                        (None, None) => false,
                        _ => true,
                    }
                })
                .cloned()
                .collect();
            let restored = store
                .before(&checkpoint)
                .and_then(|before| fsops::mirror(&before, self.workspace(), &touched, true));
            let _ = store.done_with(&checkpoint);
            if restored.is_ok() {
                // What was put back holds the baseline's content again, as
                // new files: their stamps go to the baseline and to the
                // commit before, as after an undo.
                for path in &touched {
                    if let (Some(now), Some(entry)) = (
                        Stamp::of(&self.workspace().join(path))?,
                        baseline.entries.get_mut(path),
                    ) {
                        entry.work = now;
                    }
                }
                self.save_baseline(&baseline)?;
                store.refresh_after(checkpoint.id, &touched, &baseline)?;
                store.discard(&checkpoint)?;
            }
            return Err(failed);
        }
        self.settle(&mut baseline, &paths)?;
        self.mask_copy(&mut baseline)?;
        for path in &paths {
            if let Some(entry) = baseline.entries.get(path) {
                checkpoint.after.insert(path.clone(), entry.work.clone());
            }
        }
        checkpoint.git_after = baseline.git.clone();
        checkpoint.state = State::Applied;
        // The checkpoint before the baseline: a crash in between leaves a
        // baseline older than the workspace, which reports conflicts rather
        // than overwriting anything.
        store.save(&checkpoint)?;
        self.save_baseline(&baseline)?;
        self.clear_steps()?;
        store.record(Event::Commit {
            checkpoint: checkpoint.id,
            changes: checkpoint.effects.len(),
            snapshot: checkpoint.snapshot.clone().unwrap_or_default(),
            effects: lonca_core::canonical::digest_of(&checkpoint.effects),
        })?;
        Ok(Some(checkpoint))
    }

    /// Merges what the copy changed in the file of `effect` with what the
    /// folder changed there since: `None` when that cannot be tried (not a
    /// changed file on both sides, or what both started from is unknown),
    /// otherwise the merge or what both changed.
    fn merge(
        &self,
        effect: &Effect,
        baseline: &Baseline,
    ) -> Result<Option<std::result::Result<Merge, Vec<String>>>> {
        if effect.change != Change::Modified || effect.kind != Kind::File || effect.in_git() {
            return Ok(None);
        }
        let Some(entry) = baseline
            .entries
            .get(&effect.path)
            .filter(|e| e.work.kind == Kind::File)
        else {
            return Ok(None);
        };
        let Some(theirs_stamp) =
            Stamp::of(&self.workspace().join(&effect.path))?.filter(|s| s.kind == Kind::File)
        else {
            return Ok(None);
        };
        let Some(base) = self.base_of(&effect.path, &entry.work)? else {
            return Ok(None);
        };
        let read =
            |root: &Path| -> Result<Option<Vec<u8>>> { outline::read_file(root, &effect.path) };
        let (Some(theirs), Some(mut ours)) = (read(self.workspace())?, read(&self.copy())?) else {
            return Ok(None);
        };
        // Merged with the real values: masked ones would read as changes to
        // every secret line.
        if secrets::is_env_file(&effect.path)
            && let Ok(text) = std::str::from_utf8(&ours)
        {
            ours = self.vault()?.unmask(text).into_bytes();
        }
        Ok(Some(
            match crate::entities::merge(&effect.path, &base, &theirs, &ours) {
                Ok(bytes) => Ok(Merge {
                    path: effect.path.clone(),
                    bytes,
                    theirs: theirs_stamp,
                }),
                Err(conflict) => Err(conflict.what),
            },
        ))
    }

    /// What the folder held at `path` at the last sync, as it stood with
    /// `expected`: in this task's base, or kept by the commit of another
    /// task on the same folder that replaced it since.
    fn base_of(&self, path: &str, expected: &Stamp) -> Result<Option<Vec<u8>>> {
        // A file that changed while the copy was made: what both started
        // from was never pinned down.
        if expected.is_unknown() {
            return Ok(None);
        }
        // Kept by this task since the last sync, whoever changed the folder;
        // a task from before bases were kept has none.
        if self.base_dir().is_dir()
            && let Some(bytes) = outline::read_file(&self.base_dir(), path)?
            && bytes.len() as u64 == expected.size
        {
            return Ok(Some(bytes));
        }
        let home = self.home();
        for info in Task::list(&home)? {
            if info.id == self.id() || info.workspace != self.info.workspace {
                continue;
            }
            // A task being removed meanwhile has nothing to give.
            let Ok(other) = Task::open(&home, &info.id) else {
                continue;
            };
            let store = other.store();
            for checkpoint in store.all().unwrap_or_default() {
                let Some(kept) = store.replaced_file(&checkpoint, path)? else {
                    continue;
                };
                if kept.mtime == expected.mtime
                    && kept.mode == expected.mode
                    && kept.bytes.len() as u64 == expected.size
                {
                    return Ok(Some(kept.bytes));
                }
            }
        }
        Ok(None)
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
            if self.mask_copy(&mut baseline)? {
                self.save_baseline(&baseline)?;
            }
        }
        Ok(set)
    }

    /// Puts `rel` at `to` as the copy held it at the last sync: from the
    /// base, or from the workspace where the base has no copy of it (too
    /// large) and the workspace has not changed it since. Nothing when it
    /// was not there.
    pub(crate) fn stage_synced(&self, rel: &str, to: &Path) -> Result<()> {
        let baseline = self.load_baseline()?;
        let Some(entry) = baseline.entries.get(rel) else {
            return Ok(());
        };
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).at(parent)?;
        }
        match entry.copy.kind {
            Kind::Dir => fs::create_dir_all(to).at(to),
            Kind::Symlink => {
                let target = entry.copy.link.clone().unwrap_or_default();
                std::os::unix::fs::symlink(target, to).at(to)
            }
            Kind::File => {
                let base = self.base_dir().join(rel);
                let from = if fs::symlink_metadata(&base).is_ok_and(|m| m.is_file()) {
                    base
                } else {
                    let work = self.workspace().join(rel);
                    match Stamp::of(&work)? {
                        Some(now) if now.unchanged(&entry.work) => work,
                        _ => {
                            return Err(Error::Invalid(format!(
                                "{rel} changed in the folder since the task last matched it, and the task kept no copy of it"
                            )));
                        }
                    }
                };
                reflink_copy::reflink_or_copy(&from, to)
                    .at(&from)
                    .map(|_| ())
            }
        }
    }

    /// Puts `paths` in the copy as `staging` holds them (absent there:
    /// gone), sorted parents first, and masks the settings files again.
    pub(crate) fn put_back(&self, staging: &Path, paths: &[String]) -> Result<()> {
        let _lock = self.lock()?;
        fsops::mirror(staging, &self.copy(), paths, false)?;
        let mut baseline = self.load_baseline()?;
        if self.mask_copy(&mut baseline)? {
            self.save_baseline(&baseline)?;
        }
        Ok(())
    }

    /// Reverts the newest applied commit in the workspace and the copy.
    /// Repeating it walks further back. The copy must have no uncommitted
    /// changes, ignored paths aside, and the workspace must still look as
    /// the commit left it.
    pub fn undo(&self) -> Result<Option<Checkpoint>> {
        let _lock = self.lock()?;
        self.workspace_in_place()?;
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
        self.mask_copy(&mut baseline)?;
        checkpoint.state = State::Undone;
        // As for commit: the checkpoint first, then the baseline.
        store.save(&checkpoint)?;
        store.refresh_after(checkpoint.id, &paths, &baseline)?;
        self.save_baseline(&baseline)?;
        self.clear_steps()?;
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
            // Only what this commit put there may go with the directory;
            // anything else (a person's file, even one a rollback made known
            // to the baseline) is not in `before` and would be lost.
            if !untouched || (removes_dir && self.has_other_than(path, paths)?) {
                conflicts.push(path.clone());
            }
        }
        conflicts.sort();
        if !conflicts.is_empty() {
            return Err(Error::Conflict(conflicts));
        }
        fsops::mirror(before, self.workspace(), paths, true)?;
        fsops::mirror(self.workspace(), &self.copy(), paths, false)?;
        self.settle(baseline, paths)
    }

    /// How long the task's connections wait for a person before they are
    /// refused: [`approvals::DEFAULT_TIMEOUT`] unless a person set it. The
    /// setting lives outside the copy, where nothing the task runs can
    /// write.
    pub fn approval_timeout(&self) -> Result<Duration> {
        Ok(Duration::from_secs(self.settings()?.approval_timeout_secs))
    }

    fn settings(&self) -> Result<Settings> {
        let path = self.dir.join("settings.json");
        if !path.is_file() {
            return Ok(Settings {
                approval_timeout_secs: default_timeout(),
                budget: None,
                label: None,
                color: None,
                group: None,
                limits: TaskLimits::default(),
                disk_base: None,
            });
        }
        read_json(&path)
    }

    /// What the task sets for itself of the limits on its runs; see
    /// [`Task::limits_in_force`].
    pub fn limits(&self) -> Result<TaskLimits> {
        Ok(self.settings()?.limits)
    }

    /// Sets [`Task::limits`], each value in its range.
    pub fn set_limits(&self, limits: &TaskLimits) -> Result<()> {
        limits.check()?;
        let mut settings = self.settings()?;
        settings.limits = *limits;
        write_json(&self.dir.join("settings.json"), &settings)
    }

    /// What the task took on disk when it was made, which its disk limit
    /// counts from; measured now for a task made before Kule kept it.
    pub fn disk_base(&self) -> Result<u64> {
        if let Some(base) = self.settings()?.disk_base {
            return Ok(base);
        }
        self.mark_disk_base()
    }

    fn mark_disk_base(&self) -> Result<u64> {
        let base = crate::resources::disk_used(&self.disk_dirs());
        let mut settings = self.settings()?;
        settings.disk_base = Some(base);
        write_json(&self.dir.join("settings.json"), &settings)?;
        Ok(base)
    }

    /// Where the task's runs write: the copy, the temporary directory and
    /// the home changes.
    pub fn disk_dirs(&self) -> Vec<PathBuf> {
        vec![self.copy(), self.dir.join("tmp"), self.dir.join("home")]
    }

    /// What the task's runs may cost, together; `None` for no limit.
    pub fn budget(&self) -> Result<Option<Money>> {
        Ok(self.settings()?.budget)
    }

    /// Sets [`Task::budget`]: a positive amount in an ISO 4217 currency, or
    /// none.
    pub fn set_budget(&self, budget: Option<Money>) -> Result<()> {
        if let Some(b) = &budget {
            b.check_budget()?;
        }
        let mut settings = self.settings()?;
        settings.budget = budget;
        write_json(&self.dir.join("settings.json"), &settings)
    }

    /// The name a person gave the task; `None` until they do.
    pub fn label(&self) -> Result<Option<String>> {
        Ok(self.settings()?.label)
    }

    /// Sets [`Task::label`], trimmed; an empty one clears it.
    pub fn set_label(&self, label: Option<&str>) -> Result<()> {
        let label = labels::clean_label(label)?;
        let mut settings = self.settings()?;
        settings.label = label;
        write_json(&self.dir.join("settings.json"), &settings)
    }

    /// The colour a person marked the task with.
    pub fn color(&self) -> Result<Option<TaskColor>> {
        Ok(self.settings()?.color)
    }

    pub fn set_color(&self, color: Option<TaskColor>) -> Result<()> {
        let mut settings = self.settings()?;
        settings.color = color;
        write_json(&self.dir.join("settings.json"), &settings)
    }

    /// The group the person put the task in, if any.
    pub fn group(&self) -> Result<Option<u64>> {
        Ok(self.settings()?.group)
    }

    /// Puts the task in group `group`, one under its home; `None` takes it
    /// out of its group. Its commits and runs are judged by that group's
    /// policy from then on.
    pub fn set_group(&self, group: Option<u64>) -> Result<()> {
        if let Some(id) = group
            && !crate::groups::exists(&self.home(), id)?
        {
            return Err(Error::Invalid(format!("no group {id}")));
        }
        let mut settings = self.settings()?;
        settings.group = group;
        write_json(&self.dir.join("settings.json"), &settings)
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
        let mut settings = self.settings()?;
        settings.approval_timeout_secs = timeout.as_secs();
        write_json(&self.dir.join("settings.json"), &settings)
    }

    /// The copy, for a person's own terminal, unless what the task ran
    /// changed what git runs there: config (`core.fsmonitor`, aliases) or
    /// hooks would run outside the sandbox the moment their shell prompt
    /// calls `git status`. Then this refuses, naming them, until they are
    /// committed or rolled back.
    pub fn terminal_dir(&self) -> Result<PathBuf> {
        let effects = self.effects()?;
        // A repository inside the copy, or a `.git` file pointing git at one
        // elsewhere: git in that folder reads what the task put there.
        let inner: Vec<&str> = effects
            .files()
            .filter(|e| e.change != Change::Deleted)
            .filter_map(|e| {
                let parts: Vec<&str> = e.path.split('/').collect();
                let at = parts.iter().position(|p| p.eq_ignore_ascii_case(".git"))?;
                Some(&e.path[..parts[..=at].join("/").len()])
            })
            .collect();
        if let Some(first) = inner.first() {
            return Err(Error::Invalid(format!(
                "the copy's {first} changed, and git there would run what it says outside the sandbox; \
                 review it, then commit or roll it back before opening a terminal"
            )));
        }
        if let Some(git) = effects.git {
            let mut runs: Vec<String> = git.hooks.iter().map(|h| format!("hook {h}")).collect();
            if git.config {
                runs.insert(0, "git config".to_owned());
            }
            if !runs.is_empty() {
                return Err(Error::Invalid(format!(
                    "the copy's {} changed, and git there would run it outside the sandbox; \
                     review it, then commit or roll it back before opening a terminal",
                    runs.join(", ")
                )));
            }
        }
        Ok(self.copy())
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

    /// Whether the workspace is still a directory where the task was made.
    pub fn workspace_exists(&self) -> bool {
        fs::symlink_metadata(self.workspace()).is_ok_and(|m| m.is_dir())
    }

    /// Refuses when the workspace was moved or deleted: a commit would
    /// otherwise build a new directory there out of the changes alone.
    fn workspace_in_place(&self) -> Result<()> {
        if self.workspace_exists() {
            return Ok(());
        }
        Err(Error::Invalid(format!(
            "the workspace {} is gone (moved or deleted outside Kule); nothing was applied, \
             and the task's changes are still in its copy",
            self.workspace().display()
        )))
    }

    /// Deletes the task and its copy. The workspace is not touched. The task
    /// is gone at once, moved to Kule's trash; [`Task::empty_trash`] takes
    /// its files off the disk, which can take a while for a large copy and
    /// its caches.
    pub fn remove(self) -> Result<()> {
        let _lock = self.lock()?;
        let trash = trash_dir(&self.home());
        fs::create_dir_all(&trash).at(&trash)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let to = trash.join(format!("{}-{nanos:x}", self.info.id));
        fs::rename(&self.dir, &to).at(&self.dir)
    }

    /// Deletes what removed tasks left in Kule's trash under `home`.
    pub fn empty_trash(home: &Path) -> Result<()> {
        let trash = trash_dir(home);
        let entries = match fs::read_dir(&trash) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).at(&trash),
        };
        for entry in entries {
            let path = entry.at(&trash)?.path();
            match fsops::remove_dir_tree(&path) {
                // Another emptying got there first.
                Err(Error::Io { source, .. }) if source.kind() == ErrorKind::NotFound => {}
                other => other?,
            }
        }
        Ok(())
    }

    /// Whether the workspace has entries under `path` that the baseline does
    /// not know: replacing or deleting the directory would destroy them.
    fn has_unknown_under(&self, path: &str, baseline: &Baseline) -> Result<bool> {
        self.any_under(path, |rel| !baseline.entries.contains_key(rel))
    }

    /// Whether the workspace has entries under `path` other than `paths`.
    fn has_other_than(&self, path: &str, paths: &[String]) -> Result<bool> {
        let listed: std::collections::BTreeSet<&str> = paths.iter().map(String::as_str).collect();
        self.any_under(path, |rel| !listed.contains(rel))
    }

    /// Whether any workspace entry under directory `path` (by its relative
    /// path) is `wanted`. Entries that vanish during the walk are skipped.
    fn any_under(&self, path: &str, wanted: impl Fn(&str) -> bool) -> Result<bool> {
        let root = self.workspace().join(path);
        if Stamp::of(&root)?.is_none_or(|s| s.kind != Kind::Dir) {
            return Ok(false);
        }
        for entry in WalkDir::new(&root).min_depth(1).follow_links(false) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) if crate::manifest::vanished(&e) => continue,
                Err(e) => {
                    return Err(Error::Io {
                        path: e.path().unwrap_or(&root).to_path_buf(),
                        source: e.into(),
                    });
                }
            };
            if wanted(&relative(self.workspace(), entry.path())?) {
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
        self.settle(baseline, paths)?;
        self.save_baseline(baseline)?;
        self.clear_steps()
    }

    /// [`Task::resync`] without saving: the baseline in memory only, so a
    /// commit or undo can record its checkpoint first.
    fn settle(&self, baseline: &mut Baseline, paths: &[String]) -> Result<()> {
        // The base follows: what the two hold now is what both start from.
        // A task from before bases were kept has none to follow.
        if self.base_dir().is_dir() {
            for path in paths {
                fsops::base_one(&self.copy(), &self.base_dir(), path)?;
            }
        }
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
                // A clock that did not move leaves the stamps as they are when
                // the bytes agree; marking them unknown would show a change
                // nobody made, from then on.
                if held()? && !fsops::same_bytes(&work, &copy)? {
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
        Ok(())
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

    /// Marks the task as having an agent at work (by this process) until the
    /// guard is dropped. Commit, rollback, undo and remove refuse meanwhile,
    /// whichever process asks: committing or rolling back under a running
    /// agent would split its work in two.
    pub fn agent_at_work(&self) -> Result<AgentAtWork> {
        let path = self.dir.join("agent");
        if let Some(pid) = self.agent_pid() {
            return Err(Error::Invalid(format!(
                "an agent is already at work in task {} (process {pid})",
                self.info.id
            )));
        }
        // Before anything at work reads the copy: its settings files hold
        // placeholders, not secrets (a person may have typed one in since).
        self.protect_secrets()?;
        let _ = fs::remove_file(&path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .at(&path)?;
        writeln!(file, "{}", std::process::id()).at(&path)?;
        Ok(AgentAtWork(path))
    }

    /// The process with an agent at work in the task, if it is still alive.
    pub fn agent_pid(&self) -> Option<u32> {
        let pid: u32 = fs::read_to_string(self.dir.join("agent"))
            .ok()?
            .trim()
            .parse()
            .ok()?;
        // SAFETY: signal 0 only asks whether the process exists.
        let alive = unsafe { libc::kill(pid as i32, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        alive.then_some(pid)
    }

    fn lock(&self) -> Result<Lock> {
        if let Some(pid) = self.agent_pid() {
            return Err(Error::Invalid(format!(
                "an agent is at work in task {} (process {pid}); wait until it is done",
                self.info.id
            )));
        }
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

/// An agent at work in a task, marked until dropped (see
/// [`Task::agent_at_work`]).
pub struct AgentAtWork(PathBuf);

impl Drop for AgentAtWork {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Where removed tasks wait to be deleted, out of the task list.
fn trash_dir(home: &Path) -> PathBuf {
    home.join("trash")
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
