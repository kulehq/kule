//! `.git` judged as a database rather than as files.
//!
//! Most of `.git` changes without meaning anything: `git status` rewrites the
//! index to refresh stat data, every ref update appends to the logs. So paths
//! under `.git` are classified, conflicts on them are decided by what the
//! repository means (index entries, reference values), and the effect set
//! reports a summary a person can read instead of object file names.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::effects::{Change, Effect};
use crate::error::{Error, Result};
use crate::manifest::Kind;

/// Commit walks stop here; past it, counts and ancestry are reported unknown.
const WALK_LIMIT: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// Content-addressed: the same name always holds the same bytes.
    Object,
    /// `HEAD`, `packed-refs` and `refs/`.
    Ref,
    Index,
    /// Written by Git as a side effect: logs, `COMMIT_EDITMSG`.
    Derived,
    /// A lock a git process holds while it writes: one left in the copy is
    /// a crashed process's, and one in the workspace is a live process's.
    /// Never committed either way.
    Lock,
    /// Can make the next `git` command run code.
    Config,
    /// Runs code on the next matching `git` command.
    Hook,
    Other,
}

/// What a workspace path under `.git` is, or `None` outside `.git`.
pub(crate) fn classify(path: &str) -> Option<Part> {
    if path == ".git" {
        return Some(Part::Other);
    }
    let rest = path.strip_prefix(".git/")?;
    if rest.ends_with(".lock") {
        return Some(Part::Lock);
    }
    let parts: Vec<&str> = rest.split('/').collect();
    // Submodules and worktrees keep repositories of their own under
    // `.git/modules/<name>/` and `.git/worktrees/<name>/`: their hooks and
    // config run code just the same.
    if matches!(parts[0], "modules" | "worktrees") {
        let name = parts.last().copied().unwrap_or_default();
        return Some(if parts.contains(&"hooks") {
            Part::Hook
        } else if matches!(name, "config" | "config.worktree") {
            Part::Config
        } else {
            Part::Other
        });
    }
    Some(match parts[0] {
        // Alternates point the repository at another object store.
        "objects"
            if matches!(
                parts.get(1..),
                Some(["info", "alternates" | "http-alternates"])
            ) =>
        {
            Part::Config
        }
        "objects" => Part::Object,
        "HEAD" | "packed-refs" | "refs" => Part::Ref,
        "index" => Part::Index,
        "logs" | "COMMIT_EDITMSG" | "ORIG_HEAD" | "FETCH_HEAD" => Part::Derived,
        "config" | "config.worktree" => Part::Config,
        "hooks" => Part::Hook,
        _ => Part::Other,
    })
}

/// Whether reference `name` is listed in the repository's `packed-refs`.
pub(crate) fn is_packed(root: &Path, name: &str) -> Result<bool> {
    let path = root.join(".git/packed-refs");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => {
            return Err(Error::Io { path, source: e });
        }
    };
    Ok(text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .any(|line| line.split_once(' ').is_some_and(|(_, r)| r.trim() == name)))
}

/// What a repository means, as far as conflicts go.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitState {
    /// Digest of the index entries (path, mode, object id, stage). Stat data
    /// is left out, so a refresh by `git status` does not change it.
    pub index: Option<String>,
    /// `HEAD` and every reference: an object id, or `ref: <name>` when symbolic.
    pub refs: BTreeMap<String, String>,
}

/// The state of the repository at `root`, or `None` without a `.git` directory.
pub(crate) fn read(root: &Path) -> Result<Option<GitState>> {
    if !root.join(".git").is_dir() {
        return Ok(None);
    }
    let fail = |e: &dyn std::fmt::Display| Error::Git {
        path: root.to_path_buf(),
        message: e.to_string(),
    };
    let repo = open(root)?;
    let index = repo
        .try_index()
        .map_err(|e| fail(&e))?
        .map(|index| digest(&index));
    let mut refs = BTreeMap::new();
    if let Some(head) = repo.try_find_reference("HEAD").map_err(|e| fail(&e))? {
        refs.insert("HEAD".to_owned(), target(&head));
    }
    let platform = repo.references().map_err(|e| fail(&e))?;
    for reference in platform.all().map_err(|e| fail(&e))? {
        let reference = reference.map_err(|e| fail(&e))?;
        refs.insert(reference.name().as_bstr().to_string(), target(&reference));
    }
    Ok(Some(GitState { index, refs }))
}

/// Opens without reading global or system configuration: what Kule sees must
/// not depend on who runs it, and nothing configured there may run.
pub(crate) fn open(root: &Path) -> Result<gix::Repository> {
    gix::open_opts(root, gix::open::Options::isolated()).map_err(|e| Error::Git {
        path: root.to_path_buf(),
        message: e.to_string(),
    })
}

fn target(reference: &gix::Reference<'_>) -> String {
    match reference.target() {
        gix::refs::TargetRef::Object(id) => id.to_hex().to_string(),
        gix::refs::TargetRef::Symbolic(name) => format!("ref: {}", name.as_bstr()),
    }
}

fn digest(index: &gix::index::File) -> String {
    let mut hasher = blake3::Hasher::new();
    for entry in index.entries() {
        let path = entry.path(index);
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path);
        hasher.update(&entry.mode.bits().to_le_bytes());
        hasher.update(entry.id.as_bytes());
        hasher.update(&entry.stage_raw().to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

/// Whether the references a change at `path` depends on still hold their
/// expected values. A loose ref or `HEAD` is checked by itself; `packed-refs`
/// holds many, so all of them must match.
pub(crate) fn refs_untouched(
    path: &str,
    now: Option<&GitState>,
    expected: Option<&GitState>,
) -> bool {
    let (now, expected) = (now.map(|s| &s.refs), expected.map(|s| &s.refs));
    match path.strip_prefix(".git/") {
        Some(name) if name == "HEAD" || name.starts_with("refs/") => {
            now.and_then(|r| r.get(name)) == expected.and_then(|r| r.get(name))
        }
        _ => now == expected,
    }
}

pub(crate) fn index_untouched(now: Option<&GitState>, expected: Option<&GitState>) -> bool {
    now.map(|s| &s.index) == expected.map(|s| &s.index)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefChange {
    pub name: String,
    /// `None` when the reference is new.
    pub old: Option<String>,
    /// `None` when the reference was deleted.
    pub new: Option<String>,
    /// Whether `new` keeps `old` in its history. Unknown for symbolic
    /// references and for histories too long to walk.
    pub fast_forward: Option<bool>,
    /// Commits reachable from `new` but not from `old` (`git rev-list --count old..new`).
    pub commits: Option<usize>,
    /// The paths the commits it brings change, each against its first
    /// parent: commits no reference of the workspace held before. `None`
    /// when there were too many to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
    /// [`RefChange::paths`] with what each commit did there, and to what:
    /// the policy judges each as it judges the same change to a file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub carried: Vec<Carried>,
}

/// A change one of a reference's new commits makes, against its first
/// parent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Carried {
    pub path: String,
    pub change: Change,
    /// What the commit has there (what its parent had, for a deletion).
    pub entry: CarriedEntry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CarriedEntry {
    File,
    Symlink,
    /// Another repository's commit: what a clone of it would check out.
    Submodule,
}

/// The repository-level reading of the effects under `.git`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitSummary {
    pub refs: Vec<RefChange>,
    /// New files under `objects/`: loose objects and packs.
    pub object_files: usize,
    /// The index entries changed, not just their stat data.
    pub index: bool,
    pub config: bool,
    /// Hooks created, changed or deleted, by name.
    pub hooks: Vec<String>,
    /// Anything else under `.git` that changed.
    pub other: Vec<String>,
    /// Paths the task staged with content its file in the copy does not
    /// hold (or took out of the index while the file stays): the next
    /// `git commit` would record what the person never reviewed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub staged: Vec<String>,
}

impl GitSummary {
    pub fn is_empty(&self) -> bool {
        self == &GitSummary::default()
    }
}

/// Reads the effects under `.git` in the copy against the state they started
/// from. `None` when nothing meaningful changed.
pub(crate) fn summarize(
    copy: &Path,
    work: &Path,
    before: Option<&GitState>,
    effects: &[Effect],
) -> Result<Option<GitSummary>> {
    if !effects.iter().any(|e| classify(&e.path).is_some()) {
        return Ok(None);
    }
    let now = read(copy)?;
    let empty = GitState::default();
    let (old_state, new_state) = (before.unwrap_or(&empty), now.as_ref().unwrap_or(&empty));
    let repo = match now {
        Some(_) => Some(open(copy)?),
        None => None,
    };

    let mut summary = GitSummary {
        index: old_state.index != new_state.index,
        ..GitSummary::default()
    };
    if summary.index
        && let Some(repo) = &repo
    {
        summary.staged = staged_unlike_files(repo, copy, work)?;
    }
    let names: BTreeSet<&String> = old_state.refs.keys().chain(new_state.refs.keys()).collect();
    for name in names {
        let (old, new) = (old_state.refs.get(name), new_state.refs.get(name));
        if old == new {
            continue;
        }
        let (fast_forward, commits) =
            match (&repo, old.and_then(|o| oid(o)), new.and_then(|n| oid(n))) {
                (Some(repo), Some(old), Some(new)) => ancestry(repo, old, new),
                _ => (None, None),
            };
        // What the new commits carry, against everything the workspace's
        // references held before.
        let known: Vec<gix::ObjectId> = old_state.refs.values().filter_map(|v| oid(v)).collect();
        let carried = match (&repo, new.and_then(|n| oid(n))) {
            (Some(repo), Some(new)) => carried(repo, new, &known),
            _ => Some(Vec::new()),
        };
        let paths = carried.as_ref().map(|all| {
            let paths: BTreeSet<&String> = all.iter().map(|c| &c.path).collect();
            paths.into_iter().cloned().collect()
        });
        summary.refs.push(RefChange {
            name: name.clone(),
            old: old.cloned(),
            new: new.cloned(),
            fast_forward,
            commits,
            paths,
            carried: carried.unwrap_or_default(),
        });
    }
    for effect in effects {
        match classify(&effect.path) {
            Some(Part::Object) if effect.change == Change::Created && effect.kind == Kind::File => {
                summary.object_files += 1;
            }
            // Objects are written once and never change; one changed or
            // removed rewrites history's content, so it goes to a person.
            Some(Part::Object) if effect.kind != Kind::Dir => {
                summary.other.push(effect.path.clone());
            }
            Some(Part::Config) => summary.config = true,
            Some(Part::Hook) if effect.kind != Kind::Dir => {
                summary
                    .hooks
                    .push(effect.path.trim_start_matches(".git/hooks/").to_owned());
            }
            Some(Part::Other) => summary.other.push(effect.path.clone()),
            _ => {}
        }
    }
    Ok((!summary.is_empty()).then_some(summary))
}

/// The index entries of the copy's repository that differ from the
/// workspace's (so the task staged them) and that the copy's files do not
/// hold, plus the entries the task took out while their file stays.
fn staged_unlike_files(repo: &gix::Repository, copy: &Path, work: &Path) -> Result<Vec<String>> {
    let fail = |e: &dyn std::fmt::Display| Error::Git {
        path: copy.to_path_buf(),
        message: e.to_string(),
    };
    let Some(index) = repo.try_index().map_err(|e| fail(&e))? else {
        return Ok(Vec::new());
    };
    let entries =
        |index: &gix::index::File| -> BTreeMap<String, (gix::ObjectId, gix::index::entry::Mode)> {
            index
                .entries()
                .iter()
                .filter(|e| e.stage_raw() == 0)
                .map(|e| (e.path(index).to_string(), (e.id, e.mode)))
                .collect()
        };
    let ours = entries(&index);
    let theirs = match open(work).ok().and_then(|w| w.try_index().ok().flatten()) {
        Some(index) => entries(&index),
        None => BTreeMap::new(),
    };
    let mut staged = Vec::new();
    for (path, (id, mode)) in &ours {
        if theirs.get(path) == Some(&(*id, *mode)) {
            continue;
        }
        if !holds(copy, path, *id, *mode) {
            staged.push(path.clone());
        }
    }
    for path in theirs.keys().filter(|p| !ours.contains_key(*p)) {
        // Out of the index, still a file: the next commit deletes it.
        if crate::outline::open_beneath(copy, path).is_ok_and(|f| f.is_some()) {
            staged.push(path.clone());
        }
    }
    staged.sort();
    Ok(staged)
}

/// Whether the copy's file at `path` holds the staged object `id`. Read
/// without following links on the way, like any read of the copy.
fn holds(copy: &Path, path: &str, id: gix::ObjectId, mode: gix::index::entry::Mode) -> bool {
    use gix::index::entry::Mode;
    use std::io::Read;

    if mode == Mode::COMMIT {
        // A submodule's commit: not a file to compare.
        return true;
    }
    let data = if mode == Mode::SYMLINK {
        // The link itself is read; its parent must be reached without one.
        let parent_reached = match path.rsplit_once('/') {
            None => true,
            Some((parent, _)) => crate::outline::open_beneath(copy, parent)
                .is_ok_and(|d| d.is_some_and(|d| d.metadata().is_ok_and(|m| m.is_dir()))),
        };
        match std::fs::read_link(copy.join(path)) {
            Ok(target) if parent_reached => target.into_os_string().into_encoded_bytes(),
            _ => return false,
        }
    } else {
        let Ok(Some(file)) = crate::outline::open_beneath(copy, path) else {
            return false;
        };
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            return false;
        }
        let mut data = Vec::new();
        if file.take(MAX_HASHED).read_to_end(&mut data).is_err() || data.len() as u64 == MAX_HASHED
        {
            return false;
        }
        data
    };
    gix::objs::compute_hash(id.kind(), gix::objs::Kind::Blob, &data).is_ok_and(|h| h == id)
}

/// Larger files are not hashed; one staged from such a file is put to a
/// person rather than read whole.
const MAX_HASHED: u64 = 256 << 20;

fn oid(value: &str) -> Option<gix::ObjectId> {
    gix::ObjectId::from_hex(value.as_bytes()).ok()
}

/// The changes the commits reachable from `tip` and from none of `known`
/// make, each against its first parent; `None` past what is read.
fn carried(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    known: &[gix::ObjectId],
) -> Option<Vec<Carried>> {
    const MAX_PATHS: usize = 10_000;
    let walk = repo
        .rev_walk([tip])
        .with_hidden(known.iter().copied())
        .all()
        .ok()?;
    let mut paths = BTreeSet::new();
    for (n, info) in walk.enumerate() {
        if n >= WALK_LIMIT {
            return None;
        }
        let commit = repo
            .find_object(info.ok()?.id)
            .ok()?
            .try_into_commit()
            .ok()?;
        let tree = commit.tree_id().ok()?.detach();
        let parent = match commit.parent_ids().next() {
            Some(p) => Some(
                repo.find_object(p.detach())
                    .ok()?
                    .try_into_commit()
                    .ok()?
                    .tree_id()
                    .ok()?
                    .detach(),
            ),
            None => None,
        };
        tree_changes(repo, parent, Some(tree), "", &mut paths)?;
        if paths.len() > MAX_PATHS {
            return None;
        }
    }
    Some(paths.into_iter().collect())
}

/// The changes between trees `a` and `b` (either may be none), under
/// `prefix`: of content, and of mode alone (a file made executable, or
/// turned into a link to the same bytes).
fn tree_changes(
    repo: &gix::Repository,
    a: Option<gix::ObjectId>,
    b: Option<gix::ObjectId>,
    prefix: &str,
    out: &mut BTreeSet<Carried>,
) -> Option<()> {
    use gix::objs::tree::EntryKind;
    type Entries = BTreeMap<String, (EntryKind, gix::ObjectId)>;
    if a == b {
        return Some(());
    }
    let entries = |id: Option<gix::ObjectId>| -> Option<Entries> {
        let Some(id) = id else {
            return Some(BTreeMap::new());
        };
        let tree = repo.find_object(id).ok()?.try_into_tree().ok()?;
        let decoded = tree.decode().ok()?;
        Some(
            decoded
                .entries
                .iter()
                .map(|e| (e.filename.to_string(), (e.mode.kind(), e.oid.to_owned())))
                .collect(),
        )
    };
    let (left, right) = (entries(a)?, entries(b)?);
    let names: BTreeSet<&String> = left.keys().chain(right.keys()).collect();
    for name in names {
        let (l, r) = (left.get(name).copied(), right.get(name).copied());
        if l == r {
            continue;
        }
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let dir = |e: Option<(EntryKind, gix::ObjectId)>| {
            e.filter(|(kind, _)| *kind == EntryKind::Tree)
                .map(|(_, id)| id)
        };
        if dir(l).is_some() || dir(r).is_some() {
            tree_changes(repo, dir(l), dir(r), &path, out)?;
        }
        let leaf = |e: Option<(EntryKind, gix::ObjectId)>| {
            e.map(|(kind, _)| kind)
                .filter(|kind| *kind != EntryKind::Tree)
                .map(|kind| match kind {
                    EntryKind::Link => CarriedEntry::Symlink,
                    EntryKind::Commit => CarriedEntry::Submodule,
                    _ => CarriedEntry::File,
                })
        };
        let (change, entry) = match (leaf(l), leaf(r)) {
            (None, None) => continue,
            (Some(was), None) => (Change::Deleted, was),
            (None, Some(now)) => (Change::Created, now),
            (Some(_), Some(now)) => (Change::Modified, now),
        };
        out.insert(Carried {
            path,
            change,
            entry,
        });
    }
    Some(())
}

/// `(fast_forward, commits)` for a move from `old` to `new`.
fn ancestry(
    repo: &gix::Repository,
    old: gix::ObjectId,
    new: gix::ObjectId,
) -> (Option<bool>, Option<usize>) {
    let count = |tip: gix::ObjectId, hidden: gix::ObjectId| -> Option<usize> {
        let walk = repo.rev_walk([tip]).with_hidden([hidden]).all().ok()?;
        let mut n = 0;
        for info in walk {
            info.ok()?;
            n += 1;
            if n > WALK_LIMIT {
                return None;
            }
        }
        Some(n)
    };
    let commits = count(new, old);
    let behind = count(old, new);
    (behind.map(|b| b == 0), commits)
}
