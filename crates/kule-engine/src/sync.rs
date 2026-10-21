//! The folder's own changes, brought into a task's copy.
//!
//! A task's copy starts as the folder was at the last sync, and the folder
//! moves on meanwhile: a person edits it, another task commits to it.
//! [`Task::sync`] brings what the folder changed since into the copy, as a
//! commit takes the copy's changes to the folder: what only the folder
//! changed, the copy takes; a file both changed is merged three ways from
//! what both started with, by function, section or key (see
//! [`crate::entities`]); and when anything cannot be brought in, nothing
//! is. The folder is only read.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde::{Deserialize, Serialize};

use crate::effects::{self, Change, Effect};
use crate::error::{Error, IoContext, Result};
use crate::fsops::{self, read_json, write_json};
use crate::git::{self, GitState, Part};
use crate::ignore;
use crate::manifest::{Baseline, Kind, Stamp, Tree, scan};
use crate::outline;
use crate::secrets;
use crate::task::Task;

/// Where the paths a sync brought wait for the agent's next run.
const NEWS_FILE: &str = "synced.json";

/// The most paths the agent's note names; the rest are counted.
const NOTE_PATHS: usize = 40;

/// What a sync brought into the copy. Paths under `.git` are left out of
/// the lists: [`Synced::repository`] tells that the repository changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Synced {
    /// Paths the copy took as the folder has them.
    pub brought: Vec<String>,
    /// Files both changed, merged in the copy: the task's part stays a
    /// change of the task's.
    pub merged: Vec<String>,
    /// Paths both changed where the copy keeps its own: what the folder's
    /// ignore rules exclude (build output, caches) when it does not merge.
    pub kept: Vec<String>,
    /// The folder's repository changed (commits, branches, the index), and
    /// the copy's has that now.
    pub repository: bool,
}

impl Synced {
    pub fn is_empty(&self) -> bool {
        self.brought.is_empty() && self.merged.is_empty() && !self.repository
    }
}

/// What the folder changed since the last sync, as a sync would find it.
/// Paths under `.git` are left out, as in [`Synced`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behind {
    /// Paths the folder changed that a sync would bring in or merge.
    pub changed: Vec<String>,
    /// Of those, the ones the task changed too: merged, or a reason the
    /// sync refuses.
    pub both: Vec<String>,
    /// The folder's repository changed.
    pub repository: bool,
}

impl Behind {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && !self.repository
    }
}

/// What a sync does with one path the folder changed.
enum Action {
    /// Only the folder changed it: the copy takes it.
    Bring,
    /// Both hold the same now.
    Agree,
    /// Both changed one file: the copy takes `bytes`, merged with
    /// `theirs`, which the folder held with stamp `stamp`.
    Merge {
        bytes: Vec<u8>,
        theirs: Vec<u8>,
        stamp: Stamp,
    },
    /// Both changed it, the folder's ignore rules exclude it, or git wrote
    /// it on the side: the copy keeps its own.
    Keep,
    /// Its stamp moved, not what it holds.
    Restamp,
}

/// Everything a sync would do, decided before it does any of it.
struct Plan {
    /// The folder's entries when the plan was made.
    work: Tree,
    /// The folder's repository then.
    work_git: Option<GitState>,
    actions: Vec<(String, Action)>,
    /// Paths both changed that cannot be brought in.
    conflicts: Vec<String>,
}

/// What the agent hears of the syncs since its last run.
#[derive(Debug, Default, Serialize, Deserialize)]
struct News {
    brought: BTreeSet<String>,
    merged: BTreeSet<String>,
    repository: bool,
}

impl Task {
    /// Brings what the folder changed since the last sync into the copy
    /// (see the module documentation). Refuses with
    /// [`Error::BothChanged`] when the folder and the task changed a path
    /// in ways that do not merge; then nothing is brought in. What still
    /// runs in the task's sandbox is ended first, as before a commit, so
    /// nothing writes in the copy meanwhile. Steps start over: the copy no
    /// longer holds what any of them recorded.
    pub fn sync(&self) -> Result<Synced> {
        let _lock = self.lock()?;
        self.workspace_must_exist()?;
        self.no_terminal_open("sync")?;
        crate::Sandbox::end_all(self)?;
        let mut baseline = self.load_baseline()?;
        let plan = self.plan_sync(&baseline, true)?;
        if !plan.conflicts.is_empty() {
            return Err(Error::BothChanged(plan.conflicts));
        }

        let mut synced = Synced::default();
        let (mut brought, mut agreed, mut merges) = (Vec::new(), Vec::new(), Vec::new());
        for (path, action) in plan.actions {
            let outside = git::classify(&path).is_none();
            match action {
                Action::Bring => {
                    if outside {
                        synced.brought.push(path.clone());
                    } else {
                        synced.repository = true;
                    }
                    brought.push(path);
                }
                Action::Agree => agreed.push(path),
                Action::Merge {
                    bytes,
                    theirs,
                    stamp,
                } => merges.push((path, bytes, theirs, stamp)),
                Action::Keep => {
                    if outside {
                        synced.kept.push(path);
                    }
                }
                Action::Restamp => {
                    if let (Some(entry), Some(now)) =
                        (baseline.entries.get_mut(&path), plan.work.get(&path))
                    {
                        entry.work = now.clone();
                    }
                }
            }
        }

        // Merged files: the copy holds both changes, starting from what the
        // folder holds now, and keeps the task's part as its change.
        for (path, bytes, theirs, stamp) in &merges {
            fsops::replace_file(&self.copy().join(path), bytes)?;
            if self.base_dir().is_dir() {
                fsops::base_put(&self.base_dir(), path, theirs)?;
            }
            if let Some(entry) = baseline.entries.get_mut(path) {
                entry.work = stamp.clone();
            }
            synced.merged.push(path.clone());
        }

        // Parents before their children, as the plan has them.
        fsops::mirror(self.workspace(), &self.copy(), &brought, false)?;
        let mut settled: Vec<String> = brought.iter().chain(&agreed).cloned().collect();
        settled.sort();
        self.settle(&mut baseline, &settled)?;
        // Where the folder moved on while it was being read, the baseline
        // keeps the stamp the plan saw, which no longer matches: the next
        // sync brings it in again rather than missing it.
        for path in &brought {
            let now = Stamp::of(&self.workspace().join(path))?;
            if let (Some(seen), Some(entry)) = (plan.work.get(path), baseline.entries.get_mut(path))
                && now.as_ref() != Some(seen)
            {
                entry.work = seen.clone();
            }
        }
        // The repository as the plan read it, for the same reason: a
        // reference moved since would otherwise look like the task's move.
        baseline.git = plan.work_git;
        // The folder's settings files come in with their values: masked
        // before anything runs in the copy again.
        self.mask_copy(&mut baseline)?;
        self.save_baseline(&baseline)?;
        if !synced.is_empty() {
            self.clear_steps()?;
            self.add_news(&synced)?;
        }
        Ok(synced)
    }

    /// What the folder changed since the last sync, as [`Task::sync`]
    /// would find it now, without bringing anything in or merging.
    pub fn behind(&self) -> Result<Behind> {
        let baseline = self.load_baseline()?;
        let plan = self.plan_sync(&baseline, false)?;
        let mut behind = Behind::default();
        let both: BTreeSet<&str> = plan.conflicts.iter().map(String::as_str).collect();
        for (path, action) in &plan.actions {
            let counts = match action {
                Action::Bring => true,
                Action::Merge { .. } => {
                    behind.both.push(path.clone());
                    true
                }
                Action::Agree | Action::Keep | Action::Restamp => false,
            };
            if !counts {
                continue;
            }
            if git::classify(path).is_some() {
                behind.repository = true;
            } else {
                behind.changed.push(path.clone());
            }
        }
        for path in both {
            if git::classify(path).is_some() {
                behind.repository = true;
            } else {
                behind.changed.push(path.to_owned());
                behind.both.push(path.to_owned());
            }
        }
        behind.changed.sort();
        behind.changed.dedup();
        behind.both.sort();
        behind.both.dedup();
        Ok(behind)
    }

    /// Decides what a sync does with every path the folder changed. With
    /// `merge`, files both changed are merged now, and those that do not
    /// merge are conflicts; without it, they are planned as merges.
    fn plan_sync(&self, baseline: &Baseline, merge: bool) -> Result<Plan> {
        // The repository before the files: a commit made while they are
        // read shows as the folder's change next time, not as the task's.
        let work_git = git::read(self.workspace())?;
        let work = scan(self.workspace())?;
        let copy = effects::compute(&self.copy(), self.workspace(), baseline)?;
        let copy_git = git::read(&self.copy())?;
        let mut theirs = folder_changes(baseline, &work);
        ignore::mark(self.workspace(), &mut theirs)?;
        let ours: BTreeMap<&str, &Effect> =
            copy.effects.iter().map(|e| (e.path.as_str(), e)).collect();

        let mut actions = Vec::new();
        let mut conflicts = Vec::new();
        for change in &theirs {
            let path = change.path.as_str();
            let part = git::classify(path);
            if part == Some(Part::Lock) {
                continue;
            }
            if change.change == Change::Modified
                && self.same_as_synced(path, baseline, &work, work_git.as_ref())?
            {
                actions.push((change.path.clone(), Action::Restamp));
                continue;
            }
            if !touched(&ours, path) {
                actions.push((change.path.clone(), Action::Bring));
                continue;
            }
            if self.agree(path)? {
                actions.push((change.path.clone(), Action::Agree));
                continue;
            }
            let action = match part {
                // Named by their content: the same name holds the same
                // object, however it was compressed.
                Some(Part::Object) => {
                    if Stamp::of(&self.copy().join(path))?.is_some() {
                        Some(Action::Agree)
                    } else {
                        Some(Action::Keep)
                    }
                }
                // The task refreshed the index's stat data only: the
                // folder's entries come in.
                Some(Part::Index)
                    if git::index_untouched(copy_git.as_ref(), baseline.git.as_ref()) =>
                {
                    Some(Action::Bring)
                }
                Some(Part::Derived) => Some(Action::Keep),
                Some(_) => None,
                None => {
                    let both_modified = change.change == Change::Modified
                        && change.kind == Kind::File
                        && ours
                            .get(path)
                            .is_some_and(|e| e.change == Change::Modified && e.kind == Kind::File);
                    let ignored = change.ignored || ours.get(path).is_some_and(|e| e.ignored);
                    if both_modified && merge {
                        match self.merge_in(path, baseline)? {
                            Some(action) => Some(action),
                            None if ignored => Some(Action::Keep),
                            None => None,
                        }
                    } else if both_modified {
                        // Only planned: `behind` merges nothing.
                        Some(Action::Merge {
                            bytes: Vec::new(),
                            theirs: Vec::new(),
                            stamp: baseline.entries[path].work.clone(),
                        })
                    } else if ignored {
                        Some(Action::Keep)
                    } else {
                        None
                    }
                }
            };
            match action {
                Some(action) => actions.push((change.path.clone(), action)),
                None => conflicts.push(change.path.clone()),
            }
        }
        // A merge that failed names what both changed.
        let conflicts = conflicts
            .into_iter()
            .map(|path| match self.merge_conflict(&path, baseline, &ours) {
                Ok(Some(what)) => format!("{path} (both changed {what})"),
                _ => path,
            })
            .collect();
        Ok(Plan {
            work,
            work_git,
            actions,
            conflicts,
        })
    }

    /// Whether the folder's `path`, whose stamp moved, still holds what it
    /// held at the last sync: the same index entries or reference values,
    /// the same link, or the same bytes as the task's base.
    fn same_as_synced(
        &self,
        path: &str,
        baseline: &Baseline,
        work: &Tree,
        work_git: Option<&GitState>,
    ) -> Result<bool> {
        let (Some(entry), Some(now)) = (baseline.entries.get(path), work.get(path)) else {
            return Ok(false);
        };
        match git::classify(path) {
            Some(Part::Index) => return Ok(git::index_untouched(work_git, baseline.git.as_ref())),
            Some(Part::Ref) if now.kind != Kind::Dir => {
                return Ok(git::refs_untouched(path, work_git, baseline.git.as_ref()));
            }
            _ => {}
        }
        let was = &entry.work;
        if was.kind != now.kind || was.mode != now.mode {
            return Ok(false);
        }
        Ok(match now.kind {
            Kind::Dir => true,
            Kind::Symlink => was.link == now.link,
            Kind::File => {
                let base = self.base_dir().join(path);
                was.size == now.size
                    && !was.is_unknown()
                    && fs::symlink_metadata(&base).is_ok_and(|m| m.is_file())
                    && fsops::same_bytes(&base, &self.workspace().join(path))?
            }
        })
    }

    /// Whether the folder and the copy hold the same at `path` now.
    fn agree(&self, path: &str) -> Result<bool> {
        let (work, copy) = (self.workspace().join(path), self.copy().join(path));
        Ok(match (Stamp::of(&work)?, Stamp::of(&copy)?) {
            (None, None) => true,
            (Some(w), Some(c)) if w.kind == c.kind && w.mode == c.mode => match w.kind {
                Kind::Dir => true,
                Kind::Symlink => w.link == c.link,
                Kind::File => w.size == c.size && fsops::same_bytes(&work, &copy)?,
            },
            _ => false,
        })
    }

    /// Merges the copy's change to file `path` with the folder's, from what
    /// both started with: `None` when they do not merge or that is unknown.
    fn merge_in(&self, path: &str, baseline: &Baseline) -> Result<Option<Action>> {
        let Some(entry) = baseline.entries.get(path) else {
            return Ok(None);
        };
        let Some(base) = self.base_of(path, &entry.work)? else {
            return Ok(None);
        };
        let Some((theirs, stamp)) = self.read_still(path)? else {
            return Ok(None);
        };
        let Some(ours) = self.ours(path)? else {
            return Ok(None);
        };
        Ok(match crate::entities::merge(path, &base, &theirs, &ours) {
            // The task's change is in the folder's already.
            Ok(bytes) if bytes == theirs => Some(Action::Bring),
            Ok(bytes) => Some(Action::Merge {
                bytes,
                theirs,
                stamp,
            }),
            Err(_) => None,
        })
    }

    /// What both changed in file `path`, for a conflict's message.
    fn merge_conflict(
        &self,
        path: &str,
        baseline: &Baseline,
        ours: &BTreeMap<&str, &Effect>,
    ) -> Result<Option<String>> {
        let file = ours
            .get(path)
            .is_some_and(|e| e.change == Change::Modified && e.kind == Kind::File);
        if !file || git::classify(path).is_some() {
            return Ok(None);
        }
        let Some(entry) = baseline.entries.get(path) else {
            return Ok(None);
        };
        let (Some(base), Some((theirs, _)), Some(ours)) = (
            self.base_of(path, &entry.work)?,
            self.read_still(path)?,
            self.ours(path)?,
        ) else {
            return Ok(None);
        };
        Ok(crate::entities::merge(path, &base, &theirs, &ours)
            .err()
            .map(|c| c.what.join(", ")))
    }

    /// The folder's file `path` and the stamp it had while it was read;
    /// `None` when it is no file Kule merges. Refused when it changed
    /// while it was read.
    fn read_still(&self, path: &str) -> Result<Option<(Vec<u8>, Stamp)>> {
        let full = self.workspace().join(path);
        let Some(before) = Stamp::of(&full)?.filter(|s| s.kind == Kind::File) else {
            return Ok(None);
        };
        let Some(bytes) = outline::read_file(self.workspace(), path)? else {
            return Ok(None);
        };
        let clock = self.clock(self.workspace())?;
        let past = clock.wait_past(before.ctime)?;
        if Stamp::of(&full)?.as_ref() != Some(&before) {
            return Err(Error::Invalid(format!(
                "{path} changed in the folder while it was read; sync again"
            )));
        }
        // A write in the same tick of the clock would leave the stamp as
        // it was: such a stamp matches nothing, and the next sync reads
        // the file again.
        Ok(Some((bytes, if past { before } else { before.unknown() })))
    }

    /// The copy's file `path`, with the real values in a settings file:
    /// masked ones would read as changes to every secret line.
    fn ours(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(mut ours) = outline::read_file(&self.copy(), path)? else {
            return Ok(None);
        };
        if secrets::is_env_file(path)
            && let Ok(text) = std::str::from_utf8(&ours)
        {
            ours = self.vault()?.unmask(path, text).into_bytes();
        }
        Ok(Some(ours))
    }

    /// Refuses when the folder is gone: a sync would empty the copy.
    fn workspace_must_exist(&self) -> Result<()> {
        if self.workspace_exists() {
            return Ok(());
        }
        Err(Error::Invalid(format!(
            "the workspace {} is gone (moved or deleted outside Kule); nothing was brought in",
            self.workspace().display()
        )))
    }

    /// Adds what a sync brought to what the agent hears at its next run.
    fn add_news(&self, synced: &Synced) -> Result<()> {
        let path = self.dir().join(NEWS_FILE);
        let mut news: News = if path.is_file() {
            read_json(&path)?
        } else {
            News::default()
        };
        news.brought.extend(synced.brought.iter().cloned());
        news.merged.extend(synced.merged.iter().cloned());
        news.repository |= synced.repository;
        write_json(&path, &news)
    }

    /// What the agent should know at the start of its run about the folder
    /// changes brought into the copy since its last one, once: `None` when
    /// there were none.
    pub fn take_sync_note(&self) -> Result<Option<String>> {
        let path = self.dir().join(NEWS_FILE);
        if !path.is_file() {
            return Ok(None);
        }
        let news: News = read_json(&path)?;
        fs::remove_file(&path).at(&path)?;
        Ok(note(&news))
    }
}

/// The paths the folder changed since the last sync, as effects of the
/// folder, sorted so that parents come first.
fn folder_changes(baseline: &Baseline, work: &Tree) -> Vec<Effect> {
    let mut changes = Vec::new();
    for (path, entry) in &baseline.entries {
        let change = match work.get(path) {
            None => Change::Deleted,
            Some(now) if !entry.work.unchanged(now) => Change::Modified,
            Some(_) => continue,
        };
        changes.push(Effect {
            path: path.clone(),
            change,
            kind: work.get(path).map_or(entry.work.kind, |s| s.kind),
            ignored: false,
        });
    }
    for (path, now) in work {
        if !baseline.entries.contains_key(path) {
            changes.push(Effect {
                path: path.clone(),
                change: Change::Created,
                kind: now.kind,
                ignored: false,
            });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

/// Whether the copy changed `path`, anything under it, or a directory on
/// the way to it (deleted it, or put something else in its place).
fn touched(ours: &BTreeMap<&str, &Effect>, path: &str) -> bool {
    if ours.contains_key(path) {
        return true;
    }
    let under = format!("{path}/");
    if ours
        .range(under.as_str()..)
        .next()
        .is_some_and(|(p, _)| p.starts_with(&under))
    {
        return true;
    }
    let mut at = path;
    while let Some((parent, _)) = at.rsplit_once('/') {
        if ours
            .get(parent)
            .is_some_and(|e| e.change == Change::Deleted || e.kind != Kind::Dir)
        {
            return true;
        }
        at = parent;
    }
    false
}

/// The agent's note for `news`, or `None` when it holds nothing.
fn note(news: &News) -> Option<String> {
    if news.brought.is_empty() && news.merged.is_empty() && !news.repository {
        return None;
    }
    let list = |paths: &BTreeSet<String>| {
        let mut named: Vec<String> = paths
            .iter()
            .take(NOTE_PATHS)
            .map(|p| format!("`{}`", plain(p)))
            .collect();
        if paths.len() > NOTE_PATHS {
            named.push(format!("and {} more", paths.len() - NOTE_PATHS));
        }
        named.join(", ")
    };
    let mut note = String::from(
        "[Kule] Since your last turn, the folder this task works on changed (the person's \
         work, or another task's), and your working copy has those changes now.",
    );
    if !news.brought.is_empty() {
        note.push_str(&format!(" Changed in the folder: {}.", list(&news.brought)));
    }
    if !news.merged.is_empty() {
        note.push_str(&format!(
            " Files you had changed too, now merged with the folder's changes: {}.",
            list(&news.merged)
        ));
    }
    if news.repository {
        note.push_str(" The git repository changed too (commits, branches or staged files).");
    }
    note.push_str(" Read these files again before you change them.");
    Some(note)
}

/// `path` as one line of plain text: no control characters, and no
/// backticks to end the code span it is shown in.
fn plain(path: &str) -> String {
    path.chars()
        .map(|c| if c.is_control() || c == '`' { '?' } else { c })
        .collect()
}
