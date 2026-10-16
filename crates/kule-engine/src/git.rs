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
        summary.refs.push(RefChange {
            name: name.clone(),
            old: old.cloned(),
            new: new.cloned(),
            fast_forward,
            commits,
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

fn oid(value: &str) -> Option<gix::ObjectId> {
    gix::ObjectId::from_hex(value.as_bytes()).ok()
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
